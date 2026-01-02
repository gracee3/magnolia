use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub id: String,
    pub wav: String,
    pub text: String,
}

fn is_wav(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("wav"))
        .unwrap_or(false)
}

fn utterance_id_from_wav_path(wav: &Path) -> anyhow::Result<String> {
    let stem = wav
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("Invalid WAV filename: {}", wav.display()))?;
    Ok(stem.to_string())
}

fn transcript_path_for_utterance(wav: &Path, utt_id: &str) -> anyhow::Result<PathBuf> {
    // LibriSpeech convention: <speaker>-<chapter>-<utt>
    // Transcript file in same directory: <speaker>-<chapter>.trans.txt
    let parts: Vec<&str> = utt_id.split('-').collect();
    if parts.len() != 3 {
        anyhow::bail!(
            "Unexpected utterance id format (expected SPEAKER-CHAPTER-UTT): {}",
            utt_id
        );
    }
    let spk = parts[0];
    let chap = parts[1];
    Ok(wav
        .parent()
        .ok_or_else(|| anyhow::anyhow!("WAV has no parent dir: {}", wav.display()))?
        .join(format!("{spk}-{chap}.trans.txt")))
}

fn parse_transcript_file(path: &Path) -> anyhow::Result<HashMap<String, String>> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("Failed to read transcript file {}", path.display()))?;
    let mut map = HashMap::new();
    for (lineno, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut it = line.split_whitespace();
        let id = it
            .next()
            .ok_or_else(|| anyhow::anyhow!("Invalid transcript line {}: empty", lineno + 1))?;
        let rest = it.collect::<Vec<_>>().join(" ");
        if rest.is_empty() {
            anyhow::bail!(
                "Invalid transcript line {} in {}: missing text for id {}",
                lineno + 1,
                path.display(),
                id
            );
        }
        map.insert(id.to_string(), rest);
    }
    Ok(map)
}

fn wav_path_string(repo_root: Option<&Path>, wav: &Path) -> String {
    if let Some(root) = repo_root {
        if let Ok(rel) = wav.strip_prefix(root) {
            return rel.to_string_lossy().to_string();
        }
    }
    wav.to_string_lossy().to_string()
}

pub fn build_manifest(dataset_root: &Path) -> anyhow::Result<Vec<ManifestEntry>> {
    let repo_root = std::env::current_dir().ok();
    let mut manifest = Vec::new();
    let mut transcript_cache: HashMap<PathBuf, HashMap<String, String>> = HashMap::new();

    for ent in WalkDir::new(dataset_root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !ent.file_type().is_file() {
            continue;
        }
        let p = ent.path();
        if !is_wav(p) {
            continue; // explicitly ignore .flac and everything else
        }
        let utt_id = utterance_id_from_wav_path(p)?;
        let trans_path = transcript_path_for_utterance(p, &utt_id)?;
        if !trans_path.exists() {
            anyhow::bail!(
                "Missing transcript for {} (expected {})",
                p.display(),
                trans_path.display()
            );
        }
        if !transcript_cache.contains_key(&trans_path) {
            let parsed = parse_transcript_file(&trans_path)?;
            transcript_cache.insert(trans_path.clone(), parsed);
        }
        let trans_map = transcript_cache
            .get(&trans_path)
            .expect("transcript_cache just populated");

        let text = trans_map.get(&utt_id).ok_or_else(|| {
            anyhow::anyhow!(
                "Transcript file {} missing entry for utterance id {} (wav {})",
                trans_path.display(),
                utt_id,
                p.display()
            )
        })?;

        manifest.push(ManifestEntry {
            id: utt_id,
            wav: wav_path_string(repo_root.as_deref(), p),
            text: text.clone(),
        });
    }

    if manifest.is_empty() {
        anyhow::bail!("No .wav files found under {}", dataset_root.display());
    }

    Ok(manifest)
}

pub fn write_manifest_jsonl(path: &Path, entries: &[ManifestEntry]) -> anyhow::Result<()> {
    let mut out = String::new();
    for e in entries {
        out.push_str(&serde_json::to_string(e)?);
        out.push('\n');
    }
    fs::write(path, out).with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_utterance_id_and_transcript_path() -> anyhow::Result<()> {
        let wav = PathBuf::from("/tmp/1272/128104/1272-128104-0009.wav");
        let id = utterance_id_from_wav_path(&wav)?;
        assert_eq!(id, "1272-128104-0009");
        let trans = transcript_path_for_utterance(&wav, &id)?;
        assert_eq!(trans, PathBuf::from("/tmp/1272/128104/1272-128104.trans.txt"));
        Ok(())
    }

    #[test]
    fn test_parse_transcript_file() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let p = dir.path().join("1272-128104.trans.txt");
        fs::write(
            &p,
            "1272-128104-0000 HELLO WORLD\n1272-128104-0001 IT'S ME\n",
        )?;
        let m = parse_transcript_file(&p)?;
        assert_eq!(m["1272-128104-0000"], "HELLO WORLD");
        assert_eq!(m["1272-128104-0001"], "IT'S ME");
        Ok(())
    }
}


