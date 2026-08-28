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
    use std::{env, thread, time::Duration};

    let target = env::args()
        .nth(1)
        .ok_or("usage: capture_probe NODE_NAME [SECONDS]")?;
    let seconds = env::args()
        .nth(2)
        .map_or(Ok(5_u64), |value| value.parse())?;
    reset_callback_allocation_counts();
    let capture = PipeWireCapture::start(CaptureConfiguration {
        target_node_name: target,
    })?;
    for _ in 0..seconds.saturating_mul(10) {
        thread::sleep(Duration::from_millis(100));
        let snapshot = capture.snapshot();
        if snapshot.state == CaptureState::Failed {
            return Err("PipeWire capture entered the failed state".into());
        }
    }
    let snapshot = capture.snapshot();
    let allocation_counts = callback_allocation_counts();
    println!("{snapshot:?} callback_allocations={allocation_counts:?}");
    if snapshot.state != CaptureState::Running || snapshot.callbacks == 0 {
        return Err("PipeWire capture did not reach running callback state".into());
    }
    if allocation_counts != (0, 0) {
        return Err("capture callback allocated or deallocated".into());
    }
    let quantum_ns = u64::from(snapshot.quantum_frames).saturating_mul(1_000_000_000)
        / u64::from(snapshot.sample_rate);
    if snapshot.callback_p99_ns.saturating_mul(4) >= quantum_ns
        || snapshot.callback_p999_ns.saturating_mul(2) >= quantum_ns
    {
        return Err("capture callback percentile exceeded the Phase 4 quantum budget".into());
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("PipeWire capture is available only on Linux");
}
