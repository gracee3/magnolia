use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

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

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct TranscriptionConfig {
    pub version: u32,
    pub sources: Vec<TranscriptionSourceConfig>,
    pub reconciliation: ReconciliationConfig,
    pub context: TranscriptionContextConfig,
}

impl Default for TranscriptionConfig {
    fn default() -> Self {
        Self {
            version: 1,
            sources: Vec::new(),
            reconciliation: ReconciliationConfig::default(),
            context: TranscriptionContextConfig::default(),
        }
    }
}

impl TranscriptionConfig {
    pub fn source(&self, id: &str) -> Option<&TranscriptionSourceConfig> {
        self.sources.iter().find(|source| source.id == id)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.version == 1,
            "unsupported transcription config version"
        );
        let mut ids = std::collections::HashSet::new();
        for source in &self.sources {
            anyhow::ensure!(
                !source.id.trim().is_empty(),
                "transcription source id is empty"
            );
            anyhow::ensure!(
                ids.insert(&source.id),
                "duplicate transcription source id: {}",
                source.id
            );
            anyhow::ensure!(
                (0.0..=1.0).contains(&source.trust),
                "source {} trust must be between 0 and 1",
                source.id
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct TranscriptionSourceConfig {
    pub id: String,
    pub provider: String,
    pub enabled: bool,
    pub priority: i32,
    pub trust: f32,
    pub mode: String,
    pub language: String,
    pub model: Option<String>,
    pub model_dir: Option<PathBuf>,
    pub num_threads: Option<i32>,
    pub endpointing: Option<bool>,
    pub delay: Option<String>,
}

impl Default for TranscriptionSourceConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            provider: String::new(),
            enabled: false,
            priority: 0,
            trust: 0.5,
            mode: "realtime".into(),
            language: "en".into(),
            model: None,
            model_dir: None,
            num_threads: None,
            endpointing: None,
            delay: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ReconciliationConfig {
    pub enabled: bool,
    pub strategy: String,
    pub provisional_min_trust: f32,
    pub final_min_trust: f32,
    pub correction_window_ms: u64,
    pub deleted_word_linger_ms: u64,
}

impl Default for ReconciliationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            strategy: "priority_weighted_alignment".into(),
            provisional_min_trust: 0.0,
            final_min_trust: 0.8,
            correction_window_ms: 4_000,
            deleted_word_linger_ms: 800,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct TranscriptionContextConfig {
    pub enabled: bool,
    pub project_root: PathBuf,
    pub include: Vec<String>,
    pub explicit_terms: Vec<String>,
    pub max_terms: usize,
}

impl Default for TranscriptionContextConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            project_root: PathBuf::from("."),
            include: Vec::new(),
            explicit_terms: Vec::new(),
            max_terms: 2_000,
        }
    }
}

pub fn read_transcription_config() -> anyhow::Result<TranscriptionConfig> {
    let configured = std::env::var_os("MAGNOLIA_TRANSCRIPTION_CONFIG").map(PathBuf::from);
    let candidates = configured.into_iter().chain([
        PathBuf::from("config/transcription.toml"),
        PathBuf::from("../../config/transcription.toml"),
    ]);
    for path in candidates {
        if path.is_file() {
            return read_transcription_config_from(&path);
        }
    }
    anyhow::bail!("could not find config/transcription.toml")
}

pub fn read_transcription_config_from(path: &Path) -> anyhow::Result<TranscriptionConfig> {
    let text = fs::read_to_string(path)?;
    let config: TranscriptionConfig = toml::from_str(&text)?;
    config.validate()?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_checked_in_transcription_config() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/transcription.toml");
        let config = read_transcription_config_from(&path).unwrap();
        assert!(config.source("sherpa_local").unwrap().enabled);
        assert!(!config.source("openai_realtime").unwrap().enabled);
    }
}
