#[cfg(target_os = "linux")]
#[global_allocator]
static ALLOCATOR: magnolia_audio::CallbackCountingAllocator =
    magnolia_audio::CallbackCountingAllocator;

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use magnolia_application::{ActivationRequest, RuntimeControl, RuntimeEvent, RuntimePort};
    use magnolia_audio::{
        callback_allocation_counts, pipewire::PipeWireRegistryManager,
        reset_callback_allocation_counts,
    };
    use magnolia_domain::{DeviceSelector, OperationId, TargetGraphRevision, WorkspaceGraph};
    use magnolia_protocol::AudioRuntimeState;
    use magnolia_runtime::NativeRuntime;
    use std::{
        collections::BTreeMap,
        env,
        io::{self, Write},
        thread,
        time::{Duration, Instant},
    };

    let node_name = env::args()
        .nth(1)
        .ok_or("usage: recovery_probe NODE_NAME")?;
    let discovery = PipeWireRegistryManager::start()?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let fingerprint = loop {
        if let Some(fingerprint) = discovery
            .snapshot()
            .devices()
            .find(|device| device.fingerprint.node_name == node_name)
            .map(|device| device.fingerprint.clone())
        {
            break fingerprint;
        }
        if Instant::now() >= deadline {
            return Err("temporary PipeWire source was not discovered".into());
        }
        thread::sleep(Duration::from_millis(50));
    };

    reset_callback_allocation_counts();
    let mut runtime = NativeRuntime::new();
    runtime.enqueue_activation(ActivationRequest {
        operation_id: OperationId::from_u128(1),
        target_graph_revision: TargetGraphRevision::new(1),
        graph: WorkspaceGraph::default(),
        device_selectors: BTreeMap::from([(
            "audio.input".to_owned(),
            DeviceSelector::Exact { fingerprint },
        )]),
    });
    wait_for(&mut runtime, Duration::from_secs(5), |event| {
        matches!(event, RuntimeEvent::ActivationSucceeded { .. })
    })?;
    runtime.enqueue_control(RuntimeControl::StartAudio);
    let running = wait_for_audio(&mut runtime, Duration::from_secs(10), |audio| {
        audio.state == AudioRuntimeState::Running && audio.callback_count > 0
    })?;
    println!(
        "READY callbacks={} discontinuities={}",
        running.callback_count, running.discontinuities
    );
    io::stdout().flush()?;

    let degraded = wait_for_audio(&mut runtime, Duration::from_secs(15), |audio| {
        audio.state == AudioRuntimeState::Degraded && audio.discontinuities > 0
    })?;
    println!(
        "DEGRADED callbacks={} discontinuities={}",
        degraded.callback_count, degraded.discontinuities
    );
    io::stdout().flush()?;

    let recovered = wait_for_audio(&mut runtime, Duration::from_secs(15), |audio| {
        audio.state == AudioRuntimeState::Running
            && audio.callback_count > degraded.callback_count
            && audio.discontinuities > degraded.discontinuities
    })?;
    runtime.enqueue_control(RuntimeControl::SetCaptureMuted(true));
    thread::sleep(Duration::from_millis(100));
    runtime.enqueue_control(RuntimeControl::SetCaptureMuted(false));
    runtime.enqueue_control(RuntimeControl::SetMonitorEnabled(true));
    runtime.enqueue_control(RuntimeControl::SetMonitorGain(30_000));
    runtime.enqueue_control(RuntimeControl::SetMonitorMuted(false));
    thread::sleep(Duration::from_millis(250));
    runtime.enqueue_control(RuntimeControl::SetMonitorMuted(true));
    runtime.enqueue_control(RuntimeControl::SetMonitorGain(0));
    runtime.enqueue_control(RuntimeControl::SetMonitorEnabled(false));
    let _ = wait_for_audio(&mut runtime, Duration::from_secs(5), |audio| {
        !audio.monitor_enabled && audio.monitor_muted && audio.monitor_gain_millionths == 0
    })?;
    println!("CONTROLS capture_mute_cycle=true monitor_peak_gain=0.03");
    runtime.enqueue_control(RuntimeControl::StopAudio);
    let _ = wait_for_audio(&mut runtime, Duration::from_secs(5), |audio| {
        audio.state == AudioRuntimeState::Stopped
    })?;
    println!(
        "RECOVERED callbacks={} discontinuities={} callback_allocations={:?}",
        recovered.callback_count,
        recovered.discontinuities,
        callback_allocation_counts()
    );
    if callback_allocation_counts() != (0, 0) {
        return Err("capture callback allocated or deallocated during recovery".into());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn wait_for(
    runtime: &mut impl magnolia_application::RuntimePort,
    timeout: std::time::Duration,
    mut accept: impl FnMut(&magnolia_application::RuntimeEvent) -> bool,
) -> Result<magnolia_application::RuntimeEvent, Box<dyn std::error::Error>> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Some(event) = runtime.poll_event() {
            if accept(&event) {
                return Ok(event);
            }
        } else {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if std::time::Instant::now() >= deadline {
            return Err("native runtime event deadline expired".into());
        }
    }
}

#[cfg(target_os = "linux")]
fn wait_for_audio(
    runtime: &mut impl magnolia_application::RuntimePort,
    timeout: std::time::Duration,
    mut accept: impl FnMut(&magnolia_protocol::AudioRuntimeProjection) -> bool,
) -> Result<magnolia_protocol::AudioRuntimeProjection, Box<dyn std::error::Error>> {
    let event = wait_for(
        runtime,
        timeout,
        |event| matches!(event, magnolia_application::RuntimeEvent::AudioProjection(audio) if accept(audio)),
    )?;
    let magnolia_application::RuntimeEvent::AudioProjection(audio) = event else {
        return Err("accepted runtime event was not an audio projection".into());
    };
    Ok(audio)
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("PipeWire recovery is available only on Linux");
}
