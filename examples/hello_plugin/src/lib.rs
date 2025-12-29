use std::ffi::CString;
use std::os::raw::c_void;
use talisman_plugin_abi::*;

/// Example plugin state
struct HelloPlugin {
    enabled: bool,
    id: CString,
    name: CString,
    counter: u32,
}

impl HelloPlugin {
    fn new() -> Self {
        Self {
            enabled: true,
            id: CString::new("hello_plugin").unwrap(),
            name: CString::new("Hello Plugin").unwrap(),
            counter: 0,
        }
    }
}

// Plugin manifest
#[no_mangle]
pub unsafe extern "C" fn talisman_plugin_manifest() -> PluginManifest {
    PluginManifest {
        abi_version: 4,
        name: "Hello Plugin\0".as_ptr() as *const i8,
        version: "0.1.0\0".as_ptr() as *const i8,
        description: "Example plugin that demonstrates the plugin ABI\0".as_ptr() as *const i8,
        author: "Talisman Team\0".as_ptr() as *const i8,
    }
}

// Create plugin instance
#[no_mangle]
pub unsafe extern "C" fn talisman_plugin_create() -> *mut c_void {
    let plugin = Box::new(HelloPlugin::new());
    Box::into_raw(plugin) as *mut c_void
}

// VTable implementation
static VTABLE: ModuleRuntimeVTable = ModuleRuntimeVTable {
    get_id: hello_get_id,
    get_name: hello_get_name,
    is_enabled: hello_is_enabled,
    set_enabled: hello_set_enabled,
    poll_signal: hello_poll_signal,
    consume_signal: hello_consume_signal,
    apply_settings: hello_apply_settings,
    destroy: hello_destroy,
};

#[no_mangle]
pub unsafe extern "C" fn talisman_plugin_get_vtable() -> *const ModuleRuntimeVTable {
    &VTABLE as *const _
}

// VTable function implementations

unsafe extern "C" fn hello_get_id(instance: *const c_void) -> *const i8 {
    let plugin = &*(instance as *const HelloPlugin);
    plugin.id.as_ptr()
}

unsafe extern "C" fn hello_get_name(instance: *const c_void) -> *const i8 {
    let plugin = &*(instance as *const HelloPlugin);
    plugin.name.as_ptr()
}

unsafe extern "C" fn hello_is_enabled(instance: *const c_void) -> bool {
    let plugin = &*(instance as *const HelloPlugin);
    plugin.enabled
}

unsafe extern "C" fn hello_set_enabled(instance: *mut c_void, enabled: bool) {
    let plugin = &mut *(instance as *mut HelloPlugin);
    plugin.enabled = enabled;
}

unsafe extern "C" fn hello_poll_signal(instance: *mut c_void, buffer: *mut SignalBuffer) -> bool {
    let plugin = &mut *(instance as *mut HelloPlugin);

    if !plugin.enabled {
        return false;
    }

    // Every 10 polls, emit a text signal
    plugin.counter += 1;
    if plugin.counter % 10 == 0 {
        let message = format!("Hello from plugin! Counter: {}", plugin.counter);
        let message_cstring = CString::new(message).unwrap();
        let message_ptr = message_cstring.into_raw();

        (*buffer).signal_type = SignalType::Text as u32;
        (*buffer).value.ptr = message_ptr as *mut c_void;
        (*buffer).size = 0; // Null-terminated string

        true
    } else {
        false
    }
}

unsafe extern "C" fn hello_consume_signal(
    _instance: *mut c_void,
    _buffer: *const SignalBuffer,
) -> *mut SignalBuffer {
    // This example plugin doesn't consume signals, just produces them
    // Return null to indicate no output signal
    std::ptr::null_mut()
}

unsafe extern "C" fn hello_apply_settings(_instance: *mut c_void, _json: *const i8) {
    // No-op for hello plugin
}

unsafe extern "C" fn hello_destroy(instance: *mut c_void) {
    let _ = Box::from_raw(instance as *mut HelloPlugin);
}
