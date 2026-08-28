use std::{
    env,
    path::{Path, PathBuf},
    process::Stdio,
};
use tempfile::TempDir;
use thiserror::Error;
use tokio::process::{Child, Command};

pub struct BrowserProcess {
    child: Child,
    _profile: TempDir,
}

impl BrowserProcess {
    pub fn launch(url: &str, override_path: Option<&Path>) -> Result<Self, BrowserLaunchError> {
        let executable = discover_chromium(override_path)?;
        let profile = tempfile::Builder::new()
            .prefix("magnolia-chromium-")
            .tempdir()
            .map_err(BrowserLaunchError::Profile)?;
        let child = Command::new(&executable)
            .arg(format!("--app={url}"))
            .arg(format!("--user-data-dir={}", profile.path().display()))
            .args([
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-background-networking",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|source| BrowserLaunchError::Spawn { executable, source })?;
        Ok(Self {
            child,
            _profile: profile,
        })
    }

    pub async fn shutdown(mut self) -> Result<(), BrowserLaunchError> {
        if self
            .child
            .try_wait()
            .map_err(BrowserLaunchError::Wait)?
            .is_none()
        {
            self.child.kill().await.map_err(BrowserLaunchError::Wait)?;
            let _ = self.child.wait().await;
        }
        Ok(())
    }
}

pub fn discover_chromium(override_path: Option<&Path>) -> Result<PathBuf, BrowserLaunchError> {
    if let Some(path) = override_path {
        return executable_path(path).ok_or_else(|| BrowserLaunchError::InvalidOverride {
            path: path.to_path_buf(),
        });
    }
    if let Some(path) = env::var_os("MAGNOLIA_CHROMIUM").map(PathBuf::from) {
        return executable_path(&path)
            .ok_or(BrowserLaunchError::InvalidEnvironmentOverride { path });
    }
    for candidate in [
        "chromium",
        "chromium-browser",
        "google-chrome",
        "google-chrome-stable",
    ] {
        if let Some(path) = find_on_path(candidate) {
            return Ok(path);
        }
    }
    Err(BrowserLaunchError::NotFound)
}

fn executable_path(path: &Path) -> Option<PathBuf> {
    path.is_file().then(|| path.to_path_buf())
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| env::split_paths(&paths).collect::<Vec<_>>())
        .map(|directory| directory.join(name))
        .find(|path| path.is_file())
}

#[derive(Debug, Error)]
pub enum BrowserLaunchError {
    #[error("Chromium was not found; install Chromium or pass --chromium PATH")]
    NotFound,
    #[error("--chromium path is not a file: {path}")]
    InvalidOverride { path: PathBuf },
    #[error("MAGNOLIA_CHROMIUM path is not a file: {path}")]
    InvalidEnvironmentOverride { path: PathBuf },
    #[error("could not create the dedicated Magnolia Chromium profile: {0}")]
    Profile(std::io::Error),
    #[error("could not launch Chromium at {executable}: {source}")]
    Spawn {
        executable: PathBuf,
        source: std::io::Error,
    },
    #[error("could not stop Chromium cleanly: {0}")]
    Wait(std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_explicit_override_is_actionable() {
        let path = Path::new("/definitely/not/a/chromium/binary");
        assert!(matches!(
            discover_chromium(Some(path)),
            Err(BrowserLaunchError::InvalidOverride { .. })
        ));
    }
}
