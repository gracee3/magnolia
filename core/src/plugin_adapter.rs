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
                    data: cstring.into_raw() as *mut _,
                    data_len: 0, // null-terminated
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
                if buffer.data.is_null() {
                    return None;
                }
                let cstr = CStr::from_ptr(buffer.data as *const i8);
                Some(Signal::Text(cstr.to_string_lossy().into_owned()))
            }
            t if t == SignalType::Pulse as u32 => Some(Signal::Pulse),
            _ => None,
        }
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
                     if !signal_buf.data.is_null() && signal_buf.signal_type == SignalType::Text as u32 {
                        let _ = std::ffi::CString::from_raw(signal_buf.data as *mut i8);
                    }
                }
                result
            };

            if let Some(signal) = maybe_signal {
                let _ = outbox.send(signal).await;
            }
            
            // Send incoming signals to plugin
            while let Ok(signal) = inbox.try_recv() {
                unsafe {
                    let signal_buf = self.encode_signal(&signal);
                    (self.plugin.vtable.consume_signal)(self.plugin.instance, &signal_buf);
                    
                    // We allocated signal_buf.data in encode_signal, we must free it
                     if !signal_buf.data.is_null() && signal_buf.signal_type == SignalType::Text as u32 {
                        let _ = std::ffi::CString::from_raw(signal_buf.data as *mut i8);
                    }
                }
            }
        }
    }
}
