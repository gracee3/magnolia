use std::fs;

/// Matches the daemon behavior: try common relative paths for `configs/layout.toml`.
pub fn read_layout_toml_text() -> anyhow::Result<String> {
    let paths = ["configs/layout.toml", "../../configs/layout.toml"];
    for p in &paths {
        if let Ok(c) = fs::read_to_string(p) {
            return Ok(c);
        }
    }
    anyhow::bail!("Could not load layout.toml from {:?}", paths);
}
