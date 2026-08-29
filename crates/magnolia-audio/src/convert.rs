use thiserror::Error;

pub fn i16_le_to_f32(input: &[u8], output: &mut [f32]) -> Result<usize, ProcessError> {
    if !input.len().is_multiple_of(2) {
        return Err(ProcessError::MisalignedInput);
    }
    let samples = input.len() / 2;
    if output.len() < samples {
        return Err(ProcessError::OutputTooSmall);
    }
    for (bytes, sample) in input.chunks_exact(2).zip(output.iter_mut()) {
        *sample = f32::from(i16::from_le_bytes([bytes[0], bytes[1]])) / 32_768.0;
    }
    Ok(samples)
}

pub fn i32_le_to_f32(input: &[u8], output: &mut [f32]) -> Result<usize, ProcessError> {
    convert_four_byte_samples(input, output, |bytes| {
        i32::from_le_bytes(bytes) as f32 / 2_147_483_648.0
    })
}

pub fn f32_le_to_f32(input: &[u8], output: &mut [f32]) -> Result<usize, ProcessError> {
    convert_four_byte_samples(input, output, f32::from_le_bytes)
}

fn convert_four_byte_samples(
    input: &[u8],
    output: &mut [f32],
    convert: impl Fn([u8; 4]) -> f32,
) -> Result<usize, ProcessError> {
    if !input.len().is_multiple_of(4) {
        return Err(ProcessError::MisalignedInput);
    }
    let samples = input.len() / 4;
    if output.len() < samples {
        return Err(ProcessError::OutputTooSmall);
    }
    for (bytes, sample) in input.chunks_exact(4).zip(output.iter_mut()) {
        *sample = convert([bytes[0], bytes[1], bytes[2], bytes[3]]);
    }
    Ok(samples)
}

pub fn downmix_to_mono(
    input: &[f32],
    channels: usize,
    output: &mut [f32],
) -> Result<usize, ProcessError> {
    if channels == 0 || !input.len().is_multiple_of(channels) {
        return Err(ProcessError::MisalignedInput);
    }
    let frames = input.len() / channels;
    if output.len() < frames {
        return Err(ProcessError::OutputTooSmall);
    }
    for (frame, mono) in input.chunks_exact(channels).zip(output.iter_mut()) {
        *mono = frame.iter().copied().sum::<f32>() / channels as f32;
    }
    Ok(frames)
}

#[derive(Debug)]
pub struct LinearResampler {
    step: f64,
    position: f64,
}

/// Streaming stereo linear resampler. Source positions are continuous across
/// callback buffers and all output storage is supplied by the caller.
#[derive(Debug)]
pub struct StereoLinearResampler {
    step: f64,
    next_source_position: f64,
    input_frames_seen: u64,
    previous: [f32; 2],
    has_previous: bool,
}

impl StereoLinearResampler {
    pub fn new(input_rate: u32, output_rate: u32) -> Result<Self, ProcessError> {
        if input_rate == 0 || output_rate == 0 {
            return Err(ProcessError::ZeroRate);
        }
        Ok(Self {
            step: f64::from(input_rate) / f64::from(output_rate),
            next_source_position: 0.0,
            input_frames_seen: 0,
            previous: [0.0; 2],
            has_previous: false,
        })
    }

    pub fn process(&mut self, input: &[f32], output: &mut [f32]) -> Result<usize, ProcessError> {
        if !input.len().is_multiple_of(2) || !output.len().is_multiple_of(2) {
            return Err(ProcessError::MisalignedInput);
        }
        let frames = input.len() / 2;
        if frames == 0 {
            return Ok(0);
        }
        let start = self.input_frames_seen;
        let end = start.saturating_add(frames as u64 - 1);
        let mut written_frames = 0;
        while written_frames < output.len() / 2 {
            let left_position = self.next_source_position.floor() as u64;
            let right_position = self.next_source_position.ceil() as u64;
            if right_position > end || left_position.saturating_add(1) < start {
                break;
            }
            let left = if left_position < start {
                if !self.has_previous {
                    break;
                }
                self.previous
            } else {
                let index = (left_position - start) as usize * 2;
                [input[index], input[index + 1]]
            };
            let right = if right_position < start {
                self.previous
            } else {
                let index = (right_position - start) as usize * 2;
                [input[index], input[index + 1]]
            };
            let fraction = (self.next_source_position - left_position as f64) as f32;
            let output_index = written_frames * 2;
            output[output_index] = left[0] + (right[0] - left[0]) * fraction;
            output[output_index + 1] = left[1] + (right[1] - left[1]) * fraction;
            written_frames += 1;
            self.next_source_position += self.step;
        }
        self.previous = [input[input.len() - 2], input[input.len() - 1]];
        self.has_previous = true;
        self.input_frames_seen = self.input_frames_seen.saturating_add(frames as u64);
        Ok(written_frames)
    }
}

impl LinearResampler {
    pub fn new(input_rate: u32, output_rate: u32) -> Result<Self, ProcessError> {
        if input_rate == 0 || output_rate == 0 {
            return Err(ProcessError::ZeroRate);
        }
        Ok(Self {
            step: f64::from(input_rate) / f64::from(output_rate),
            position: 0.0,
        })
    }

    pub fn process(&mut self, input: &[f32], output: &mut [f32]) -> usize {
        if input.len() < 2 {
            return 0;
        }
        let mut written = 0;
        while written < output.len() && self.position + 1.0 < input.len() as f64 {
            let left = self.position.floor() as usize;
            let fraction = (self.position - left as f64) as f32;
            output[written] = input[left] + (input[left + 1] - input[left]) * fraction;
            written += 1;
            self.position += self.step;
        }
        self.position = (self.position - input.len() as f64).max(0.0);
        written
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ProcessError {
    #[error("audio input does not contain complete samples or frames")]
    MisalignedInput,
    #[error("prepared output buffer is too small")]
    OutputTooSmall,
    #[error("sample rates must be non-zero")]
    ZeroRate,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions_are_explicit_and_use_caller_storage() {
        let mut stereo = [0.0; 4];
        i16_le_to_f32(&[0, 0, 0xff, 0x7f, 0, 0x80, 0, 0], &mut stereo).unwrap();
        let mut mono = [0.0; 2];
        downmix_to_mono(&stereo, 2, &mut mono).unwrap();
        assert!(mono[0] > 0.49);
        assert!(mono[1] < -0.49);
    }

    #[test]
    fn supported_pipewire_formats_convert_without_allocating_output() {
        let mut output = [0.0; 2];
        assert_eq!(
            f32_le_to_f32(&[0, 0, 0, 63, 0, 0, 128, 191], &mut output).unwrap(),
            2
        );
        assert_eq!(output, [0.5, -1.0]);
        assert_eq!(
            i32_le_to_f32(&[0, 0, 0, 64, 0, 0, 0, 192], &mut output).unwrap(),
            2
        );
        assert_eq!(output, [0.5, -0.5]);
    }

    #[test]
    fn resampler_writes_only_to_preallocated_output() {
        let mut resampler = LinearResampler::new(48_000, 16_000).unwrap();
        let input = [0.0, 0.25, 0.5, 0.75, 1.0, 0.75, 0.5, 0.25];
        let mut output = [0.0; 4];
        let written = resampler.process(&input, &mut output);
        assert_eq!(written, 3);
        assert_eq!(&output[..written], &[0.0, 0.75, 0.5]);
    }

    #[test]
    fn stereo_resampler_carries_fractional_position_across_buffers() {
        let mut resampler = StereoLinearResampler::new(24_000, 48_000).unwrap();
        let mut output = [0.0; 16];
        let first = resampler
            .process(&[0.0, 1.0, 1.0, 2.0], &mut output)
            .unwrap();
        let second = resampler
            .process(&[2.0, 3.0, 3.0, 4.0], &mut output[first * 2..])
            .unwrap();
        assert_eq!(first + second, 7);
        assert_eq!(&output[..8], &[0.0, 1.0, 0.5, 1.5, 1.0, 2.0, 1.5, 2.5]);
    }
}
