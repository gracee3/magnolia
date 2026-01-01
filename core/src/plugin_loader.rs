use anyhow::{Context, Result};
use libloading::{Library, Symbol};
use std::ffi::CStr;
use std::os::raw::c_void;
use std::path::{Path, PathBuf};
use magnolia_plugin_abi::*;

/// Loaded plugin library with manifest and vtable
pub struct PluginLibrary {
    _lib: Library,
    pub manifest: PluginManifest,
    pub vtable: &'static ModuleRuntimeVTable,
    pub instance: *mut c_void,
    pub schema: Option<*const ModuleSchemaAbi>,
}

// Safety: The plugin instance must be thread-safe for the operations called on it.
// We are bridging C ABI which involves raw pointers.
unsafe impl Send for PluginLibrary {}
unsafe impl Sync for PluginLibrary {}

impl PluginLibrary {
    /// Load a plugin from a shared library file
    ///
    /// # Safety
    ///
    /// This loads arbitrary code from a .so/.dll file. Only load trusted plugins.
    pub unsafe fn load(path: &Path) -> Result<Self> {
        log::info!("Loading plugin from: {}", path.display());

        let lib = Library::new(path)
            .with_context(|| format!("Failed to load library: {}", path.display()))?;

        // Get manifest
        let manifest_fn: Symbol<PluginManifestFn> = lib
            .get(PLUGIN_MANIFEST_SYMBOL)
            .context("Plugin missing magnolia_plugin_manifest symbol")?;
        let manifest = manifest_fn();

        // Check ABI version
        if manifest.abi_version != ABI_VERSION {
            anyhow::bail!(
                "ABI version mismatch: plugin has {}, expected {}",
                manifest.abi_version,
                ABI_VERSION
            );
        }

        // Get vtable
        let vtable_fn: Symbol<PluginGetVTableFn> = lib
            .get(PLUGIN_VTABLE_SYMBOL)
            .context("Plugin missing magnolia_plugin_get_vtable symbol")?;
        let vtable = &*vtable_fn();

        // Get schema (optional)
        let schema = if let Ok(schema_fn) = lib.get::<PluginGetSchemaFn>(PLUGIN_SCHEMA_SYMBOL) {
            log::info!("Plugin exports schema symbol");
            let schema_ptr = schema_fn();
            if !schema_ptr.is_null() {
                Some(schema_ptr)
            } else {
                None
            }
        } else {
            log::debug!("Plugin does not export schema symbol");
            None
        };

        // Create instance
        let create_fn: Symbol<PluginCreateFn> = lib
            .get(PLUGIN_CREATE_SYMBOL)
            .context("Plugin missing magnolia_plugin_create symbol")?;
        let instance = create_fn();

        if instance.is_null() {
            anyhow::bail!("Plugin create function returned null");
        }

        let name = CStr::from_ptr(manifest.name).to_string_lossy();
        let version = CStr::from_ptr(manifest.version).to_string_lossy();
        log::info!("Loaded plugin: {} v{}", name, version);

        Ok(Self {
            _lib: lib,
            manifest,
            vtable,
            instance,
            schema,
        })
    }

    pub fn name(&self) -> String {
        unsafe {
            CStr::from_ptr(self.manifest.name)
                .to_string_lossy()
                .into_owned()
        }
    }
}

impl Drop for PluginLibrary {
    fn drop(&mut self) {
        unsafe {
            (self.vtable.destroy)(self.instance);
        }
    }
}

/// Plugin discovery and loading
pub struct PluginLoader {
    plugin_dirs: Vec<PathBuf>,
    pub loaded: Vec<PluginLibrary>,
}

impl PluginLoader {
    pub fn new() -> Self {
        let mut dirs = vec![PathBuf::from("./plugins")];

        // Add user plugin directory
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join(".talisman/plugins"));
        }

        Self {
            plugin_dirs: dirs,
            loaded: Vec::new(),
        }
    }

    /// Add a custom plugin directory
    pub fn add_plugin_dir(&mut self, dir: PathBuf) {
        self.plugin_dirs.push(dir);
    }

    /// Discover all plugin files in configured directories
    pub fn discover(&self) -> Result<Vec<PathBuf>> {
        let mut plugins = Vec::new();

        for dir in &self.plugin_dirs {
            if !dir.exists() {
                log::debug!("Plugin directory does not exist: {}", dir.display());
                continue;
            }

            log::info!("Scanning for plugins in: {}", dir.display());

            for entry in std::fs::read_dir(dir)
                .with_context(|| format!("Failed to read directory: {}", dir.display()))?
            {
                let entry = entry?;
                let path = entry.path();

                // Check if it's a plugin library
                if self.is_plugin_file(&path) {
                    // Check if already loaded?
                    // For now, simple discovery
                    log::debug!("Found plugin file: {}", path.display());
                    plugins.push(path);
                }
            }
        }

        Ok(plugins)
    }

    /// Drain all loaded plugins, transferring ownership to the caller
    pub fn drain_loaded(&mut self) -> Vec<PluginLibrary> {
        self.loaded.drain(..).collect()
    }

    fn is_plugin_file(&self, path: &Path) -> bool {
        if !path.is_file() {
            return false;
        }

        let Some(ext) = path.extension() else {
            return false;
        };

        #[cfg(target_os = "linux")]
        return ext == "so";

        #[cfg(target_os = "windows")]
        return ext == "dll";

        #[cfg(target_os = "macos")]
        return ext == "dylib";

        #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
        false
    }

    /// Load a specific plugin
    ///
    /// # Safety
    ///
    /// Loads arbitrary code from shared library
    pub unsafe fn load_plugin(&mut self, path: &Path) -> Result<()> {
        let plugin = PluginLibrary::load(path)?;
        self.loaded.push(plugin);
        Ok(())
    }

    /// Discover and load all plugins
    pub unsafe fn load_all(&mut self) -> Result<usize> {
        let plugin_paths = self.discover()?;
        let mut loaded_count = 0;

        for path in plugin_paths {
            match self.load_plugin(&path) {
                Ok(_) => loaded_count += 1,
                Err(e) => log::error!("Failed to load plugin {}: {}", path.display(), e),
            }
        }

        log::info!("Loaded {} plugins", loaded_count);
        Ok(loaded_count)
    }
}

impl Default for PluginLoader {
    fn default() -> Self {
        Self::new()
    }
}
