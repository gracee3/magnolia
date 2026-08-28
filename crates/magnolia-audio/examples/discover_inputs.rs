#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    for device in magnolia_audio::pipewire::discover_inputs()? {
        println!(
            "{}\t{}\t{}",
            device.global_id,
            device.node_name,
            device.description.as_deref().unwrap_or("")
        );
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("PipeWire discovery is available only on Linux");
}
