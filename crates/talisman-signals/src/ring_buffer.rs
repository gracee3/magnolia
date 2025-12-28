use std::sync::atomic::{AtomicUsize, Ordering};
use std::cell::UnsafeCell;
use std::sync::Arc;

/// Single-Producer Single-Consumer lock-free ring buffer
/// 
/// Designed for real-time audio/video streams where latency is critical.
/// Uses atomic operations for synchronization without locks.
/// 
/// # Safety
/// 
/// This is safe as long as:
/// - Only ONE thread writes (producer)
/// - Only ONE thread reads (consumer)
/// - Types are Copy or properly handled
/// 
/// # Performance
/// 
/// - Push: ~5-10ns (atomic store + write)
/// - Pop: ~5-10ns (atomic load + read)
/// - vs mpsc channel: ~100-500ns per send/recv
/// 
/// For 48kHz audio, this means <1μs latency vs potentially 10-50μs with channels.
pub struct SPSCRingBuffer<T> {
    buffer: Box<[UnsafeCell<T>]>,
    write_pos: AtomicUsize,
    read_pos: AtomicUsize,
    capacity: usize,
}

// Safety: Only accessed from single producer/consumer threads
unsafe impl<T: Send> Send for SPSCRingBuffer<T> {}
unsafe impl<T: Send> Sync for SPSCRingBuffer<T> {}

impl<T: Copy + Default> SPSCRingBuffer<T> {
    /// Create a new ring buffer with the given capacity
    /// 
    /// Capacity must be a power of 2 for efficient modulo operations
    pub fn new(capacity: usize) -> Self {
        assert!(capacity.is_power_of_two(), "Capacity must be power of 2");
        assert!(capacity > 1, "Capacity must be > 1");
        
        let buffer = (0..capacity)
            .map(|_| UnsafeCell::new(T::default()))
            .collect::<Vec<_>>()
            .into_boxed_slice();
            
        Self {
            buffer,
            write_pos: AtomicUsize::new(0),
            read_pos: AtomicUsize::new(0),
            capacity,
        }
    }
    
    /// Try to push an item (producer side)
    /// 
    /// Returns `Err(item)` if buffer is full (consumer hasn't kept up)
    /// 
    /// # Performance
    /// 
    /// This is a non-blocking operation that completes in ~5-10ns
    #[inline]
    pub fn try_push(&self, item: T) -> Result<(), T> {
        let write = self.write_pos.load(Ordering::Acquire);
        let read = self.read_pos.load(Ordering::Acquire);
        
        // Use bitwise AND for fast modulo (works because capacity is power of 2)
        let next_write = (write + 1) & (self.capacity - 1);
        
        // Buffer full if next write position would equal read position
        if next_write == read {
            return Err(item);
        }
        
        // Safe because only producer writes to write_pos
        unsafe {
            *self.buffer[write].get() = item;
        }
        
        // Make write visible to consumer
        self.write_pos.store(next_write, Ordering::Release);
        Ok(())
    }
    
    /// Try to pop an item (consumer side)
    /// 
    /// Returns `None` if buffer is empty (producer hasn't produced yet)
    /// 
    /// # Performance
    /// 
    /// This is a non-blocking operation that completes in ~5-10ns
    #[inline]
    pub fn try_pop(&self) -> Option<T> {
        let read = self.read_pos.load(Ordering::Acquire);
        let write = self.write_pos.load(Ordering::Acquire);
        
        // Buffer empty if read equals write
        if read == write {
            return None;
        }
        
        // Safe because only consumer reads from read_pos
        let item = unsafe { *self.buffer[read].get() };
        
        let next_read = (read + 1) & (self.capacity - 1);
        
        // Make read visible to producer
        self.read_pos.store(next_read, Ordering::Release);
        
        Some(item)
    }
    
    /// Get current fill level (approximate)
    /// 
    /// Note: This is a snapshot and may be stale by the time you use it
    pub fn len(&self) -> usize {
        let write = self.write_pos.load(Ordering::Acquire);
        let read = self.read_pos.load(Ordering::Acquire);
        
        if write >= read {
            write - read
        } else {
            self.capacity - read + write
        }
    }
    
    /// Check if buffer is empty (approximate)
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    
    /// Check if buffer is full (approximate)
    pub fn is_full(&self) -> bool {
        self.len() >= self.capacity - 1
    }
    
    ///Get capacity
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

impl<T: Copy + Default> std::fmt::Debug for SPSCRingBuffer<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SPSCRingBuffer")
            .field("capacity", &self.capacity)
            .field("write_pos", &self.write_pos.load(Ordering::Relaxed))
            .field("read_pos", &self.read_pos.load(Ordering::Relaxed))
            .field("len", &self.len())
            .finish()
    }
}

/// Handle to a ring buffer for sending (producer side)
#[derive(Debug)]
pub struct RingBufferSender<T: Copy + Default> {
    inner: Arc<SPSCRingBuffer<T>>,
}

impl<T: Copy + Default> RingBufferSender<T> {
    pub fn try_send(&self, item: T) -> Result<(), T> {
        self.inner.try_push(item)
    }
    
    pub fn len(&self) -> usize {
        self.inner.len()
    }
    
    pub fn is_full(&self) -> bool {
        self.inner.is_full()
    }
}

impl<T: Copy + Default> Clone for RingBufferSender<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

/// Handle to a ring buffer for receiving (consumer side)
#[derive(Debug)]
pub struct RingBufferReceiver<T: Copy + Default> {
    inner: Arc<SPSCRingBuffer<T>>,
}

impl<T: Copy + Default> RingBufferReceiver<T> {
    pub fn try_recv(&self) -> Option<T> {
        self.inner.try_pop()
    }
    
    pub fn len(&self) -> usize {
        self.inner.len()
    }
    
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

impl<T: Copy + Default> Clone for RingBufferReceiver<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

/// Create a new ring buffer channel
/// 
/// Returns (sender, receiver) pair for SPSC communication
/// 
/// # Example
/// 
/// ```
/// use talisman_core::ring_buffer;
/// 
/// let (tx, rx) = ring_buffer::channel::<f32>(1024);
/// 
/// // Producer thread
/// tx.try_send(0.5).unwrap();
/// 
/// // Consumer thread
/// assert_eq!(rx.try_recv(), Some(0.5));
/// ```
pub fn channel<T: Copy + Default>(capacity: usize) -> (RingBufferSender<T>, RingBufferReceiver<T>) {
    let buffer = Arc::new(SPSCRingBuffer::new(capacity));
    
    (
        RingBufferSender { inner: buffer.clone() },
        RingBufferReceiver { inner: buffer },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    
    #[test]
    fn test_basic_push_pop() {
        let (tx, rx) = channel::<u32>(4);
        
        assert_eq!(tx.try_send(1), Ok(()));
        assert_eq!(tx.try_send(2), Ok(()));
        assert_eq!(tx.try_send(3), Ok(()));
        
        assert_eq!(rx.try_recv(), Some(1));
        assert_eq!(rx.try_recv(), Some(2));
        assert_eq!(rx.try_recv(), Some(3));
        assert_eq!(rx.try_recv(), None);
    }
    
    #[test]
    fn test_full() {
        let (tx, _rx) = channel::<u32>(4);
        
        assert_eq!(tx.try_send(1), Ok(()));
        assert_eq!(tx.try_send(2), Ok(()));
        assert_eq!(tx.try_send(3), Ok(()));
        
        // Buffer is full (capacity - 1)
        assert!(tx.try_send(4).is_err());
    }
    
    #[test]
    fn test_wrap_around() {
        let (tx, rx) = channel::<u32>(4);
        
        // Fill buffer
        for i in 0..3 {
            assert_eq!(tx.try_send(i), Ok(()));
        }
        
        // Drain buffer
        for i in 0..3 {
            assert_eq!(rx.try_recv(), Some(i));
        }
        
        // Refill (should wrap around)
        for i in 10..13 {
            assert_eq!(tx.try_send(i), Ok(()));
        }
        
        for i in 10..13 {
            assert_eq!(rx.try_recv(), Some(i));
        }
    }
    
    #[test]
    fn test_concurrent() {
        let (tx, rx) = channel::<u32>(1024);
        
        let producer = thread::spawn(move || {
            for i in 0..10000 {
                while tx.try_send(i).is_err() {
                    // Spin until space available
                    thread::yield_now();
                }
            }
        });
        
        let consumer = thread::spawn(move || {
            let mut count = 0;
            let mut received = Vec::new();
            
            while count < 10000 {
                if let Some(val) = rx.try_recv() {
                    received.push(val);
                    count += 1;
                } else {
                    thread::yield_now();
                }
            }
            
            received
        });
        
        producer.join().unwrap();
        let received = consumer.join().unwrap();
        
        // Verify all items received in order
        assert_eq!(received.len(), 10000);
        for (i, val) in received.iter().enumerate() {
            assert_eq!(*val, i as u32);
        }
    }
    
    #[test]
    fn test_f32_audio_samples() {
        let (tx, rx) = channel::<f32>(2048);
        
        // Simulate audio samples
        let samples: Vec<f32> = (0..1000).map(|i| (i as f32) * 0.001).collect();
        
        for sample in &samples {
            tx.try_send(*sample).unwrap();
        }
        
        for expected in samples {
            assert_eq!(rx.try_recv(), Some(expected));
        }
    }
}
