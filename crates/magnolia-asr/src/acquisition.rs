use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactLock {
    pub name: String,
    pub source_url: String,
    pub expected_bytes: u64,
    pub sha256: Option<String>,
    #[serde(default)]
    pub extracted_sha256: BTreeMap<String, String>,
    pub license: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SherpaAcquisitionLock {
    pub schema_major: u16,
    pub adapter_version: String,
    pub model_asset_id: u64,
    pub model: ArtifactLock,
    pub native_library: ArtifactLock,
}

impl SherpaAcquisitionLock {
    /// Fail closed before network or filesystem mutation.
    pub fn validate(&self) -> Result<(), AcquisitionError> {
        if self.schema_major != 1 {
            return Err(AcquisitionError::UnsupportedMajor(self.schema_major));
        }
        if self.adapter_version != crate::SHERPA_ADAPTER_VERSION {
            return Err(AcquisitionError::WrongAdapter(self.adapter_version.clone()));
        }
        if self.model_asset_id != crate::ACCEPTED_MODEL_ASSET_ID
            || self.model.name != crate::ACCEPTED_MODEL_NAME
            || self.model.expected_bytes != crate::ACCEPTED_MODEL_ARCHIVE_BYTES
            || self.model.source_url != crate::ACCEPTED_MODEL_URL
        {
            return Err(AcquisitionError::WrongModel);
        }
        if self.native_library.name != crate::ACCEPTED_NATIVE_NAME
            || self.native_library.expected_bytes != crate::ACCEPTED_NATIVE_ARCHIVE_BYTES
            || self.native_library.source_url != crate::ACCEPTED_NATIVE_URL
        {
            return Err(AcquisitionError::WrongNativeLibrary);
        }
        validate_artifact("model", &self.model, true)?;
        validate_artifact("native_library", &self.native_library, false)?;
        Ok(())
    }
}

fn validate_artifact(
    kind: &'static str,
    artifact: &ArtifactLock,
    require_extracted: bool,
) -> Result<(), AcquisitionError> {
    if !artifact.source_url.starts_with("https://") || artifact.expected_bytes == 0 {
        return Err(AcquisitionError::InvalidArtifact(kind));
    }
    let hash = artifact
        .sha256
        .as_deref()
        .ok_or(AcquisitionError::MissingAuthoritativeHash(kind))?;
    if !valid_sha256(hash) {
        return Err(AcquisitionError::InvalidHash(kind));
    }
    if artifact.license.trim().is_empty() {
        return Err(AcquisitionError::MissingLicense(kind));
    }
    if require_extracted {
        for required in [
            "encoder-epoch-99-avg-1.onnx",
            "decoder-epoch-99-avg-1.onnx",
            "joiner-epoch-99-avg-1.onnx",
            "tokens.txt",
        ] {
            match artifact.extracted_sha256.get(required) {
                Some(hash) if valid_sha256(hash) => {}
                _ => return Err(AcquisitionError::MissingExtractedHash(required)),
            }
        }
    }
    Ok(())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AcquisitionError {
    #[error("unsupported acquisition schema major {0}")]
    UnsupportedMajor(u16),
    #[error("lock targets Sherpa adapter {0}, not the accepted version")]
    WrongAdapter(String),
    #[error("lock does not identify the accepted Zipformer model asset")]
    WrongModel,
    #[error("lock does not identify the accepted Sherpa 1.13.4 Linux CPU library")]
    WrongNativeLibrary,
    #[error("{0} artifact metadata is invalid")]
    InvalidArtifact(&'static str),
    #[error("{0} artifact lacks an authoritative SHA-256")]
    MissingAuthoritativeHash(&'static str),
    #[error("{0} artifact SHA-256 is invalid")]
    InvalidHash(&'static str),
    #[error("{0} artifact has no established license")]
    MissingLicense(&'static str),
    #[error("model lacks an authoritative extracted hash for {0}")]
    MissingExtractedHash(&'static str),
    #[error("acquisition lock could not be read: {0}")]
    Read(PathBuf),
}
