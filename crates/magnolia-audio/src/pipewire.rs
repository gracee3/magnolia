//! Linux PipeWire device discovery.

use pipewire as pw;
use std::{cell::RefCell, rc::Rc};
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
}
