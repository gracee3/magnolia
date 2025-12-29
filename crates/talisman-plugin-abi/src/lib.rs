use std::os::raw::{c_char, c_void};

/// Current ABI version - increment when making breaking changes
pub const ABI_VERSION: u32 = 4;

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

/// Data types for ports (matches core DataType enum)
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataTypeAbi {
    Text = 0,
    Audio = 1,
    Blob = 2,
    Numeric = 3,
    Astrology = 4,
    Control = 5,
    Any = 255,
}

/// Port direction
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortDirectionAbi {
    Input = 0,
    Output = 1,
}

/// Port schema for FFI - describes a single port
#[repr(C)]
#[derive(Debug)]
pub struct PortSchemaAbi {
    pub id: *const c_char,
    pub label: *const c_char,
    pub data_type: DataTypeAbi,
    pub direction: PortDirectionAbi,
}

/// Module schema for FFI - describes the module's ports
#[repr(C)]
#[derive(Debug)]
pub struct ModuleSchemaAbi {
    pub id: *const c_char,
    pub name: *const c_char,
    pub description: *const c_char,
    pub ports: *const PortSchemaAbi,
    pub ports_len: usize,
    /// Optional JSON Schema for settings (null if none)
    pub settings_schema: *const c_char,
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
    /// Returns: 0 = no output, pointer = output signal buffer (caller must free)
    pub consume_signal: unsafe extern "C" fn(*mut c_void, *const SignalBuffer) -> *mut SignalBuffer,
    
    /// Apply settings as JSON string
    pub apply_settings: unsafe extern "C" fn(*mut c_void, *const c_char),
    
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
    GpuContext = 8, 
    Texture = 9,
}

/// Value/Handle union for Signal Buffer (ABI v3)
#[repr(C)]
#[derive(Clone, Copy)]
pub union SignalValue {
    /// Legacy/Direct pointer to data
    pub ptr: *mut c_void,
    /// Shared Memory File Descriptor (POSIX shm)
    pub shm_fd: i64,
    /// GPU Texture ID / Handle (Backend agnostic ID)
    pub gpu_id: u64,
    /// Direct Integer Value
    pub integer: i64,
    /// Direct Float Value
    pub float_val: f64,
}

/// Opaque signal buffer for passing data across FFI boundary
#[repr(C)]
pub struct SignalBuffer {
    pub signal_type: u32,
    pub value: SignalValue,
    /// Size of data (bytes or count depending on type)
    pub size: u64,
    /// Extra parameter for metadata (e.g. sample rate, dimensions)
    pub param: u64,
}

impl SignalBuffer {
    pub fn empty() -> Self {
        Self {
            signal_type: SignalType::Pulse as u32,
            value: SignalValue { ptr: std::ptr::null_mut() },
            size: 0,
            param: 0,
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

/// Get the module schema (optional, for port discovery)
/// Returns null if not supported
pub type PluginGetSchemaFn = unsafe extern "C" fn() -> *const ModuleSchemaAbi;

/// Symbol names that plugins must export
pub const PLUGIN_MANIFEST_SYMBOL: &[u8] = b"talisman_plugin_manifest\0";
pub const PLUGIN_CREATE_SYMBOL: &[u8] = b"talisman_plugin_create\0";
pub const PLUGIN_VTABLE_SYMBOL: &[u8] = b"talisman_plugin_get_vtable\0";
/// Optional schema export symbol
pub const PLUGIN_SCHEMA_SYMBOL: &[u8] = b"talisman_plugin_get_schema\0";

