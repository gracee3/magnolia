use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ParakeetSttSettings {
    pub model_dir: PathBuf,
    pub device: u32,
    pub vocab_path: PathBuf,
    pub streaming_encoder_path: Option<PathBuf>,
    pub use_fp16: bool,
    pub chunk_frames: Option<usize>,
    pub advance_frames: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
struct ParakeetSttToml {
    model_dir: PathBuf,
    #[serde(default = "default_device")]
    device: u32,
    #[serde(default)]
    streaming_encoder_path: Option<PathBuf>,
    #[serde(default)]
    use_fp16: bool,
    #[serde(default)]
    chunk_frames: Option<usize>,
    #[serde(default)]
    advance_frames: Option<usize>,
}

fn default_device() -> u32 {
    0
}

#[derive(Debug, Clone, Deserialize)]
struct RootConfigToml {
    #[serde(default)]
    parakeet_stt: Option<ParakeetSttToml>,
}

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

pub fn resolve_vocab_path(model_dir: &Path) -> Option<PathBuf> {
    let direct = model_dir.join("vocab.txt");
    if direct.exists() {
        return Some(direct);
    }
    // Mirror Parakeet runtime fallback: <repo_root>/tools/export_onnx/out/vocab.txt
    // If model_dir is ".../models/<name>", repo_root is ".../".
    let repo_root = model_dir
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf());
    if let Some(root) = repo_root {
        let fallback = root.join("tools").join("export_onnx").join("out").join("vocab.txt");
        if fallback.exists() {
            return Some(fallback);
        }
    }
    None
}

pub fn validate_parakeet_assets(model_dir: &Path) -> anyhow::Result<PathBuf> {
    if !model_dir.exists() {
        anyhow::bail!("parakeet_stt.model_dir does not exist: {}", model_dir.display());
    }
    let encoder = model_dir.join("encoder.engine");
    let predictor = model_dir.join("predictor.engine");
    let joint = model_dir.join("joint.engine");
    for p in [&encoder, &predictor, &joint] {
        if !p.exists() {
            anyhow::bail!("Missing engine file: {}", p.display());
        }
    }
    let vocab = resolve_vocab_path(model_dir).ok_or_else(|| {
        anyhow::anyhow!(
            "Missing vocab.txt (expected {} or repo fallback tools/export_onnx/out/vocab.txt)",
            model_dir.join("vocab.txt").display()
        )
    })?;
    Ok(vocab)
}

pub fn load_parakeet_stt_settings() -> anyhow::Result<ParakeetSttSettings> {
    let text = read_layout_toml_text()?;
    let root: RootConfigToml = toml::from_str(&text)
        .map_err(|e| anyhow::anyhow!("Failed to parse layout.toml for parakeet_stt settings: {e}"))?;
    let cfg = root.parakeet_stt.ok_or_else(|| {
        anyhow::anyhow!("Missing [parakeet_stt] config in layout.toml (needs model_dir, optional device)")
    })?;
    let vocab = validate_parakeet_assets(&cfg.model_dir)?;
    let streaming_encoder_path = if let Some(path) = cfg.streaming_encoder_path {
        if !path.exists() {
            anyhow::bail!("parakeet_stt.streaming_encoder_path does not exist: {}", path.display());
        }
        Some(path)
    } else {
        None
    };
    Ok(ParakeetSttSettings {
        model_dir: cfg.model_dir,
        device: cfg.device,
        vocab_path: vocab,
        streaming_encoder_path,
        use_fp16: cfg.use_fp16,
        chunk_frames: cfg.chunk_frames,
        advance_frames: cfg.advance_frames,
    })
}
