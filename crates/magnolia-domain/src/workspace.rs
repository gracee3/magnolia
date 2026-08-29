use crate::{
    DescriptorRegistry, DocumentRevision, Edge, EntityId, GraphValidationError, ModuleInstance,
    SchemaVersion, WorkspaceGraph,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use thiserror::Error;

pub const DOCUMENT_SCHEMA: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceFingerprint {
    pub node_name: String,
    pub device_api: String,
    pub object_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DeviceSelector {
    FollowDefaultInput,
    Exact { fingerprint: DeviceFingerprint },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitAxis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LayoutNode {
    Tile {
        tile_id: EntityId,
    },
    Tabs {
        active: usize,
        children: Vec<LayoutNode>,
    },
    Split {
        axis: SplitAxis,
        /// Integer millionths avoids float equality and serialization drift.
        ratio_millionths: u32,
        first: Box<LayoutNode>,
        second: Box<LayoutNode>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutPreset {
    pub root: LayoutNode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TileBinding {
    #[serde(default)]
    pub module_ids: Vec<EntityId>,
    #[serde(default)]
    pub resource_ids: Vec<EntityId>,
    #[serde(default)]
    pub settings: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceDocument {
    pub schema: SchemaVersion,
    pub revision: DocumentRevision,
    pub graph: WorkspaceGraph,
    #[serde(default)]
    pub tile_bindings: BTreeMap<EntityId, TileBinding>,
    #[serde(default)]
    pub presets: BTreeMap<String, LayoutPreset>,
    #[serde(default)]
    pub promoted_settings: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub device_selectors: BTreeMap<String, DeviceSelector>,
}

impl Default for WorkspaceDocument {
    fn default() -> Self {
        Self {
            schema: DOCUMENT_SCHEMA,
            revision: DocumentRevision::ZERO,
            graph: WorkspaceGraph::default(),
            tile_bindings: BTreeMap::new(),
            presets: BTreeMap::new(),
            promoted_settings: BTreeMap::new(),
            device_selectors: BTreeMap::new(),
        }
    }
}

impl WorkspaceDocument {
    pub fn validate(&self, registry: &DescriptorRegistry) -> Result<(), WorkspaceError> {
        if self.schema.major != DOCUMENT_SCHEMA.major {
            return Err(WorkspaceError::UnsupportedSchemaMajor {
                received: self.schema.major,
                supported: DOCUMENT_SCHEMA.major,
            });
        }
        registry.validate_graph(&self.graph)?;
        for (tile_id, binding) in &self.tile_bindings {
            for module_id in &binding.module_ids {
                if !self.graph.modules.contains_key(module_id) {
                    return Err(WorkspaceError::TileReferencesMissingModule {
                        tile_id: *tile_id,
                        module_id: *module_id,
                    });
                }
            }
        }
        for (name, preset) in &self.presets {
            if name.trim().is_empty() {
                return Err(WorkspaceError::BlankPresetName);
            }
            validate_layout(&preset.root, &self.tile_bindings)?;
        }
        if self
            .promoted_settings
            .keys()
            .any(|key| key.trim().is_empty())
        {
            return Err(WorkspaceError::BlankSettingKey);
        }
        if self
            .device_selectors
            .keys()
            .any(|key| key.trim().is_empty())
        {
            return Err(WorkspaceError::BlankDeviceSelectorKey);
        }
        for selector in self.device_selectors.values() {
            if let DeviceSelector::Exact { fingerprint } = selector {
                if fingerprint.node_name.trim().is_empty()
                    || fingerprint.device_api.trim().is_empty()
                    || fingerprint.object_path.trim().is_empty()
                {
                    return Err(WorkspaceError::BlankDeviceFingerprintProperty);
                }
            }
        }
        Ok(())
    }

    pub fn to_pretty_json(&self) -> Result<String, WorkspaceError> {
        serde_json::to_string_pretty(self).map_err(WorkspaceError::Serialize)
    }

    pub fn from_json(source: &str, registry: &DescriptorRegistry) -> Result<Self, WorkspaceError> {
        let document: Self = serde_json::from_str(source).map_err(WorkspaceError::Deserialize)?;
        document.validate(registry)?;
        Ok(document)
    }

    pub fn apply(
        &self,
        batch: &WorkspaceEditBatch,
        registry: &DescriptorRegistry,
    ) -> Result<Self, WorkspaceError> {
        if batch.edits.is_empty() {
            return Err(WorkspaceError::EmptyEditBatch);
        }
        let mut candidate = self.clone();
        for edit in &batch.edits {
            apply_edit(&mut candidate, edit)?;
        }
        candidate.validate(registry)?;
        Ok(candidate)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceEditBatch {
    pub edits: Vec<WorkspaceEdit>,
}

impl WorkspaceEditBatch {
    #[must_use]
    pub fn new(edits: Vec<WorkspaceEdit>) -> Self {
        Self { edits }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkspaceEdit {
    AddModule {
        instance: ModuleInstance,
    },
    RemoveModule {
        module_id: EntityId,
    },
    AddEdge {
        edge: Edge,
    },
    RemoveEdge {
        edge_id: EntityId,
    },
    SetModuleConfiguration {
        module_id: EntityId,
        configuration: Value,
    },
    BindTile {
        tile_id: EntityId,
        binding: TileBinding,
    },
    UnbindTile {
        tile_id: EntityId,
    },
    PutPreset {
        name: String,
        preset: LayoutPreset,
    },
    RemovePreset {
        name: String,
    },
    SetPromotedSetting {
        key: String,
        value: Value,
    },
    RemovePromotedSetting {
        key: String,
    },
    SetDeviceSelector {
        key: String,
        selector: DeviceSelector,
    },
    RemoveDeviceSelector {
        key: String,
    },
}

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("unsupported document schema major {received}; supported major is {supported}")]
    UnsupportedSchemaMajor { received: u16, supported: u16 },
    #[error(transparent)]
    Graph(#[from] GraphValidationError),
    #[error("tile {tile_id} references missing module {module_id}")]
    TileReferencesMissingModule {
        tile_id: EntityId,
        module_id: EntityId,
    },
    #[error("layout references unbound tile {0}")]
    LayoutReferencesUnboundTile(EntityId),
    #[error("layout tab stack must contain children and have a valid active index")]
    InvalidTabStack,
    #[error("layout split ratio must be between 1 and 999999 millionths")]
    InvalidSplitRatio,
    #[error("preset name must not be blank")]
    BlankPresetName,
    #[error("promoted setting key must not be blank")]
    BlankSettingKey,
    #[error("device selector key must not be blank")]
    BlankDeviceSelectorKey,
    #[error("exact device fingerprint properties must not be blank")]
    BlankDeviceFingerprintProperty,
    #[error("workspace edit batch must not be empty")]
    EmptyEditBatch,
    #[error("module {0} already exists")]
    DuplicateModule(EntityId),
    #[error("module {0} does not exist")]
    MissingModule(EntityId),
    #[error("edge {0} already exists")]
    DuplicateEdge(EntityId),
    #[error("edge {0} does not exist")]
    MissingEdge(EntityId),
    #[error("tile binding {0} does not exist")]
    MissingTileBinding(EntityId),
    #[error("preset {0:?} does not exist")]
    MissingPreset(String),
    #[error("promoted setting {0:?} does not exist")]
    MissingPromotedSetting(String),
    #[error("device selector {0:?} does not exist")]
    MissingDeviceSelector(String),
    #[error("failed to serialize workspace: {0}")]
    Serialize(serde_json::Error),
    #[error("failed to deserialize workspace: {0}")]
    Deserialize(serde_json::Error),
}

fn apply_edit(
    document: &mut WorkspaceDocument,
    edit: &WorkspaceEdit,
) -> Result<(), WorkspaceError> {
    match edit {
        WorkspaceEdit::AddModule { instance } => {
            if document.graph.modules.contains_key(&instance.id) {
                return Err(WorkspaceError::DuplicateModule(instance.id));
            }
            document.graph.modules.insert(instance.id, instance.clone());
        }
        WorkspaceEdit::RemoveModule { module_id } => {
            document
                .graph
                .modules
                .remove(module_id)
                .ok_or(WorkspaceError::MissingModule(*module_id))?;
        }
        WorkspaceEdit::AddEdge { edge } => {
            if document.graph.edges.contains_key(&edge.id) {
                return Err(WorkspaceError::DuplicateEdge(edge.id));
            }
            document.graph.edges.insert(edge.id, edge.clone());
        }
        WorkspaceEdit::RemoveEdge { edge_id } => {
            document
                .graph
                .edges
                .remove(edge_id)
                .ok_or(WorkspaceError::MissingEdge(*edge_id))?;
        }
        WorkspaceEdit::SetModuleConfiguration {
            module_id,
            configuration,
        } => {
            document
                .graph
                .modules
                .get_mut(module_id)
                .ok_or(WorkspaceError::MissingModule(*module_id))?
                .configuration = configuration.clone();
        }
        WorkspaceEdit::BindTile { tile_id, binding } => {
            document.tile_bindings.insert(*tile_id, binding.clone());
        }
        WorkspaceEdit::UnbindTile { tile_id } => {
            document
                .tile_bindings
                .remove(tile_id)
                .ok_or(WorkspaceError::MissingTileBinding(*tile_id))?;
        }
        WorkspaceEdit::PutPreset { name, preset } => {
            document.presets.insert(name.clone(), preset.clone());
        }
        WorkspaceEdit::RemovePreset { name } => {
            document
                .presets
                .remove(name)
                .ok_or_else(|| WorkspaceError::MissingPreset(name.clone()))?;
        }
        WorkspaceEdit::SetPromotedSetting { key, value } => {
            document
                .promoted_settings
                .insert(key.clone(), value.clone());
        }
        WorkspaceEdit::RemovePromotedSetting { key } => {
            document
                .promoted_settings
                .remove(key)
                .ok_or_else(|| WorkspaceError::MissingPromotedSetting(key.clone()))?;
        }
        WorkspaceEdit::SetDeviceSelector { key, selector } => {
            document
                .device_selectors
                .insert(key.clone(), selector.clone());
        }
        WorkspaceEdit::RemoveDeviceSelector { key } => {
            document
                .device_selectors
                .remove(key)
                .ok_or_else(|| WorkspaceError::MissingDeviceSelector(key.clone()))?;
        }
    }
    Ok(())
}

fn validate_layout(
    node: &LayoutNode,
    bindings: &BTreeMap<EntityId, TileBinding>,
) -> Result<(), WorkspaceError> {
    match node {
        LayoutNode::Tile { tile_id } => {
            if !bindings.contains_key(tile_id) {
                return Err(WorkspaceError::LayoutReferencesUnboundTile(*tile_id));
            }
        }
        LayoutNode::Tabs { active, children } => {
            if children.is_empty() || *active >= children.len() {
                return Err(WorkspaceError::InvalidTabStack);
            }
            for child in children {
                validate_layout(child, bindings)?;
            }
        }
        LayoutNode::Split {
            ratio_millionths,
            first,
            second,
            ..
        } => {
            if !(1..1_000_000).contains(ratio_millionths) {
                return Err(WorkspaceError::InvalidSplitRatio);
            }
            validate_layout(first, bindings)?;
            validate_layout(second, bindings)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{synthetic, ModuleTypeId};
    use serde_json::json;

    #[test]
    fn typed_batch_is_atomic_when_a_later_edit_fails() {
        let registry = synthetic::registry();
        let document = WorkspaceDocument::default();
        let module_id = EntityId::from_u128(1);
        let batch = WorkspaceEditBatch::new(vec![
            WorkspaceEdit::AddModule {
                instance: ModuleInstance {
                    id: module_id,
                    module_type: ModuleTypeId::new(synthetic::SOURCE).unwrap(),
                    configuration: json!({}),
                },
            },
            WorkspaceEdit::RemoveEdge {
                edge_id: EntityId::from_u128(2),
            },
        ]);
        assert!(matches!(
            document.apply(&batch, &registry),
            Err(WorkspaceError::MissingEdge(_))
        ));
        assert!(document.graph.modules.is_empty());
    }

    #[test]
    fn pretty_json_round_trips_and_rejects_unknown_major() {
        let registry = synthetic::registry();
        let document = WorkspaceDocument::default();
        let json = document.to_pretty_json().unwrap();
        assert_eq!(
            WorkspaceDocument::from_json(&json, &registry).unwrap(),
            document
        );

        let unsupported = json.replace("\"major\": 1", "\"major\": 2");
        assert!(matches!(
            WorkspaceDocument::from_json(&unsupported, &registry),
            Err(WorkspaceError::UnsupportedSchemaMajor { received: 2, .. })
        ));
    }
}
