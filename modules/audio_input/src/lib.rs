
pub mod tile;

use talisman_module_api::{StaticModule, ControlMsg, ControlCx, TickCx, Manifest, PortDesc};
use talisman_signals::{DataType, PortDirection, Signal, ring_buffer};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SizedSample};
use std::sync::{Arc, Mutex};
use log::{error, info};

struct SendStream(cpal::Stream);
unsafe impl Send for SendStream {}
unsafe impl Sync for SendStream {}

pub struct AudioInputModule {
    enabled: bool,
    stream: Option<SendStream>,
    pending_receiver: Option<talisman_signals::ring_buffer::RingBufferReceiver<f32>>,
    sender: Option<talisman_signals::ring_buffer::RingBufferSender<f32>>,
    sample_rate: u32,
    channels: u16,
    ports: Vec<PortDesc>,
}

impl AudioInputModule {
    pub fn new() -> Self {
        let capacity = 8192; 
        let (tx, rx) = ring_buffer::channel::<f32>(capacity);

        Self {
            enabled: true,
            stream: None,
            pending_receiver: Some(rx),
            sender: Some(tx),
            sample_rate: 44100, // Default until init
            channels: 2,        // Default until init
            ports: vec![
                PortDesc {
                    id: "stream".to_string(),
                    label: "Audio Stream".to_string(),
                    data_type: DataType::Audio,
                    direction: PortDirection::Output,
                }
            ],
        }
    }

    pub fn initialize(&mut self) {
        if self.stream.is_some() {
            return;
        }

        match self.setup_cpal() {
            Ok((stream, sr, ch)) => {
                info!("AudioInput initialized. SR: {}, Ch: {}", sr, ch);
                self.stream = Some(SendStream(stream));
                self.sample_rate = sr;
                self.channels = ch;
            }
            Err(e) => {
                error!("Failed to initialize CPAL: {}", e);
            }
        }
    }

    fn setup_cpal(&mut self) -> anyhow::Result<(cpal::Stream, u32, u16)> {
        let host = cpal::default_host();
        let device = host.default_input_device()
            .ok_or_else(|| anyhow::anyhow!("No input device"))?;
        
        let config = device.default_input_config()?;
        let sample_rate = config.sample_rate().0;
        let channels = config.channels();
        
        // Move sender into callback (SPSC: single producer)
        let tx = self.sender.take().ok_or_else(|| anyhow::anyhow!("No sender available"))?;

        let err_fn = |err| error!("cpal stream error: {}", err);

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => {
                device.build_input_stream(
                    &config.into(),
                    move |data: &[f32], _| {
                        for &sample in data {
                            let _ = tx.try_send(sample);
                        }
                    },
                    err_fn,
                    None
                )?
            }
            _ => return Err(anyhow::anyhow!("Only F32 supported for now")),
        };

        stream.play()?;
        Ok((stream, sample_rate, channels))
    }
}

impl StaticModule for AudioInputModule {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: "audio_input".to_string(),
            name: "Audio Input".to_string(),
            description: "Captures audio from default input device via CPAL (RT)".to_string(),
            version: "0.1.0".to_string(),
            author: "Talisman".to_string(),
            settings_schema: None,
        }
    }

    fn ports(&self) -> &[PortDesc] {
        &self.ports
    }
    
    fn on_control(&mut self, _cx: &mut ControlCx, _msg: ControlMsg) {
        // Handle enable/disable
    }

    fn tick(&mut self, _cx: &mut TickCx) {
        // Initialize on first tick if needed (lazy init fallback)
        if self.stream.is_none() {
            self.initialize();
        }
    }
}

// Helper to get the receiver out (Static module privilege)
impl AudioInputModule {
    pub fn take_receiver(&mut self) -> Option<(talisman_signals::ring_buffer::RingBufferReceiver<f32>, u32, u16)> {
        match self.pending_receiver.take() {
            Some(rx) => Some((rx, self.sample_rate, self.channels)),
            None => None,
        }
    }
}
