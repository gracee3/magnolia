#![cfg(not(target_os = "linux"))]

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use log::error;

use talisman_signals::ring_buffer::RingBufferReceiver;

use super::{AudioOutputBackend, BackendStream, DeviceInfo, NegotiatedFormat};

struct SendStream {
    _stream: cpal::Stream,
}
unsafe impl Send for SendStream {}
unsafe impl Sync for SendStream {}

pub struct CpalOutputBackend;

impl CpalOutputBackend {
    pub fn new() -> Self {
        Self
    }
}

impl AudioOutputBackend for CpalOutputBackend {
    fn refresh_devices(&mut self) -> anyhow::Result<Vec<DeviceInfo>> {
        let host = cpal::default_host();
        let devices = host
            .output_devices()
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
        rx: RingBufferReceiver<f32>,
    ) -> anyhow::Result<(BackendStream, NegotiatedFormat, String)> {
        let host = cpal::default_host();
        let resolved_device = if device_id == "Default" {
            host.default_output_device()
        } else {
            host.output_devices().ok().and_then(|mut devices| {
                devices.find(|d| d.name().ok().as_deref() == Some(device_id))
            })
        }
        .ok_or_else(|| anyhow::anyhow!("No output device"))?;

        let resolved_name = resolved_device
            .name()
            .unwrap_or_else(|_| "Unknown".to_string());

        let config = resolved_device.default_output_config()?;
        let sample_rate = config.sample_rate().0;
        let channels = config.channels();

        let err_fn = |err| error!("cpal output error: {}", err);
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => resolved_device.build_output_stream(
                &config.into(),
                move |data: &mut [f32], _| {
                    for sample in data {
                        *sample = rx.try_recv().unwrap_or(0.0);
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
