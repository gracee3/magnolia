#[cfg(target_os = "linux")]
#[global_allocator]
static ALLOCATOR: magnolia_audio::CallbackCountingAllocator =
    magnolia_audio::CallbackCountingAllocator;

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use magnolia_audio::{
        callback_allocation_counts, reset_callback_allocation_counts, CaptureConfiguration,
        CaptureState, PipeWireCapture,
    };
    use magnolia_observe::{AnalyzerFrame, AnalyzerKind, ObservationHub};
    use std::{env, thread, time::Duration};

    let target = env::args()
        .nth(1)
        .ok_or("usage: live_observation_probe NODE_NAME [SECONDS]")?;
    let seconds = env::args()
        .nth(2)
        .map_or(Ok(30_u64), |value| value.parse())?;
    reset_callback_allocation_counts();
    let hub = ObservationHub::default();
    for kind in AnalyzerKind::ALL {
        hub.acquire(kind);
    }
    let mut capture = PipeWireCapture::start(CaptureConfiguration {
        target_node_name: target,
    })?;
    let edge = capture
        .take_analysis_edge()
        .ok_or("analysis edge was unavailable")?;
    let worker = hub.attach(edge, 48_000, 2)?;
    for _ in 0..seconds.saturating_mul(10) {
        thread::sleep(Duration::from_millis(100));
        if capture.snapshot().state == CaptureState::Failed {
            return Err("PipeWire capture entered the failed state".into());
        }
    }
    let capture_snapshot = capture.snapshot();
    let status = hub.status();
    let allocations = callback_allocation_counts();
    for kind in AnalyzerKind::ALL {
        let frame = hub.latest(kind).ok_or("leased analyzer emitted no frame")?;
        if matches!(frame, AnalyzerFrame::Spectrum(ref spectrum) if spectrum.bins_db.len() != 1_025)
        {
            return Err("spectrum frame did not contain 1025 real FFT bins".into());
        }
    }
    let maximum_p95 = status
        .latency_p95_ns
        .values()
        .copied()
        .max()
        .unwrap_or(u64::MAX);
    println!(
        "OBSERVATION capture={capture_snapshot:?} status={status:?} callback_allocations={allocations:?}"
    );
    if capture_snapshot.state != CaptureState::Running || status.processed_blocks == 0 {
        return Err("native observation did not process live blocks".into());
    }
    if status.ring_faults != 0 || capture_snapshot.dropped_frames != 0 {
        return Err("native observation reported a ring fault or dropped frame".into());
    }
    if allocations != (0, 0) {
        return Err("capture callback allocated or deallocated while analyzers were active".into());
    }
    if maximum_p95 > 33_300_000 {
        return Err("dense analyzer p95 exceeded 33.3 ms".into());
    }
    drop(worker);
    for kind in AnalyzerKind::ALL {
        hub.release(kind);
    }
    if hub.status().active_leases != 0 {
        return Err("analyzer leases survived explicit release".into());
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("live observation is available only on Linux");
}
