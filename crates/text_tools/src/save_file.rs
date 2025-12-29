use async_trait::async_trait;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use talisman_core::{DataType, ModuleSchema, Port, PortDirection, Result, Signal, Sink};

/// Output format for SaveFileSink
#[derive(Clone, Debug, Default)]
pub enum OutputFormat {
    #[default]
    Text,
    Png,
    Bmp,
    Wav,
}

/// A sink that saves incoming signals to files.
/// - Text signals are saved as .txt files
/// - Blob signals (images) are saved as .png or .bmp files
/// - Audio signals are saved as .wav files
pub struct SaveFileSink {
    enabled: bool,
    output_path: Arc<Mutex<PathBuf>>,
    output_format: Arc<Mutex<OutputFormat>>,
    last_saved: Arc<Mutex<Option<String>>>,
    // Persistent writer to avoid re-opening/overwriting WAV headers for every chunk
    audio_writer: Arc<Mutex<Option<hound::WavWriter<std::io::BufWriter<File>>>>>,
}

impl SaveFileSink {
    pub fn new(path: PathBuf) -> Self {
        Self {
            enabled: true,
            output_path: Arc::new(Mutex::new(path)),
            output_format: Arc::new(Mutex::new(OutputFormat::Text)),
            last_saved: Arc::new(Mutex::new(None)),
            audio_writer: Arc::new(Mutex::new(None)),
        }
    }

    /// Set the output file path
    pub fn set_path(&self, path: PathBuf) {
        *self.output_path.lock().unwrap() = path;
    }

    /// Get the current output path
    pub fn get_path(&self) -> PathBuf {
        self.output_path.lock().unwrap().clone()
    }

    /// Set the output format
    pub fn set_format(&self, format: OutputFormat) {
        *self.output_format.lock().unwrap() = format;
    }

    /// Get the current output format
    pub fn get_format(&self) -> OutputFormat {
        self.output_format.lock().unwrap().clone()
    }
}

impl Default for SaveFileSink {
    fn default() -> Self {
        Self::new(PathBuf::from("output.txt"))
    }
}

#[async_trait]
impl Sink for SaveFileSink {
    fn name(&self) -> &str {
        "save_file"
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            id: "save_file".to_string(),
            name: "Save File".to_string(),
            description: "Saves input signals to file (text or image)".to_string(),
            ports: vec![
                Port {
                    id: "text_in".to_string(),
                    label: "Text Input".to_string(),
                    data_type: DataType::Text,
                    direction: PortDirection::Input,
                },
                Port {
                    id: "blob_in".to_string(),
                    label: "Image/Blob Input".to_string(),
                    data_type: DataType::Blob,
                    direction: PortDirection::Input,
                },
                Port {
                    id: "audio_in".to_string(),
                    label: "Audio Input".to_string(),
                    data_type: DataType::Audio,
                    direction: PortDirection::Input,
                },
            ],
            settings_schema: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "output_path": {
                        "type": "string",
                        "title": "Output Path",
                        "default": "output.txt"
                    },
                    "format": {
                        "type": "string",
                        "enum": ["text", "png", "bmp", "wav"],
                        "title": "Output Format",
                        "default": "text"
                    }
                }
            })),
        }
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn render_output(&self) -> Option<String> {
        self.last_saved.lock().unwrap().clone()
    }

    async fn consume(&self, signal: Signal) -> Result<Option<Signal>> {
        if !self.enabled {
            return Ok(None);
        }

        let path = self.output_path.lock().unwrap().clone();
        let format = self.output_format.lock().unwrap().clone();

        match signal {
            Signal::Text(text) => {
                if matches!(format, OutputFormat::Text) {
                    match File::create(&path) {
                        Ok(mut file) => {
                            if let Err(e) = file.write_all(text.as_bytes()) {
                                log::error!("SaveFileSink: Failed to write text: {}", e);
                            } else {
                                let msg = format!("Saved {} bytes to {:?}", text.len(), path);
                                log::info!("SaveFileSink: {}", msg);
                                *self.last_saved.lock().unwrap() = Some(msg);
                            }
                        }
                        Err(e) => {
                            log::error!("SaveFileSink: Failed to create file {:?}: {}", path, e);
                        }
                    }
                }
            }
            Signal::Blob { bytes, mime_type } => match format {
                OutputFormat::Png | OutputFormat::Bmp => match File::create(&path) {
                    Ok(mut file) => {
                        if let Err(e) = file.write_all(&bytes) {
                            log::error!("SaveFileSink: Failed to write blob: {}", e);
                        } else {
                            let msg = format!(
                                "Saved {} bytes ({}) to {:?}",
                                bytes.len(),
                                mime_type,
                                path
                            );
                            log::info!("SaveFileSink: {}", msg);
                            *self.last_saved.lock().unwrap() = Some(msg);
                        }
                    }
                    Err(e) => {
                        log::error!("SaveFileSink: Failed to create file {:?}: {}", path, e);
                    }
                },
                _ => {
                    log::warn!(
                        "SaveFileSink: Received Blob but format is {:?}, ignoring",
                        format
                    );
                }
            },

            Signal::Audio {
                sample_rate,
                channels,
                timestamp_us: _,
                data,
            } => {
                let mut guard = self.audio_writer.lock().unwrap();

                // Initialize writer if None or if we should check path changes (simplified here)
                // Realistically we should check if path changed, but for now assuming session consistency
                if guard.is_none() {
                    let spec = hound::WavSpec {
                        channels: channels,
                        sample_rate: sample_rate,
                        bits_per_sample: 32,
                        sample_format: hound::SampleFormat::Float,
                    };

                    match File::create(&path) {
                        Ok(file) => {
                            let buf_writer = std::io::BufWriter::new(file);
                            match hound::WavWriter::new(buf_writer, spec) {
                                Ok(writer) => {
                                    *guard = Some(writer);
                                    log::info!("SaveFileSink: Started WAV recording to {:?}", path);
                                }
                                Err(e) => {
                                    log::error!("SaveFileSink: Failed to create WavWriter: {}", e)
                                }
                            }
                        }
                        Err(e) => log::error!("SaveFileSink: Failed to create WAV file: {}", e),
                    }
                }

                if let Some(writer) = guard.as_mut() {
                    for sample in data {
                        if let Err(e) = writer.write_sample(sample) {
                            log::error!("SaveFileSink: Error writing sample: {}", e);
                            break;
                        }
                    }
                    // Try to flush frequently so data is safe?
                    // WavWriter doesn't have explicit flush that updates header length easily without finalize.
                    // But we depend on Drop to finalize or manual finalize.
                    // For continuous streaming, we just keep writing.
                }
            }
            _ => {
                // Ignore other signal types
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;

    #[tokio::test]
    async fn test_save_text_file() {
        let path = temp_dir().join("test_save_file.txt");
        let sink = SaveFileSink::new(path.clone());

        sink.consume(Signal::Text("Hello, World!".to_string()))
            .await
            .unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "Hello, World!");

        // Cleanup
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn test_schema() {
        let sink = SaveFileSink::default();
        let schema = sink.schema();

        assert_eq!(schema.id, "save_file");
        assert_eq!(schema.ports.len(), 3); // text, blob, audio inputs
    }
}
