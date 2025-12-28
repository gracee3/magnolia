use async_trait::async_trait;
use std::ffi::CStr;
use tokio::sync::mpsc;
use talisman_plugin_abi::*;
use crate::{Signal, ModuleRuntime, ModuleSchema, PluginLibrary};

pub struct PluginModuleAdapter {
    plugin: PluginLibrary,
    id_cache: String,
    name_cache: String,
}

impl PluginModuleAdapter {
    pub fn new(plugin: PluginLibrary) -> Self {
        let (id_cache, name_cache) = unsafe {
            let id = CStr::from_ptr((plugin.vtable.get_id)(plugin.instance as *const _))
                .to_string_lossy()
                .into_owned();
            let name = CStr::from_ptr((plugin.vtable.get_name)(plugin.instance as *const _))
                .to_string_lossy()
                .into_owned();
            (id, name)
        };
        
        Self { plugin, id_cache, name_cache }
    }
    
    fn encode_signal(&self, signal: &Signal) -> SignalBuffer {
        // Convert Rust Signal to C SignalBuffer
        match signal {
            Signal::Text(text) => {
                let cstring = std::ffi::CString::new(text.as_str()).unwrap_or_default();
                SignalBuffer {
                    signal_type: SignalType::Text as u32,
                    value: SignalValue { ptr: cstring.into_raw() as *mut _ },
                    size: 0, // null-terminated
                    param: 0,
                }
            }
            Signal::Audio { sample_rate, channels, data } => {
                // Transfer ownership of data to the buffer
                // We clone the data here because we can't easily take ownership from &Signal
                // But in a real hot-path, efficient movement is key.
                // For now, adapter clones.
                let mut data_clone = data.clone();
                let ptr = data_clone.as_mut_ptr();
                let len = data_clone.len();
                std::mem::forget(data_clone); // Leak it (ownership passed to buffer)

                // Pack metadata into param: High 32 = Sample Rate, Low 32 = Channels
                let param = ((*sample_rate as u64) << 32) | (*channels as u64);

                SignalBuffer {
                    signal_type: SignalType::Audio as u32,
                    value: SignalValue { ptr: ptr as *mut _ },
                    size: len as u64,
                    param,
                }
            }
            Signal::GpuContext { device, queue } => {
                SignalBuffer {
                    signal_type: SignalType::GpuContext as u32,
                    value: SignalValue { ptr: *device as *mut _ },
                    size: 0,
                    param: *queue as u64,
                }
            }
            Signal::Texture { id, view, width, height } => {
                let param = ((*width as u64) << 32) | (*height as u64);
                // Correct mapping:
                // Value.ptr = View Handle
                // Size = ID
                // Param = W/H
                
                SignalBuffer {
                    signal_type: SignalType::Texture as u32,
                    value: SignalValue { ptr: *view as *mut _ },
                    size: *id,
                    param,
                }
            }
            Signal::Pulse => SignalBuffer::empty(),
            // TODO: extensive signal mapping
            _ => SignalBuffer::empty(), 
        }
    }
    
    unsafe fn decode_signal(&self, buffer: &SignalBuffer) -> Option<Signal> {
        match buffer.signal_type {
            t if t == SignalType::Text as u32 => {
                if buffer.value.ptr.is_null() {
                    return None;
                }
                let cstr = CStr::from_ptr(buffer.value.ptr as *const i8);
                Some(Signal::Text(cstr.to_string_lossy().into_owned()))
            }
            t if t == SignalType::Audio as u32 => {
                if buffer.value.ptr.is_null() {
                    return None;
                }
                // Unpack metadata
                let sample_rate = (buffer.param >> 32) as u32;
                let channels = (buffer.param & 0xFFFFFFFF) as u16;
                let size = buffer.size as usize;
                
                // Reconstruct Vec from raw parts
                // Note: We copy here to be safe and own the data in the Signal
                // effectively treating the input buffer as borrowed.
                // BUT, to avoid double-free or leak, we need to know who owns it.
                // The contract is: Consumer takes ownership if it returns ptr?
                // Or Consumer copies?
                // The current adapter implementation assumes it TAKES ownership for freeing
                // but decoding usually returns a new struct.
                
                let slice = std::slice::from_raw_parts(buffer.value.ptr as *const f32, size);
                let data = slice.to_vec(); // Copy
                
                Some(Signal::Audio {
                    sample_rate,
                    channels,
                    data,
                })
            }
            t if t == SignalType::GpuContext as u32 => {
                 let device = buffer.value.ptr as usize;
                 let queue = buffer.param as usize;
                 Some(Signal::GpuContext { device, queue })
            }
            t if t == SignalType::Texture as u32 => {
                 let view = buffer.value.ptr as usize;
                 let id = buffer.size;
                 let width = (buffer.param >> 32) as u32;
                 let height = (buffer.param & 0xFFFFFFFF) as u32;
                 Some(Signal::Texture { id, view, width, height })
            }
            t if t == SignalType::Pulse as u32 => Some(Signal::Pulse),
            _ => None,
        }
    }
    
    // === HOT-RELOAD LIFECYCLE HOOKS ===
    
    /// Called before the plugin is unloaded during hot-reload.
    /// Allows the plugin to flush pending data, save state, etc.
    pub fn pre_unload(&mut self) {
        log::info!("Plugin {} preparing for hot-reload unload", self.id_cache);
        
        // Disable the plugin to stop it from processing
        self.set_enabled(false);
        
        // Give it a moment to finish any pending work
        // In a real implementation, you might want to:
        // - Flush any pending output signals
        // - Save plugin state to disk
        // - Close file handles or network connections
    }
    
    /// Called after a new plugin instance is loaded during hot-reload.
    /// Can be used to restore state from the previous instance.
    pub fn post_reload(&mut self, _previous_state: Option<Vec<u8>>) {
        log::info!("Plugin {} completed hot-reload", self.id_cache);
        
        // Re-enable the plugin
        self.set_enabled(true);
        
        // In a real implementation, you might:
        // - Restore saved state
        // - Re-establish connections
        // - Notify the plugin of configuration changes
    }
    
    /// Get plugin state for persistence across hot-reload (placeholder)
    pub fn get_state(&self) -> Option<Vec<u8>> {
        // Future: Plugins could implement a get_state callback in the vtable
        // that returns serialized state
        None
    }
}

#[async_trait]
impl ModuleRuntime for PluginModuleAdapter {
    fn id(&self) -> &str {
        &self.id_cache
    }
    
    fn name(&self) -> &str {
        &self.name_cache
    }
    
    fn schema(&self) -> ModuleSchema {
        // Since the current C ABI doesn't support full schema introspection yet
        // we create a basic schema from the cached info
        ModuleSchema {
            id: self.id_cache.clone(),
            name: self.name_cache.clone(),
            description: format!("Plugin: {}", self.name_cache),
            ports: vec![], // TODO: Extend ABI to support port definitions
            settings_schema: None,
        }
    }
    
    fn is_enabled(&self) -> bool {
        unsafe { (self.plugin.vtable.is_enabled)(self.plugin.instance as *const _) }
    }
    
    fn set_enabled(&mut self, enabled: bool) {
        unsafe { (self.plugin.vtable.set_enabled)(self.plugin.instance, enabled) }
    }
    
    async fn run(&mut self, mut inbox: mpsc::Receiver<Signal>, outbox: mpsc::Sender<Signal>) {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(10));
        
        loop {
            interval.tick().await;
            
            // Poll plugin for outgoing signals
            // We must process and free the buffer BEFORE awaiting anything, 
            // because SignalBuffer is !Send (contains raw pointers)
            let maybe_signal = unsafe {
                let mut signal_buf = SignalBuffer::empty();
                let mut result = None;
                
                if (self.plugin.vtable.poll_signal)(self.plugin.instance, &mut signal_buf) {
                    result = self.decode_signal(&signal_buf);
                    
                    // Free the buffer data if allocated by the plugin
                     if !signal_buf.value.ptr.is_null() {
                        if signal_buf.signal_type == SignalType::Text as u32 {
                            let _ = std::ffi::CString::from_raw(signal_buf.value.ptr as *mut i8);
                        } else if signal_buf.signal_type == SignalType::Audio as u32 {
                            let size = signal_buf.size as usize;
                            // Assume capacity == length (plugin must ensure this)
                            let _ = Vec::from_raw_parts(signal_buf.value.ptr as *mut f32, size, size);
                        }
                    }
                }
                result
            };

            if let Some(signal) = maybe_signal {
                let _ = outbox.send(signal).await;
            }
            
            // Send incoming signals to plugin and handle any output
            while let Ok(signal) = inbox.try_recv() {
                let maybe_output = unsafe {
                    let signal_buf = self.encode_signal(&signal);
                    let output_ptr = (self.plugin.vtable.consume_signal)(self.plugin.instance, &signal_buf);
                                        // We allocated signal_buf.data in encode_signal, we must free it
                     if !signal_buf.value.ptr.is_null() {
                        if signal_buf.signal_type == SignalType::Text as u32 {
                             let _ = std::ffi::CString::from_raw(signal_buf.value.ptr as *mut i8);
                        } else if signal_buf.signal_type == SignalType::Audio as u32 {
                             let size = signal_buf.size as usize;
                             std::mem::drop(Vec::from_raw_parts(signal_buf.value.ptr as *mut f32, size, size));
                        }
                     }
                    
                    // Check if plugin returned an output signal
                    if !output_ptr.is_null() {
                        let output_signal = self.decode_signal(&*output_ptr);
                        // Free the output buffer that the plugin allocated
                        if !(*output_ptr).value.ptr.is_null() {
                            if (*output_ptr).signal_type == SignalType::Text as u32 {
                                let _ = std::ffi::CString::from_raw((*output_ptr).value.ptr as *mut i8);
                            } else if (*output_ptr).signal_type == SignalType::Audio as u32 {
                                let size = (*output_ptr).size as usize;
                                let _ = Vec::from_raw_parts((*output_ptr).value.ptr as *mut f32, size, size);
                            }
                        }
                        // Free the SignalBuffer struct itself (plugin allocated it)
                        let _ = Box::from_raw(output_ptr);
                        output_signal
                    } else {
                        None
                    }
                };
                
                // Send any output signal from consume_signal
                if let Some(output) = maybe_output {
                    let _ = outbox.send(output).await;
                }
            }
        }
    }
}
