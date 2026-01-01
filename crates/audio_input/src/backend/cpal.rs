#![cfg(not(target_os = "linux"))]

use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use log::error;

use magnolia_signals::ring_buffer::RingBufferSender;

use super::{AudioInputBackend, BackendStream, DeviceInfo, NegotiatedFormat};

struct SendStream {
    _stream: cpal::Stream,
}
unsafe impl Send for SendStream {}
unsafe impl Sync for SendStream {}

fn now_micros() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

pub struct CpalInputBackend;

impl CpalInputBackend {
    pub fn new() -> Self {
        Self
    }
}

impl AudioInputBackend for CpalInputBackend {
    fn refresh_devices(&mut self) -> anyhow::Result<Vec<DeviceInfo>> {
        let host = cpal::default_host();
        let devices = host
            .input_devices()
            .map(|devices| {
                devices
                    .filter_map(|d| d.name().ok())
                    .map(|name| DeviceInfo {
                        id: name.clone(),
                        name,
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(devices)
    }

    fn start(
        &mut self,
        device_id: &str,
        tx: RingBufferSender<f32>,
        capture_us: Arc<AtomicU64>,
    ) -> anyhow::Result<(BackendStream, NegotiatedFormat, String)> {
        let host = cpal::default_host();

        let resolved_device = if device_id == "Default" {
            host.default_input_device()
        } else {
            host.input_devices().ok().and_then(|mut devices| {
                devices.find(|d| d.name().ok().as_deref() == Some(device_id))
            })
        }
        .ok_or_else(|| anyhow::anyhow!("No input device"))?;

        let resolved_name = resolved_device
            .name()
            .unwrap_or_else(|_| "Unknown".to_string());

        let config = resolved_device.default_input_config()?;
        let sample_rate = config.sample_rate().0;
        let channels = config.channels();

        let err_fn = |err| error!("cpal input error: {}", err);
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => resolved_device.build_input_stream(
                &config.into(),
                move |data: &[f32], _| {
                    capture_us.store(now_micros(), std::sync::atomic::Ordering::Relaxed);
                    for &sample in data {
                        let _ = tx.try_send(sample);
                    }
                },
                err_fn,
                None,
            )?,
            _ => return Err(anyhow::anyhow!("Only F32 supported for now")),
        };

        stream.play()?;

        Ok((
            BackendStream::new(SendStream { _stream: stream }),
            NegotiatedFormat {
                sample_rate,
                channels,
            },
            resolved_name,
        ))
    }
}
