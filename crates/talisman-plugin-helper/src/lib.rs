pub use talisman_plugin_abi;
// Re-export common types for convenience
pub use talisman_plugin_abi::{
    SignalBuffer, SignalType, SignalValue, PluginManifest, ModuleRuntimeVTable,
    ABI_VERSION,
};

use std::os::raw::c_char;

/// Macro to export the necessary C-ABI symbols for a Talisman plugin.
#[macro_export]
macro_rules! export_plugin {
    ($plugin_type:ty) => {
        // We use full paths to avoid import conflicts
        
        // --- MANIFEST ---
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn talisman_plugin_manifest() -> $crate::PluginManifest {
            use std::ffi::CString;
            let name = CString::new(<$plugin_type>::name()).unwrap();
            let ver = CString::new(<$plugin_type>::version()).unwrap();
            let desc = CString::new(<$plugin_type>::description()).unwrap();
            let auth = CString::new(<$plugin_type>::author()).unwrap();
            
            $crate::PluginManifest {
                abi_version: $crate::ABI_VERSION,
                name: name.into_raw(),
                version: ver.into_raw(),
                description: desc.into_raw(),
                author: auth.into_raw(),
            }
        }

        // --- CREATE ---
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn talisman_plugin_create() -> *mut std::os::raw::c_void {
            let plugin = Box::new(<$plugin_type>::default());
            Box::into_raw(plugin) as *mut std::os::raw::c_void
        }

        // --- VTABLE ---
        static VTABLE: $crate::ModuleRuntimeVTable = $crate::ModuleRuntimeVTable {
            get_id: _plugin_get_id,
            get_name: _plugin_get_name,
            is_enabled: _plugin_is_enabled,
            set_enabled: _plugin_set_enabled,
            poll_signal: _plugin_poll_signal,
            consume_signal: _plugin_consume_signal,
            destroy: _plugin_destroy,
        };

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn talisman_plugin_get_vtable() -> *const $crate::ModuleRuntimeVTable {
            &VTABLE as *const _
        }

        // --- TRAMPOLINES ---

        unsafe extern "C" fn _plugin_get_id(instance: *const std::os::raw::c_void) -> *const std::os::raw::c_char {
            let plugin = &*(instance as *const $plugin_type);
            plugin.c_id()
        }

        unsafe extern "C" fn _plugin_get_name(instance: *const std::os::raw::c_void) -> *const std::os::raw::c_char {
            let plugin = &*(instance as *const $plugin_type);
            plugin.c_name()
        }

        unsafe extern "C" fn _plugin_is_enabled(instance: *const std::os::raw::c_void) -> bool {
            let plugin = &*(instance as *const $plugin_type);
            plugin.is_enabled()
        }

        unsafe extern "C" fn _plugin_set_enabled(instance: *mut std::os::raw::c_void, enabled: bool) {
            let plugin = &mut *(instance as *mut $plugin_type);
            plugin.set_enabled(enabled);
        }

        unsafe extern "C" fn _plugin_poll_signal(instance: *mut std::os::raw::c_void, buffer: *mut $crate::SignalBuffer) -> bool {
            let plugin = &mut *(instance as *mut $plugin_type);
            plugin.poll_signal(&mut *buffer)
        }

        unsafe extern "C" fn _plugin_consume_signal(instance: *mut std::os::raw::c_void, buffer: *const $crate::SignalBuffer) -> *mut $crate::SignalBuffer {
            let plugin = &mut *(instance as *mut $plugin_type);
            match plugin.consume_signal(&*buffer) {
                Some(mut out) => {
                    Box::into_raw(Box::new(out))
                }
                None => std::ptr::null_mut()
            }
        }

        unsafe extern "C" fn _plugin_destroy(instance: *mut std::os::raw::c_void) {
            let _ = Box::from_raw(instance as *mut $plugin_type);
        }
    };
}

pub trait TalismanPlugin: Default {
    // Metadata
    fn name() -> &'static str;
    fn version() -> &'static str;
    fn description() -> &'static str;
    fn author() -> &'static str;

    // Runtime
    fn c_id(&self) -> *const c_char;
    fn c_name(&self) -> *const c_char;
    
    fn is_enabled(&self) -> bool;
    fn set_enabled(&mut self, enabled: bool);
    
    fn poll_signal(&mut self, buffer: &mut SignalBuffer) -> bool;
    fn consume_signal(&mut self, input: &SignalBuffer) -> Option<SignalBuffer>;
}
