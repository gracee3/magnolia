#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use magnolia_audio::{CaptureConfiguration, CaptureState, PipeWireCapture};
    use std::{env, thread, time::Duration};

    let target = env::args()
        .nth(1)
        .ok_or("usage: capture_probe NODE_NAME [SECONDS]")?;
    let seconds = env::args()
        .nth(2)
        .map_or(Ok(5_u64), |value| value.parse())?;
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
    println!("{snapshot:?}");
    if snapshot.state != CaptureState::Running || snapshot.callbacks == 0 {
        return Err("PipeWire capture did not reach running callback state".into());
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("PipeWire capture is available only on Linux");
}
