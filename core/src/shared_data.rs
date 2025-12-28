

/// Audio data wrapper for shared (Arc) signals
#[derive(Debug, Clone)]
pub struct AudioData {
    pub sample_rate: u32,
    pub channels: u16,
    pub data: Vec<f32>,
}

impl AudioData {
    pub fn new(sample_rate: u32, channels: u16, data: Vec<f32>) -> Self {
        Self {
            sample_rate,
            channels,
            data,
        }
    }
}

/// Blob data wrapper for shared (Arc) signals
#[derive(Debug, Clone)]
pub struct BlobData {
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

impl BlobData {
    pub fn new(mime_type: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            mime_type: mime_type.into(),
            bytes,
        }
    }
}
