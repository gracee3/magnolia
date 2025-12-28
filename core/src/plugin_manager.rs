use anyhow::Result;
use notify::{Watcher, RecursiveMode, Event, RecommendedWatcher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock, mpsc};
use log::{info, error};

use crate::plugin_loader::{PluginLoader, PluginLibrary};

pub struct PluginManager {
    // Shared loader state
    pub loader: Arc<RwLock<PluginLoader>>,
    
    // File watcher
    watcher: Option<RecommendedWatcher>,
    
    // Channel to notify about reload events (path of reloaded plugin)
    reload_tx: mpsc::Sender<PathBuf>,
    pub reload_rx: mpsc::Receiver<PathBuf>,
}

impl PluginManager {
    pub fn new() -> Self {
        let (reload_tx, reload_rx) = mpsc::channel();
        
        Self {
            loader: Arc::new(RwLock::new(PluginLoader::new())),
            watcher: None,
            reload_tx,
            reload_rx,
        }
    }
    
    /// Enable hot-reloading by watching plugin directories
    pub fn enable_hot_reload(&mut self) -> Result<()> {
        let reload_tx = self.reload_tx.clone();
        
        // Create watcher logic wrapped in sync API of notify 6.0
        let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            match res {
                Ok(event) => {
                    if event.kind.is_modify() {
                        for path in event.paths {
                            if let Some(ext) = path.extension() {
                                let ext_str = ext.to_string_lossy();
                                if ext_str == "so" || ext_str == "dll" || ext_str == "dylib" {
                                    info!("Plugin changed: {}", path.display());
                                    let _ = reload_tx.send(path.clone());
                                }
                            }
                        }
                    }
                }
                Err(e) => error!("Watch error: {:?}", e),
            }
        })?;
        
        // Watch all plugin directories configured in loader
        // We need to read-lock the loader to get dirs
        let dirs = {
             // This is a bit rigid as we need public access to dirs in PluginLoader or method
             // For now assume standard dirs + ./plugins
             vec![PathBuf::from("./plugins")]
        };
        
        for dir in dirs {
            if dir.exists() {
                info!("Watching for hot-reload: {}", dir.display());
                watcher.watch(&dir, RecursiveMode::NonRecursive)?;
            }
        }
        
        self.watcher = Some(watcher);
        Ok(())
    }
    
    /// Handle the reload of a plugin by path
    /// This should be called when a path is received from reload_rx
    pub fn reload_plugin(&self, path: &Path) -> Result<PluginLibrary> {
        // Since we are creating a fresh library instance, we don't strictly need the write lock 
        // on the loader unless we are updating the loader's internal list.
        // Current PluginLoader::load doesn't update list, PluginLoader::load_plugin does.
        // We probably want to just load it isolated.
        
        info!("Reloading plugin code from {}", path.display());
        
        // Unsafe load - verification happens inside
        unsafe {
            PluginLibrary::load(path)
        }
    }
}
