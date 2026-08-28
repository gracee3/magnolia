use magnolia_domain::{DeviceFingerprint, DeviceSelector};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceDirection {
    Input,
    Output,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryDevice {
    pub global_id: u32,
    pub runtime_id: String,
    pub direction: DeviceDirection,
    pub fingerprint: DeviceFingerprint,
    pub label: String,
    pub channel_positions: Vec<String>,
}

impl RegistryDevice {
    #[must_use]
    pub fn new(
        global_id: u32,
        direction: DeviceDirection,
        fingerprint: DeviceFingerprint,
        label: String,
    ) -> Self {
        let runtime_id = deterministic_runtime_id(&fingerprint);
        Self {
            global_id,
            runtime_id,
            direction,
            fingerprint,
            label,
            channel_positions: Vec::new(),
        }
    }
}

#[derive(Debug, Default)]
pub struct DeviceRegistry {
    devices: BTreeMap<u32, RegistryDevice>,
    default_input_node_name: Option<String>,
    default_output_node_name: Option<String>,
    revision: u64,
}

impl DeviceRegistry {
    pub fn add_or_replace(&mut self, device: RegistryDevice) {
        self.devices.insert(device.global_id, device);
        self.revision = self.revision.saturating_add(1);
    }

    pub fn remove(&mut self, global_id: u32) -> Option<RegistryDevice> {
        let removed = self.devices.remove(&global_id);
        if removed.is_some() {
            self.revision = self.revision.saturating_add(1);
        }
        removed
    }

    pub fn set_default_input(&mut self, node_name: Option<String>) {
        if self.default_input_node_name != node_name {
            self.default_input_node_name = node_name;
            self.revision = self.revision.saturating_add(1);
        }
    }

    pub fn set_default_output(&mut self, node_name: Option<String>) {
        if self.default_output_node_name != node_name {
            self.default_output_node_name = node_name;
            self.revision = self.revision.saturating_add(1);
        }
    }

    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn resolve_input(
        &self,
        selector: &DeviceSelector,
    ) -> Result<&RegistryDevice, DeviceResolutionError> {
        match selector {
            DeviceSelector::FollowDefaultInput => {
                let node_name = self
                    .default_input_node_name
                    .as_deref()
                    .ok_or(DeviceResolutionError::DefaultInputUnavailable)?;
                self.devices
                    .values()
                    .find(|device| {
                        device.direction == DeviceDirection::Input
                            && device.fingerprint.node_name == node_name
                    })
                    .ok_or(DeviceResolutionError::DefaultInputUnavailable)
            }
            DeviceSelector::Exact { fingerprint } => self
                .devices
                .values()
                .find(|device| {
                    device.direction == DeviceDirection::Input && &device.fingerprint == fingerprint
                })
                .ok_or_else(|| DeviceResolutionError::ExactInputUnavailable(fingerprint.clone())),
        }
    }

    pub fn devices(&self) -> impl Iterator<Item = &RegistryDevice> {
        self.devices.values()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DeviceResolutionError {
    #[error("PipeWire default input metadata is unavailable or unresolved")]
    DefaultInputUnavailable,
    #[error("exact PipeWire input fingerprint is unavailable: {0:?}")]
    ExactInputUnavailable(DeviceFingerprint),
}

#[must_use]
pub fn deterministic_runtime_id(fingerprint: &DeviceFingerprint) -> String {
    // Fixed FNV-1a parameters make identifiers stable across processes and do
    // not conflate the user-facing label with device identity.
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in fingerprint
        .node_name
        .bytes()
        .chain([0])
        .chain(fingerprint.device_api.bytes())
        .chain([0])
        .chain(fingerprint.object_path.bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("pw-{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fingerprint(name: &str) -> DeviceFingerprint {
        DeviceFingerprint {
            node_name: name.to_owned(),
            device_api: "alsa".to_owned(),
            object_path: format!("alsa:{name}"),
        }
    }

    #[test]
    fn defaults_are_honest_and_exact_devices_never_fall_back() {
        let mut registry = DeviceRegistry::default();
        registry.add_or_replace(RegistryDevice::new(
            7,
            DeviceDirection::Input,
            fingerprint("source.a"),
            "Microphone".to_owned(),
        ));
        assert_eq!(
            registry.resolve_input(&DeviceSelector::FollowDefaultInput),
            Err(DeviceResolutionError::DefaultInputUnavailable)
        );
        registry.set_default_input(Some("source.a".to_owned()));
        assert_eq!(
            registry
                .resolve_input(&DeviceSelector::FollowDefaultInput)
                .map(|device| device.global_id),
            Ok(7)
        );
        assert!(matches!(
            registry.resolve_input(&DeviceSelector::Exact {
                fingerprint: fingerprint("source.missing")
            }),
            Err(DeviceResolutionError::ExactInputUnavailable(_))
        ));
    }

    #[test]
    fn runtime_identity_ignores_labels_and_survives_global_id_changes() {
        let fingerprint = fingerprint("source.a");
        let first = RegistryDevice::new(
            7,
            DeviceDirection::Input,
            fingerprint.clone(),
            "Old label".to_owned(),
        );
        let second = RegistryDevice::new(
            91,
            DeviceDirection::Input,
            fingerprint,
            "New label".to_owned(),
        );
        assert_eq!(first.runtime_id, second.runtime_id);
    }
}
