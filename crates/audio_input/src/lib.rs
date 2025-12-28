use async_trait::async_trait;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use log::{error, info};
use std::sync::{Arc, Mutex};
use talisman_core::{
    DataType, ModuleSchema, Port, PortDirection, Signal, Source,
};
use tokio::sync::mpsc;

pub mod realtime;
pub use realtime::AudioInputSourceRT;

#[cfg(feature = "tile-rendering")]
pub mod tile;

#[cfg(feature = "tile-rendering")]
pub use tile::AudioVisTile;

struct SendStream(cpal::Stream);
unsafe impl Send for SendStream {}
unsafe impl Sync for SendStream {}

pub struct AudioInputSource {
    receiver: mpsc::Receiver<Signal>,
    _stream: SendStream, // Keep stream alive
    enabled: bool,
}

impl AudioInputSource {
    pub fn new(buffer_size_frames: usize) -> anyhow::Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| anyhow::anyhow!("No input device available"))?;

        info!("Using audio input device: {}", device.name()?);

        let supported_config = device.default_input_config()?;
        let sample_format = supported_config.sample_format();
        let config: cpal::StreamConfig = supported_config.into();
        
        // let config: cpal::StreamConfig = device.default_input_config()?.into();
        let sample_rate = config.sample_rate.0;
        let channels = config.channels;

        let (tx, rx) = mpsc::channel(100);
        let tx = Arc::new(Mutex::new(tx));

        let err_fn = move |err| {
            error!("an error occurred on stream: {}", err);
        };

        let stream = match sample_format {
            cpal::SampleFormat::F32 => {
                Self::run::<f32>(&device, &config.into(), tx, sample_rate, channels, buffer_size_frames, err_fn)?
            },
            cpal::SampleFormat::I16 => {
                Self::run::<i16>(&device, &config.into(), tx, sample_rate, channels, buffer_size_frames, err_fn)?
            },
            cpal::SampleFormat::U16 => {
                Self::run::<u16>(&device, &config.into(), tx, sample_rate, channels, buffer_size_frames, err_fn)?
            },
            _ => return Err(anyhow::anyhow!("Unsupported sample format")),
        };

        stream.play()?;

        Ok(Self {
            receiver: rx,
            _stream: SendStream(stream),
            enabled: true,
        })
    }

    fn run<T>(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        tx: Arc<Mutex<mpsc::Sender<Signal>>>,
        sample_rate: u32,
        channels: u16,
        _chunk_size: usize,
        err_fn: impl Fn(cpal::StreamError) + Send + 'static,
    ) -> anyhow::Result<cpal::Stream>
    where
        T: cpal::Sample + cpal::SizedSample + Send + 'static + num_traits::ToPrimitive,
    {
        // let mut buffer = Vec::with_capacity(chunk_size * channels as usize);

        let stream = device.build_input_stream(
            config,
            move |data: &[T], _: &_| {
                // Determine how many samples to read
                // Simple approach: emit every callback chunk. 
                // Better approach for consistency: buffer until chunk_size.
                
                // For low latency visualization, smaller frequent chunks are okay.
                // We'll just convert and send whatever we get for now to minimize latency.
                
                let floats: Vec<f32> = data.iter().map(|s| s.to_f32().unwrap_or(0.0)).collect();
                
                // Construct Signal
                let signal = Signal::Audio {
                    sample_rate,
                    channels,
                    data: floats,
                };

                // Non-blocking send
                if let Ok(sender) = tx.lock() {
                     let _ = sender.try_send(signal);
                }
            },
            err_fn,
            None,
        )?;

        Ok(stream)
    }
}

#[async_trait]
impl Source for AudioInputSource {
    fn name(&self) -> &str {
        "audio_input"
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            id: "audio_input".to_string(),
            name: "Audio Input".to_string(),
            description: "Captures realtime audio from default microphone".to_string(),
            ports: vec![Port {
                id: "audio_out".to_string(),
                label: "Signal Out".to_string(),
                data_type: DataType::Audio,
                direction: PortDirection::Output,
            }],
            settings_schema: None,
        }
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if enabled {
            self._stream.0.play().ok();
        } else {
            self._stream.0.pause().ok();
        }
    }

    async fn poll(&mut self) -> Option<Signal> {
        self.receiver.recv().await
    }
}
