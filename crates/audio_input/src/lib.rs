use talisman_plugin_helper::{
    export_plugin, TalismanPlugin, SignalBuffer, SignalType, SignalValue,
};
use std::sync::{Arc, Mutex};
use log::{error, info};
use tokio::sync::mpsc;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SizedSample};

// Define the plugin struct
#[derive(Default)]
struct AudioInputPlugin {
    source: Option<AudioInputSource>,
    enabled: bool,
}

struct SendStream(cpal::Stream);
unsafe impl Send for SendStream {}
unsafe impl Sync for SendStream {}

pub struct AudioInputSource {
    receiver: mpsc::Receiver<AudioPacket>,
    _stream: SendStream,
    sample_rate: u32,
    channels: u16,
}

struct AudioPacket {
    data: Vec<f32>,
}

impl AudioInputPlugin {
    fn init_source(&mut self) {
        if self.source.is_none() {
             match AudioInputSource::new(4096) {
                Ok(src) => {
                    info!("AudioInputPlugin initialized cpal source");
                    self.source = Some(src);
                }
                Err(e) => {
                    error!("AudioInputPlugin failed to init cpal: {}", e);
                }
            }
        }
    }
}

// Implement the trait
impl TalismanPlugin for AudioInputPlugin {
    fn name() -> &'static str { "audio_input" }
    fn version() -> &'static str { "0.1.0" }
    fn description() -> &'static str { "Captures audio from default input device" }
    fn author() -> &'static str { "Talisman" }

    fn c_id(&self) -> *const std::os::raw::c_char {
        // Static C string for ID
        b"audio_input\0".as_ptr() as *const _
    }
    fn c_name(&self) -> *const std::os::raw::c_char {
        b"Audio Input\0".as_ptr() as *const _
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if enabled {
            self.init_source();
            if let Some(src) = &self.source {
                src._stream.0.play().ok();
            }
        } else {
            if let Some(src) = &self.source {
                src._stream.0.pause().ok();
            }
        }
    }

    fn poll_signal(&mut self, buffer: &mut SignalBuffer) -> bool {
        if !self.enabled {
            return false;
        }
        self.init_source(); // fast check

        if let Some(src) = &mut self.source {
            match src.receiver.try_recv() {
                Ok(packet) => {
                    // Populate buffer with Audio Signal
                    // Must ensure capacity == length for cleanup safety
                    let mut data = packet.data;
                    data.shrink_to_fit();
                    if data.capacity() != data.len() {
                        data = data.into_boxed_slice().into_vec();
                    }
                    
                    let ptr = data.as_mut_ptr();
                    let len = data.len();
                    std::mem::forget(data); // Leak to host
                    
                    let param = ((src.sample_rate as u64) << 32) | (src.channels as u64);
                    
                    buffer.signal_type = SignalType::Audio as u32;
                    buffer.value = SignalValue { ptr: ptr as *mut _ };
                    buffer.size = len as u64;
                    buffer.param = param;
                    
                    return true;
                }
                Err(_) => return false,
            }
        }
        false
    }
    
    fn consume_signal(&mut self, _input: &SignalBuffer) -> Option<SignalBuffer> {
        // Audio Input doesn't consume signals yet (maybe config later)
        None
    }
}

// Export the symbols
export_plugin!(AudioInputPlugin);


// ==================== Audio Source Implementation ====================

impl AudioInputSource {
    pub fn new(_buffer_size_frames: usize) -> anyhow::Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| anyhow::anyhow!("No input device available"))?;
            
        let config = device.default_input_config()?;
        let sample_rate = config.sample_rate().0;
        let channels = config.channels();
        
        let (tx, rx) = mpsc::channel(100);
        let tx = Arc::new(Mutex::new(tx));
        
        // We use a simple callback that sends chunks
        let err_fn = |err| error!("cpal stream error: {}", err);
        
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => Self::run::<f32>(&device, &config.into(), tx.clone(), err_fn)?,
            cpal::SampleFormat::I16 => Self::run::<i16>(&device, &config.into(), tx.clone(), err_fn)?,
            cpal::SampleFormat::U16 => Self::run::<u16>(&device, &config.into(), tx.clone(), err_fn)?,
            _ => return Err(anyhow::anyhow!("Unsupported sample format")),
        };
        
        stream.play()?;
        
        Ok(Self {
            receiver: rx,
            _stream: SendStream(stream),
            sample_rate,
            channels,
        })
    }
    
    fn run<T>(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        tx: Arc<Mutex<mpsc::Sender<AudioPacket>>>,
        err_fn: impl Fn(cpal::StreamError) + Send + 'static,
    ) -> anyhow::Result<cpal::Stream>
    where
        T: Sample + SizedSample + Send + 'static + num_traits::ToPrimitive,
    {
        let stream = device.build_input_stream(
            config,
            move |data: &[T], _| {
                let floats: Vec<f32> = data.iter().map(|s| s.to_f32().unwrap_or(0.0)).collect();
                if let Ok(sender) = tx.lock() {
                     let _ = sender.try_send(AudioPacket { data: floats });
                }
            },
            err_fn,
            None
        )?;
        Ok(stream)
    }
}
