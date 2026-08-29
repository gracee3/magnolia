use rustfft::{num_complex::Complex32, Fft, FftPlanner};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, f32::consts::PI, sync::Arc};
use thiserror::Error;

pub const ANALYZER_SCHEMA_MAJOR: u16 = 1;
pub const SPECTRUM_WINDOW: usize = 2_048;
pub const SPECTRUM_HOP: usize = SPECTRUM_WINDOW / 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalyzerKind {
    Meter,
    Waveform,
    Spectrum,
    Diagnostics,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AnalyzerFrame {
    Meter(MeterFrame),
    Waveform(WaveformFrame),
    Spectrum(SpectrumFrame),
    Diagnostics(DiagnosticsFrame),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameHeader {
    pub schema_major: u16,
    pub schema_minor: u16,
    pub sequence: u64,
    pub source_start: u64,
    pub source_end: u64,
    pub capture_monotonic_ns: u64,
    pub block_complete_monotonic_ns: u64,
    pub graph_monotonic_ns: u64,
    pub analyzer_monotonic_ns: u64,
    pub cumulative_dropped_frames: u64,
    pub discontinuity: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeterFrame {
    pub header: FrameHeader,
    pub rms: Vec<f32>,
    pub peak: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WaveformFrame {
    pub header: FrameHeader,
    pub channels: u16,
    pub envelopes: Vec<MinMax>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MinMax {
    pub minimum: f32,
    pub maximum: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpectrumFrame {
    pub header: FrameHeader,
    pub sample_rate: u32,
    pub bins_db: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiagnosticsFrame {
    pub header: FrameHeader,
    pub queue_depth: u32,
    pub utilization_millionths: u32,
    pub processing_ns: u64,
    pub cumulative_dropped_frames: u64,
    pub cumulative_discontinuities: u64,
    pub latency_ns: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct BlockTiming {
    pub sequence: u64,
    pub source_start: u64,
    pub capture_monotonic_ns: u64,
    pub block_complete_monotonic_ns: u64,
    pub graph_monotonic_ns: u64,
    pub analyzer_monotonic_ns: u64,
    pub cumulative_dropped_frames: u64,
    pub discontinuity: bool,
    pub queue_depth: u32,
    pub utilization_millionths: u32,
    pub processing_ns: u64,
    pub cumulative_discontinuities: u64,
}

pub struct AnalyzerEngine {
    sample_rate: u32,
    channels: usize,
    leased: BTreeSet<AnalyzerKind>,
    hann: Box<[f32]>,
    spectrum_samples: Box<[f32]>,
    spectrum_fill: usize,
    fft_buffer: Box<[Complex32]>,
    fft_scratch: Box<[Complex32]>,
    fft: Arc<dyn Fft<f32>>,
}

impl AnalyzerEngine {
    pub fn new(sample_rate: u32, channels: usize) -> Result<Self, AnalyzerError> {
        if sample_rate == 0 || !(1..=2).contains(&channels) {
            return Err(AnalyzerError::UnsupportedFormat);
        }
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(SPECTRUM_WINDOW);
        let scratch_len = fft.get_inplace_scratch_len();
        Ok(Self {
            sample_rate,
            channels,
            leased: BTreeSet::new(),
            hann: (0..SPECTRUM_WINDOW)
                .map(|index| 0.5 - 0.5 * (2.0 * PI * index as f32 / SPECTRUM_WINDOW as f32).cos())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            spectrum_samples: vec![0.0; SPECTRUM_WINDOW].into_boxed_slice(),
            spectrum_fill: 0,
            fft_buffer: vec![Complex32::new(0.0, 0.0); SPECTRUM_WINDOW].into_boxed_slice(),
            fft_scratch: vec![Complex32::new(0.0, 0.0); scratch_len].into_boxed_slice(),
            fft,
        })
    }

    pub fn set_leased(&mut self, kind: AnalyzerKind, leased: bool) {
        if leased {
            self.leased.insert(kind);
        } else {
            self.leased.remove(&kind);
        }
    }

    #[must_use]
    pub fn is_bypassed(&self) -> bool {
        self.leased.is_empty()
    }

    pub fn process(
        &mut self,
        interleaved: &[f32],
        timing: BlockTiming,
    ) -> Result<Vec<AnalyzerFrame>, AnalyzerError> {
        if !interleaved.len().is_multiple_of(self.channels) {
            return Err(AnalyzerError::MisalignedBlock);
        }
        if self.is_bypassed() {
            return Ok(Vec::new());
        }
        let frames = interleaved.len() / self.channels;
        let header = FrameHeader {
            schema_major: ANALYZER_SCHEMA_MAJOR,
            schema_minor: 0,
            sequence: timing.sequence,
            source_start: timing.source_start,
            source_end: timing.source_start.saturating_add(frames as u64),
            capture_monotonic_ns: timing.capture_monotonic_ns,
            block_complete_monotonic_ns: timing.block_complete_monotonic_ns,
            graph_monotonic_ns: timing.graph_monotonic_ns,
            analyzer_monotonic_ns: timing.analyzer_monotonic_ns,
            cumulative_dropped_frames: timing.cumulative_dropped_frames,
            discontinuity: timing.discontinuity,
        };
        let mut output = Vec::with_capacity(self.leased.len());
        if self.leased.contains(&AnalyzerKind::Meter) {
            let mut sums = vec![0.0_f64; self.channels];
            let mut peak = vec![0.0_f32; self.channels];
            for frame in interleaved.chunks_exact(self.channels) {
                for (channel, sample) in frame.iter().enumerate() {
                    sums[channel] += f64::from(*sample) * f64::from(*sample);
                    peak[channel] = peak[channel].max(sample.abs());
                }
            }
            let rms = sums
                .into_iter()
                .map(|sum| (sum / frames.max(1) as f64).sqrt() as f32)
                .collect();
            output.push(AnalyzerFrame::Meter(MeterFrame {
                header: header.clone(),
                rms,
                peak,
            }));
        }
        if self.leased.contains(&AnalyzerKind::Waveform) {
            let envelopes = interleaved
                .chunks_exact(self.channels)
                .map(|frame| MinMax {
                    minimum: frame.iter().copied().fold(f32::INFINITY, f32::min),
                    maximum: frame.iter().copied().fold(f32::NEG_INFINITY, f32::max),
                })
                .collect();
            output.push(AnalyzerFrame::Waveform(WaveformFrame {
                header: header.clone(),
                channels: self.channels as u16,
                envelopes,
            }));
        }
        if self.leased.contains(&AnalyzerKind::Spectrum) {
            for frame in interleaved.chunks_exact(self.channels) {
                let mono = frame.iter().copied().sum::<f32>() / self.channels as f32;
                if self.spectrum_fill < SPECTRUM_WINDOW {
                    self.spectrum_samples[self.spectrum_fill] = mono;
                    self.spectrum_fill += 1;
                }
                if self.spectrum_fill == SPECTRUM_WINDOW {
                    for index in 0..SPECTRUM_WINDOW {
                        self.fft_buffer[index] =
                            Complex32::new(self.spectrum_samples[index] * self.hann[index], 0.0);
                    }
                    self.fft
                        .process_with_scratch(&mut self.fft_buffer, &mut self.fft_scratch);
                    let bins_db = self.fft_buffer[..=SPECTRUM_WINDOW / 2]
                        .iter()
                        .map(|bin| {
                            (bin.norm() / SPECTRUM_WINDOW as f32).max(1.0e-12).log10() * 20.0
                        })
                        .collect();
                    output.push(AnalyzerFrame::Spectrum(SpectrumFrame {
                        header: header.clone(),
                        sample_rate: self.sample_rate,
                        bins_db,
                    }));
                    self.spectrum_samples
                        .copy_within(SPECTRUM_HOP..SPECTRUM_WINDOW, 0);
                    self.spectrum_fill = SPECTRUM_WINDOW - SPECTRUM_HOP;
                }
            }
        }
        if self.leased.contains(&AnalyzerKind::Diagnostics) {
            output.push(AnalyzerFrame::Diagnostics(DiagnosticsFrame {
                header,
                queue_depth: timing.queue_depth,
                utilization_millionths: timing.utilization_millionths,
                processing_ns: timing.processing_ns,
                cumulative_dropped_frames: timing.cumulative_dropped_frames,
                cumulative_discontinuities: timing.cumulative_discontinuities,
                latency_ns: timing
                    .analyzer_monotonic_ns
                    .saturating_sub(timing.capture_monotonic_ns),
            }));
        }
        Ok(output)
    }
}

#[derive(Debug, Error)]
pub enum AnalyzerError {
    #[error("analyzers support only non-zero-rate mono or stereo PCM")]
    UnsupportedFormat,
    #[error("analyzer input does not contain complete interleaved frames")]
    MisalignedBlock,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timing(sequence: u64) -> BlockTiming {
        BlockTiming {
            sequence,
            source_start: sequence * 256,
            capture_monotonic_ns: 10,
            block_complete_monotonic_ns: 20,
            graph_monotonic_ns: 30,
            analyzer_monotonic_ns: 40,
            cumulative_dropped_frames: 0,
            discontinuity: false,
            queue_depth: 1,
            utilization_millionths: 10_000,
            processing_ns: 100,
            cumulative_discontinuities: 0,
        }
    }

    #[test]
    fn analyzers_run_only_while_leased() {
        let mut engine = AnalyzerEngine::new(48_000, 2).unwrap();
        assert!(engine.process(&[0.5; 512], timing(0)).unwrap().is_empty());
        engine.set_leased(AnalyzerKind::Meter, true);
        let frames = engine.process(&[0.5; 512], timing(1)).unwrap();
        let AnalyzerFrame::Meter(meter) = &frames[0] else {
            panic!("meter")
        };
        assert_eq!(meter.rms, vec![0.5, 0.5]);
        assert_eq!(meter.peak, vec![0.5, 0.5]);
        engine.set_leased(AnalyzerKind::Meter, false);
        assert!(engine.is_bypassed());
    }

    #[test]
    fn spectrum_uses_2048_sample_hann_windows_with_half_overlap() {
        let mut engine = AnalyzerEngine::new(48_000, 1).unwrap();
        engine.set_leased(AnalyzerKind::Spectrum, true);
        let signal: Vec<_> = (0..3_072)
            .map(|index| (2.0 * PI * 1_000.0 * index as f32 / 48_000.0).sin())
            .collect();
        let spectra = engine
            .process(&signal, timing(0))
            .unwrap()
            .into_iter()
            .filter(|frame| matches!(frame, AnalyzerFrame::Spectrum(_)))
            .count();
        assert_eq!(spectra, 2);
    }
}
