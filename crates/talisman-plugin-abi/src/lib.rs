use std::os::raw::{c_char, c_void};

/// Current ABI version - increment when making breaking changes
pub const ABI_VERSION: u32 = 1;

/// Plugin manifest - describes the plugin's capabilities
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PluginManifest {
    pub abi_version: u32,
    pub name: *const c_char,
    pub version: *const c_char,
    pub description: *const c_char,
    pub author: *const c_char,
}

/// VTable for module runtime callbacks
/// This is the stable C ABI interface that plugins must implement
#[repr(C)]
pub struct ModuleRuntimeVTable {
    /// Get module ID (must be stable across runs)
    pub get_id: unsafe extern "C" fn(*const c_void) -> *const c_char,
    
    /// Get human-readable module name
    pub get_name: unsafe extern "C" fn(*const c_void) -> *const c_char,
    
    /// Check if module is enabled
    pub is_enabled: unsafe extern "C" fn(*const c_void) -> bool,
    
    /// Enable or disable module
    pub set_enabled: unsafe extern "C" fn(*mut c_void, bool),
    
    /// Poll for outgoing signal (source behavior)
    /// Returns true if signal was written to buffer, false if no signal available
    pub poll_signal: unsafe extern "C" fn(*mut c_void, *mut SignalBuffer) -> bool,
    
    /// Consume incoming signal (sink behavior)
    pub consume_signal: unsafe extern "C" fn(*mut c_void, *const SignalBuffer),
    
    /// Destroy the module instance
    pub destroy: unsafe extern "C" fn(*mut c_void),
}

/// Signal types (matches core Signal enum)
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalType {
    Text = 0,
    Intent = 1,
    Astrology = 2,
    Blob = 3,
    Audio = 4,
    Control = 5,
    Computed = 6,
    Pulse = 7,
}

/// Opaque signal buffer for passing data across FFI boundary
#[repr(C)]
pub struct SignalBuffer {
    pub signal_type: u32,
    pub data: *mut c_void,
    pub data_len: usize,
}

impl SignalBuffer {
    pub fn empty() -> Self {
        Self {
            signal_type: SignalType::Pulse as u32,
            data: std::ptr::null_mut(),
            data_len: 0,
        }
    }
}

/// Plugin entry points - these must be exported by the plugin .so/.dll

/// Get plugin manifest
pub type PluginManifestFn = unsafe extern "C" fn() -> PluginManifest;

/// Create a new plugin instance
pub type PluginCreateFn = unsafe extern "C" fn() -> *mut c_void;

/// Get the vtable for the plugin
pub type PluginGetVTableFn = unsafe extern "C" fn() -> *const ModuleRuntimeVTable;

/// Symbol names that plugins must export
pub const PLUGIN_MANIFEST_SYMBOL: &[u8] = b"talisman_plugin_manifest\0";
pub const PLUGIN_CREATE_SYMBOL: &[u8] = b"talisman_plugin_create\0";
pub const PLUGIN_VTABLE_SYMBOL: &[u8] = b"talisman_plugin_get_vtable\0";
