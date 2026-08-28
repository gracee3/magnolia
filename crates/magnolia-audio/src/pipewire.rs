//! Linux PipeWire device discovery.

use crate::{DeviceDirection, DeviceRegistry, RegistryDevice};
use magnolia_domain::DeviceFingerprint;
use pipewire as pw;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    sync::{Arc, Mutex, MutexGuard},
    thread::{self, JoinHandle},
};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputDevice {
    pub global_id: u32,
    pub node_name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceSelector {
    FollowDefault,
    ExactNodeName(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ResolveDeviceError {
    #[error("exact PipeWire input node is unavailable: {0}")]
    ExactDeviceMissing(String),
    #[error("the default PipeWire input is unavailable because default metadata was not supplied")]
    DefaultInputUnavailable,
}

pub fn resolve_device<'a>(
    devices: &'a [InputDevice],
    selector: &DeviceSelector,
) -> Result<&'a InputDevice, ResolveDeviceError> {
    match selector {
        // Registry enumeration does not identify the default. Returning any
        // device here would silently change user intent based on sort order.
        DeviceSelector::FollowDefault => Err(ResolveDeviceError::DefaultInputUnavailable),
        DeviceSelector::ExactNodeName(name) => devices
            .iter()
            .find(|device| device.node_name == *name)
            .ok_or_else(|| ResolveDeviceError::ExactDeviceMissing(name.clone())),
    }
}

/// Enumerate current PipeWire audio source nodes and return after one core sync.
pub fn discover_inputs() -> Result<Vec<InputDevice>, PipeWireDiscoveryError> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;
    let registry = core.get_registry()?;
    let devices = Rc::new(RefCell::new(Vec::new()));
    let discovered = devices.clone();

    let _registry_listener = registry
        .add_listener_local()
        .global(move |object| {
            if object.type_ != pw::types::ObjectType::Node {
                return;
            }
            let Some(properties) = object.props.as_ref() else {
                return;
            };
            let Some(media_class) = properties.get(*pw::keys::MEDIA_CLASS) else {
                return;
            };
            if media_class != "Audio/Source" && media_class != "Audio/Source/Virtual" {
                return;
            }
            let Some(node_name) = properties.get(*pw::keys::NODE_NAME) else {
                return;
            };
            discovered.borrow_mut().push(InputDevice {
                global_id: object.id,
                node_name: node_name.to_owned(),
                description: properties
                    .get(*pw::keys::NODE_DESCRIPTION)
                    .map(str::to_owned),
            });
        })
        .register();

    let pending = core.sync(0)?;
    let loop_for_done = mainloop.clone();
    let _core_listener = core
        .add_listener_local()
        .done(move |id, sequence| {
            if id == pw::core::PW_ID_CORE && sequence == pending {
                loop_for_done.quit();
            }
        })
        .register();
    mainloop.run();

    let mut result = devices.borrow().clone();
    result.sort_by(|left, right| left.node_name.cmp(&right.node_name));
    Ok(result)
}

#[derive(Debug, Error)]
pub enum PipeWireDiscoveryError {
    #[error("PipeWire discovery failed: {0}")]
    PipeWire(#[from] pw::Error),
    #[error("failed to start PipeWire registry thread: {0}")]
    Thread(std::io::Error),
}

pub struct PipeWireRegistryManager {
    registry: Arc<Mutex<DeviceRegistry>>,
    stop: Option<pw::channel::Sender<()>>,
    worker: Option<JoinHandle<Result<(), PipeWireDiscoveryError>>>,
}

impl PipeWireRegistryManager {
    pub fn start() -> Result<Self, PipeWireDiscoveryError> {
        pw::init();
        let registry = Arc::new(Mutex::new(DeviceRegistry::default()));
        let worker_registry = Arc::clone(&registry);
        let (stop, receiver) = pw::channel::channel();
        let worker = thread::Builder::new()
            .name("magnolia-pipewire-registry".to_owned())
            .spawn(move || run_registry_loop(worker_registry, receiver))
            .map_err(PipeWireDiscoveryError::Thread)?;
        Ok(Self {
            registry,
            stop: Some(stop),
            worker: Some(worker),
        })
    }

    pub fn snapshot(&self) -> DeviceRegistry {
        self.lock().clone()
    }

    fn lock(&self) -> MutexGuard<'_, DeviceRegistry> {
        self.registry
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }
}

impl Drop for PipeWireRegistryManager {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run_registry_loop(
    state: Arc<Mutex<DeviceRegistry>>,
    stop: pw::channel::Receiver<()>,
) -> Result<(), PipeWireDiscoveryError> {
    use pw::metadata::Metadata;

    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;
    let registry = core.get_registry_rc()?;
    let weak_registry = registry.downgrade();
    let metadata_objects = Rc::new(RefCell::new(Vec::new()));
    let device_apis = Rc::new(RefCell::new(BTreeMap::<u32, String>::new()));
    let apis_for_global = Rc::clone(&device_apis);
    let metadata_for_global = Rc::clone(&metadata_objects);
    let node_state = Arc::clone(&state);
    let metadata_state = Arc::clone(&state);
    let remove_state = Arc::clone(&state);

    let _stop = stop.attach(mainloop.loop_(), {
        let mainloop = mainloop.clone();
        move |_| mainloop.quit()
    });
    let _listener = registry
        .add_listener_local()
        .global(move |object| match object.type_ {
            pw::types::ObjectType::Device => {
                if let Some(api) = object
                    .props
                    .as_ref()
                    .and_then(|properties| properties.get(*pw::keys::DEVICE_API))
                {
                    apis_for_global
                        .borrow_mut()
                        .insert(object.id, api.to_owned());
                }
            }
            pw::types::ObjectType::Node => {
                let Some(properties) = object.props.as_ref() else {
                    return;
                };
                let Some(media_class) = properties.get(*pw::keys::MEDIA_CLASS) else {
                    return;
                };
                let direction = match media_class {
                    "Audio/Source" | "Audio/Source/Virtual" => DeviceDirection::Input,
                    "Audio/Sink" => DeviceDirection::Output,
                    _ => return,
                };
                let Some(node_name) = properties.get(*pw::keys::NODE_NAME) else {
                    return;
                };
                let Some(device_id) = properties
                    .get(*pw::keys::DEVICE_ID)
                    .and_then(|value| value.parse::<u32>().ok())
                else {
                    return;
                };
                let Some(device_api) = apis_for_global.borrow().get(&device_id).cloned() else {
                    return;
                };
                let Some(object_path) = properties.get(*pw::keys::OBJECT_PATH) else {
                    return;
                };
                let fingerprint = DeviceFingerprint {
                    node_name: node_name.to_owned(),
                    device_api,
                    object_path: object_path.to_owned(),
                };
                let label = properties
                    .get(*pw::keys::NODE_DESCRIPTION)
                    .unwrap_or(node_name)
                    .to_owned();
                node_state
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .add_or_replace(RegistryDevice::new(
                        object.id,
                        direction,
                        fingerprint,
                        label,
                    ));
            }
            pw::types::ObjectType::Metadata => {
                let is_default = object
                    .props
                    .as_ref()
                    .and_then(|properties| properties.get("metadata.name"))
                    == Some("default");
                if !is_default {
                    return;
                }
                let Some(registry) = weak_registry.upgrade() else {
                    return;
                };
                let Ok(metadata) = registry.bind::<Metadata, _>(object) else {
                    return;
                };
                let listener = metadata
                    .add_listener_local()
                    .property({
                        let state = Arc::clone(&metadata_state);
                        move |_subject, key, _value_type, value| {
                            match key {
                                Some("default.audio.source") => {
                                    state
                                        .lock()
                                        .unwrap_or_else(|error| error.into_inner())
                                        .set_default_input(parse_default_node_name(value));
                                }
                                Some("default.audio.sink") => {
                                    state
                                        .lock()
                                        .unwrap_or_else(|error| error.into_inner())
                                        .set_default_output(parse_default_node_name(value));
                                }
                                _ => {}
                            }
                            0
                        }
                    })
                    .register();
                metadata_for_global.borrow_mut().push((metadata, listener));
            }
            _ => {}
        })
        .global_remove(move |id| {
            remove_state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(id);
        })
        .register();
    mainloop.run();
    Ok(())
}

fn parse_default_node_name(value: Option<&str>) -> Option<String> {
    let value = value?;
    serde_json::from_str::<serde_json::Value>(value)
        .ok()
        .and_then(|value| value.get("name")?.as_str().map(str::to_owned))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_selector_never_falls_back() {
        let devices = vec![InputDevice {
            global_id: 7,
            node_name: "test.source".to_owned(),
            description: None,
        }];
        assert!(matches!(
            resolve_device(
                &devices,
                &DeviceSelector::ExactNodeName("missing".to_owned())
            ),
            Err(ResolveDeviceError::ExactDeviceMissing(_))
        ));
    }

    #[test]
    fn unresolved_default_never_uses_the_first_sorted_device() {
        let devices = vec![InputDevice {
            global_id: 7,
            node_name: "first.sorted.source".to_owned(),
            description: None,
        }];
        assert_eq!(
            resolve_device(&devices, &DeviceSelector::FollowDefault),
            Err(ResolveDeviceError::DefaultInputUnavailable)
        );
    }

    #[test]
    fn default_metadata_json_extracts_only_a_node_name() {
        assert_eq!(
            parse_default_node_name(Some(r#"{"name":"source.default"}"#)),
            Some("source.default".to_owned())
        );
        assert_eq!(parse_default_node_name(Some("not-json")), None);
        assert_eq!(parse_default_node_name(None), None);
    }
}
