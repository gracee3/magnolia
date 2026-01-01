pub use magnolia_plugin_abi;
// Re-export common types for convenience
pub use magnolia_plugin_abi::{
    ABI_VERSION, ModuleRuntimeVTable, PluginManifest, SignalBuffer, SignalType, SignalValue,
};

use std::os::raw::c_char;

/// Macro to export the necessary C-ABI symbols for a Magnolia plugin.
#[macro_export]
macro_rules! export_plugin {
    ($plugin_type:ty) => {
        // We use full paths to avoid import conflicts

        // --- MANIFEST ---
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn magnolia_plugin_manifest() -> $crate::PluginManifest {
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
        pub unsafe extern "C" fn magnolia_plugin_create() -> *mut std::os::raw::c_void {
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
            apply_settings: _plugin_apply_settings,
            destroy: _plugin_destroy,
        };

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn magnolia_plugin_get_vtable() -> *const $crate::ModuleRuntimeVTable
        {
            &VTABLE as *const _
        }

        // --- SCHEMA ---
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn magnolia_plugin_get_schema()
        -> *const $crate::magnolia_plugin_abi::ModuleSchemaAbi {
            // Leak strings to keep them valid for the lifetime of the plugin (static)
            use std::ffi::CString;

            // Note: We don't support ports via macro yet, user must implement strict ABI manually if they want ports.
            // But we do support settings_schema.

            static mut SCHEMA: Option<$crate::magnolia_plugin_abi::ModuleSchemaAbi> = None;
            static mut SCHEMA_INIT: std::sync::Once = std::sync::Once::new();

            unsafe {
                SCHEMA_INIT.call_once(|| {
                    let id = CString::new(<$plugin_type>::name()).unwrap(); // Use name as ID for now
                    let name = CString::new(<$plugin_type>::name()).unwrap();
                    let desc = CString::new(<$plugin_type>::description()).unwrap();

                    let settings = if let Some(s) = <$plugin_type>::settings_schema() {
                        CString::new(s).unwrap().into_raw()
                    } else {
                        std::ptr::null()
                    };

                    SCHEMA = Some($crate::magnolia_plugin_abi::ModuleSchemaAbi {
                        id: id.into_raw(),
                        name: name.into_raw(),
                        description: desc.into_raw(),
                        ports: std::ptr::null(), // Ports not supported via basic macro yet
                        ports_len: 0,
                        settings_schema: settings,
                    });
                });

                SCHEMA.as_ref().unwrap() as *const _
            }
        }

        // --- TRAMPOLINES ---

        unsafe extern "C" fn _plugin_get_id(
            instance: *const std::os::raw::c_void,
        ) -> *const std::os::raw::c_char {
            let plugin = &*(instance as *const $plugin_type);
            plugin.c_id()
        }

        unsafe extern "C" fn _plugin_get_name(
            instance: *const std::os::raw::c_void,
        ) -> *const std::os::raw::c_char {
            let plugin = &*(instance as *const $plugin_type);
            plugin.c_name()
        }

        unsafe extern "C" fn _plugin_is_enabled(instance: *const std::os::raw::c_void) -> bool {
            let plugin = &*(instance as *const $plugin_type);
            plugin.is_enabled()
        }

        unsafe extern "C" fn _plugin_set_enabled(
            instance: *mut std::os::raw::c_void,
            enabled: bool,
        ) {
            let plugin = &mut *(instance as *mut $plugin_type);
            plugin.set_enabled(enabled);
        }

        unsafe extern "C" fn _plugin_poll_signal(
            instance: *mut std::os::raw::c_void,
            buffer: *mut $crate::SignalBuffer,
        ) -> bool {
            let plugin = &mut *(instance as *mut $plugin_type);
            plugin.poll_signal(&mut *buffer)
        }

        unsafe extern "C" fn _plugin_consume_signal(
            instance: *mut std::os::raw::c_void,
            buffer: *const $crate::SignalBuffer,
        ) -> *mut $crate::SignalBuffer {
            let plugin = &mut *(instance as *mut $plugin_type);
            match plugin.consume_signal(&*buffer) {
                Some(mut out) => Box::into_raw(Box::new(out)),
                None => std::ptr::null_mut(),
            }
        }

        unsafe extern "C" fn _plugin_apply_settings(
            instance: *mut std::os::raw::c_void,
            json: *const std::os::raw::c_char,
        ) {
            let plugin = &mut *(instance as *mut $plugin_type);
            if !json.is_null() {
                if let Ok(c_str) = std::ffi::CStr::from_ptr(json).to_str() {
                    plugin.apply_settings(c_str);
                }
            }
        }

        unsafe extern "C" fn _plugin_destroy(instance: *mut std::os::raw::c_void) {
            let _ = Box::from_raw(instance as *mut $plugin_type);
        }
    };
}

pub trait MagnoliaPlugin: Default {
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

    // Settings
    fn settings_schema() -> Option<String> {
        None
    }
    fn apply_settings(&mut self, _json: &str) {}
}
