/// Specialized audio frame for ring buffer transmission
/// 
/// This is optimized to be Copy + Default for use in SPSCRingBuffer.
/// For real-time audio processing with minimal latency.
#[derive(Debug, Clone, Copy, Default)]
pub struct AudioFrame {
    /// Timestamp (microseconds since start)
    pub timestamp_us: u64,
    /// Left channel sample
    pub left: f32,
    /// Right channel sample  
    pub right: f32,
}

impl AudioFrame {
    pub fn new(left: f32, right: f32) -> Self {
        Self {
            timestamp_us: 0,
            left,
            right,
        }
    }
    
    pub fn mono(sample: f32) -> Self {
        Self {
            timestamp_us: 0,
            left: sample,
            right: sample,
        }
    }
    
    pub fn with_timestamp(mut self, timestamp_us: u64) -> Self {
        self.timestamp_us = timestamp_us;
        self
    }
}
