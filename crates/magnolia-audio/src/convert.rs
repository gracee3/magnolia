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
    fn resampler_writes_only_to_preallocated_output() {
        let mut resampler = LinearResampler::new(48_000, 16_000).unwrap();
        let input = [0.0, 0.25, 0.5, 0.75, 1.0, 0.75, 0.5, 0.25];
        let mut output = [0.0; 4];
        let written = resampler.process(&input, &mut output);
        assert_eq!(written, 3);
        assert_eq!(&output[..written], &[0.0, 0.75, 0.5]);
    }
}
