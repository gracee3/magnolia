use magnolia_desktop::{HostConfiguration, MagnoliaHost};
use std::{env, path::PathBuf};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with_target(false)
        .init();

    let mut configuration = HostConfiguration::default();
    let mut arguments = env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--assets" => {
                configuration.assets =
                    PathBuf::from(arguments.next().ok_or("--assets requires a directory")?);
            }
            "--port" => {
                configuration.port = arguments
                    .next()
                    .ok_or("--port requires a number")?
                    .parse()?;
            }
            "--chromium" => {
                configuration.chromium = Some(PathBuf::from(
                    arguments.next().ok_or("--chromium requires a path")?,
                ));
            }
            "--no-browser" => configuration.launch_browser = false,
            "--test-mode" => {
                configuration.test_mode = true;
                configuration.auto_activate = false;
            }
            "--help" | "-h" => {
                println!(
                    "magnolia-desktop [--assets DIR] [--port PORT] [--chromium PATH] [--no-browser] [--test-mode]"
                );
                return Ok(());
            }
            unknown => return Err(format!("unknown argument: {unknown}").into()),
        }
    }

    let test_mode = configuration.test_mode;
    let launch_browser = configuration.launch_browser;
    let host = MagnoliaHost::start(configuration).await?;
    if test_mode {
        println!(
            "MAGNOLIA_READY {}",
            serde_json::to_string(&host.ready_info())?
        );
    } else if !launch_browser {
        println!("Open this local URL manually: {}", host.launch_url());
    } else {
        println!("Magnolia cockpit is ready at {}", host.ready_info().origin);
    }
    if let Some(error) = host.browser_launch_error() {
        eprintln!("Chromium was not launched: {error}");
        eprintln!("Open this local URL manually: {}", host.launch_url());
    }
    tokio::signal::ctrl_c().await?;
    host.shutdown().await?;
    Ok(())
}
