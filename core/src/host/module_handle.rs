use talisman_module_api::{StaticModule, ControlMsg, ControlCx, TickCx, Manifest, PortDesc};
use std::sync::{Arc, Mutex};
use crate::plugin_adapter::PluginModuleAdapter;

/// A unified wrapper for both static and dynamic modules.
pub enum ModuleImpl {
    /// A statically compiled Rust module (Tier 0/1)
    Static(Box<dyn StaticModule>),
    /// A dynamically loaded plugin (Tier 2) - TODO: wrap PluginModuleAdapter or similar
    /// For now, we use a placeholder or the existing adapter if it fits.
    /// The existing adapter is async-based, while the new model implies a synchronous tick.
    /// We will need to bridge this.
    Dynamic(Arc<Mutex<PluginModuleAdapter>>), 
}

pub struct ModuleHandle {
    pub id: String,
    pub inner: ModuleImpl,
    pub manifest: Manifest,
    pub ports: Vec<PortDesc>,
}

impl ModuleHandle {
    pub fn new_static(mut module: Box<dyn StaticModule>) -> Self {
        let manifest = module.manifest();
        let ports = module.ports().to_vec();
        
        Self {
            id: manifest.id.clone(),
            inner: ModuleImpl::Static(module),
            manifest,
            ports,
        }
    }

    pub fn tick(&mut self, cx: &mut TickCx) {
        match &mut self.inner {
            ModuleImpl::Static(module) => module.tick(cx),
            ModuleImpl::Dynamic(_adapter) => {
                // TODO: Dynamic plugins don't tick on the RT thread yet.
                // Or we bridge to them differently.
                // For Phase 0/1, we focus on Static.
            }
        }
    }

    pub fn on_control(&mut self, cx: &mut ControlCx, msg: ControlMsg) {
        match &mut self.inner {
            ModuleImpl::Static(module) => module.on_control(cx, msg),
            ModuleImpl::Dynamic(_adapter) => {
                // TODO: Bridge control messages to dynamic plugin
            }
        }
    }
}
