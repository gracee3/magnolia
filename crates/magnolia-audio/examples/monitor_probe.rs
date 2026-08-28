#[cfg(target_os = "linux")]
#[global_allocator]
static ALLOCATOR: magnolia_audio::CallbackCountingAllocator =
    magnolia_audio::CallbackCountingAllocator;

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use magnolia_audio::{
        callback_allocation_counts, pipewire::PipeWireRegistryManager,
        reset_callback_allocation_counts, CaptureConfiguration, CaptureState, OutputConfiguration,
        PipeWireCapture, PipeWireOutput,
    };
    use std::{env, thread, time::Duration};

    let source = env::args()
        .nth(1)
        .ok_or("usage: monitor_probe SOURCE_NODE")?;
    reset_callback_allocation_counts();
    let registry = PipeWireRegistryManager::start()?;
    let mut output_name = None;
    for _ in 0..50 {
        thread::sleep(Duration::from_millis(100));
        output_name = registry
            .snapshot()
            .default_output()
            .ok()
            .map(|device| device.fingerprint.node_name.clone());
        if output_name.is_some() {
            break;
        }
    }
    let mut capture = PipeWireCapture::start(CaptureConfiguration {
        target_node_name: source,
    })?;
    for _ in 0..50 {
        thread::sleep(Duration::from_millis(100));
        if capture.snapshot().state == CaptureState::Running {
            break;
        }
    }
    let consumer = capture
        .take_monitor_edge()
        .ok_or("capture graph edge unavailable")?;
    let output = PipeWireOutput::start(
        OutputConfiguration {
            target_node_name: output_name.ok_or("default output unresolved")?,
        },
        consumer,
    )?;
    output.set_gain_millionths(0);
    output.set_muted(true);
    thread::sleep(Duration::from_secs(3));
    let capture_snapshot = capture.snapshot();
    let output_snapshot = output.snapshot();
    let allocation_counts = callback_allocation_counts();
    println!(
        "capture={capture_snapshot:?}\noutput={output_snapshot:?}\ncallback_allocations={allocation_counts:?}"
    );
    if !output_snapshot.running || output_snapshot.callbacks == 0 {
        return Err("muted PipeWire output did not reach callback state".into());
    }
    if allocation_counts != (0, 0) {
        return Err("capture or output callback allocated or deallocated".into());
    }
    if capture_snapshot.faults != 0
        || capture_snapshot.dropped_frames != 0
        || output_snapshot.underruns != 0
    {
        return Err("muted monitoring reported a callback fault, drop, or underrun".into());
    }
    let output_quantum_ns = 256_u64.saturating_mul(1_000_000_000) / 48_000;
    if output_snapshot.callback_p99_ns.saturating_mul(4) >= output_quantum_ns
        || output_snapshot.callback_p999_ns.saturating_mul(2) >= output_quantum_ns
    {
        return Err("output callback percentile exceeded the Phase 4 quantum budget".into());
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("PipeWire monitoring is available only on Linux");
}
