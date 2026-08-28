use crate::{ControlId, EntityId, ModuleTypeId, PortId, StreamTypeId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaVersion {
    pub major: u16,
    pub minor: u16,
}

impl SchemaVersion {
    #[must_use]
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionLane {
    RealTime,
    NearRealTime,
    Asynchronous,
    Storage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortDirection {
    Input,
    Output,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "name",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ClockDomain {
    AudioFrames,
    RuntimeMonotonic,
    External(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryPolicy {
    Exact,
    Latest,
    DropOldest,
    Durable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamDefinition {
    pub type_id: StreamTypeId,
    pub schema_version: SchemaVersion,
    pub clock: ClockDomain,
    #[serde(default)]
    pub format: BTreeMap<String, String>,
    pub delivery: DeliveryPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortDefinition {
    pub id: PortId,
    pub direction: PortDirection,
    pub stream: StreamDefinition,
    #[serde(default)]
    pub allow_multiple: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlKind {
    Toggle,
    Number,
    Choice { options: Vec<String> },
    Text,
    Trigger,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlDefinition {
    pub id: ControlId,
    pub label: String,
    pub kind: ControlKind,
    pub setting_key: String,
    pub default_value: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleDescriptor {
    pub module_type: ModuleTypeId,
    pub version: SchemaVersion,
    pub execution_lane: ExecutionLane,
    pub ports: Vec<PortDefinition>,
    #[serde(default)]
    pub controls: Vec<ControlDefinition>,
    #[serde(default)]
    pub capabilities: BTreeSet<String>,
    #[serde(default)]
    pub configuration_schema: Value,
    #[serde(default)]
    pub introduces_delay: bool,
}

impl ModuleDescriptor {
    #[must_use]
    pub fn port(&self, id: &PortId) -> Option<&PortDefinition> {
        self.ports.iter().find(|port| &port.id == id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleInstance {
    pub id: EntityId,
    pub module_type: ModuleTypeId,
    #[serde(default)]
    pub configuration: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortRef {
    pub module_id: EntityId,
    pub port_id: PortId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edge {
    pub id: EntityId,
    pub from: PortRef,
    pub to: PortRef,
    /// Required and non-zero for a cross-lane edge.
    pub capacity: Option<u32>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceGraph {
    #[serde(default)]
    pub modules: BTreeMap<EntityId, ModuleInstance>,
    #[serde(default)]
    pub edges: BTreeMap<EntityId, Edge>,
}

#[derive(Debug, Default, Clone)]
pub struct DescriptorRegistry {
    descriptors: BTreeMap<ModuleTypeId, ModuleDescriptor>,
}

impl DescriptorRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, descriptor: ModuleDescriptor) -> Result<(), DescriptorError> {
        validate_descriptor(&descriptor)?;
        if self.descriptors.contains_key(&descriptor.module_type) {
            return Err(DescriptorError::DuplicateModuleType);
        }
        self.descriptors
            .insert(descriptor.module_type.clone(), descriptor);
        Ok(())
    }

    #[must_use]
    pub fn get(&self, module_type: &ModuleTypeId) -> Option<&ModuleDescriptor> {
        self.descriptors.get(module_type)
    }

    pub fn validate_graph(&self, graph: &WorkspaceGraph) -> Result<(), GraphValidationError> {
        validate_graph(self, graph)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DescriptorError {
    #[error("module type is already registered")]
    DuplicateModuleType,
    #[error("descriptor contains duplicate port {0}")]
    DuplicatePort(PortId),
    #[error("descriptor contains duplicate control {0}")]
    DuplicateControl(ControlId),
    #[error("descriptor contains a blank module, port, stream, or control identifier")]
    BlankIdentifier,
    #[error("descriptor label or setting key must not be blank")]
    BlankControlMetadata,
    #[error("choice control must have at least one unique option")]
    InvalidChoiceOptions,
    #[error("control {0} has a default value incompatible with its kind")]
    InvalidControlDefault(ControlId),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GraphValidationError {
    #[error("module map key does not match instance ID {instance_id}")]
    ModuleKeyMismatch { instance_id: EntityId },
    #[error("edge map key does not match edge ID {edge_id}")]
    EdgeKeyMismatch { edge_id: EntityId },
    #[error("module {module_id} uses unregistered type {module_type}")]
    UnknownModuleType {
        module_id: EntityId,
        module_type: ModuleTypeId,
    },
    #[error("module {module_id} has a blank type identifier")]
    BlankModuleType { module_id: EntityId },
    #[error("edge {edge_id} references missing module {module_id}")]
    MissingModule {
        edge_id: EntityId,
        module_id: EntityId,
    },
    #[error("edge {edge_id} references missing port {port_id} on module {module_id}")]
    MissingPort {
        edge_id: EntityId,
        module_id: EntityId,
        port_id: PortId,
    },
    #[error("edge {edge_id} has an invalid port direction")]
    InvalidDirection { edge_id: EntityId },
    #[error("edge {edge_id} has mismatched stream type or schema")]
    StreamMismatch { edge_id: EntityId },
    #[error("edge {edge_id} crosses incompatible clock domains")]
    ClockMismatch { edge_id: EntityId },
    #[error("edge {edge_id} crosses incompatible stream formats")]
    FormatMismatch { edge_id: EntityId },
    #[error("edge {edge_id} crosses execution lanes without a non-zero capacity")]
    UnboundedCrossLane { edge_id: EntityId },
    #[error("input {module_id}:{port_id} rejects fan-in")]
    InvalidFanIn {
        module_id: EntityId,
        port_id: PortId,
    },
    #[error("graph contains a cycle without an explicit delay module")]
    CycleWithoutDelay,
}

fn validate_descriptor(descriptor: &ModuleDescriptor) -> Result<(), DescriptorError> {
    if descriptor.module_type.is_blank() {
        return Err(DescriptorError::BlankIdentifier);
    }
    let mut ports = BTreeSet::new();
    for port in &descriptor.ports {
        if port.id.is_blank() || port.stream.type_id.is_blank() {
            return Err(DescriptorError::BlankIdentifier);
        }
        if !ports.insert(port.id.clone()) {
            return Err(DescriptorError::DuplicatePort(port.id.clone()));
        }
    }

    let mut controls = BTreeSet::new();
    for control in &descriptor.controls {
        if control.id.is_blank() {
            return Err(DescriptorError::BlankIdentifier);
        }
        if !controls.insert(control.id.clone()) {
            return Err(DescriptorError::DuplicateControl(control.id.clone()));
        }
        if control.label.trim().is_empty() || control.setting_key.trim().is_empty() {
            return Err(DescriptorError::BlankControlMetadata);
        }
        if let ControlKind::Choice { options } = &control.kind {
            let unique: BTreeSet<_> = options.iter().collect();
            if options.is_empty() || unique.len() != options.len() {
                return Err(DescriptorError::InvalidChoiceOptions);
            }
        }
        let valid_default = match &control.kind {
            ControlKind::Toggle => control.default_value.is_boolean(),
            ControlKind::Number => control.default_value.is_number(),
            ControlKind::Choice { options } => control
                .default_value
                .as_str()
                .is_some_and(|default| options.iter().any(|option| option == default)),
            ControlKind::Text => control.default_value.is_string(),
            ControlKind::Trigger => control.default_value.is_null(),
        };
        if !valid_default {
            return Err(DescriptorError::InvalidControlDefault(control.id.clone()));
        }
    }
    Ok(())
}

fn validate_graph(
    registry: &DescriptorRegistry,
    graph: &WorkspaceGraph,
) -> Result<(), GraphValidationError> {
    for (key, module) in &graph.modules {
        if key != &module.id {
            return Err(GraphValidationError::ModuleKeyMismatch {
                instance_id: module.id,
            });
        }
        if module.module_type.is_blank() {
            return Err(GraphValidationError::BlankModuleType {
                module_id: module.id,
            });
        }
        if registry.get(&module.module_type).is_none() {
            return Err(GraphValidationError::UnknownModuleType {
                module_id: module.id,
                module_type: module.module_type.clone(),
            });
        }
    }

    let mut fan_in: BTreeMap<(EntityId, PortId), usize> = BTreeMap::new();
    let mut adjacency: BTreeMap<EntityId, Vec<EntityId>> = BTreeMap::new();
    for (key, edge) in &graph.edges {
        if key != &edge.id {
            return Err(GraphValidationError::EdgeKeyMismatch { edge_id: edge.id });
        }
        let from_instance =
            graph
                .modules
                .get(&edge.from.module_id)
                .ok_or(GraphValidationError::MissingModule {
                    edge_id: edge.id,
                    module_id: edge.from.module_id,
                })?;
        let to_instance =
            graph
                .modules
                .get(&edge.to.module_id)
                .ok_or(GraphValidationError::MissingModule {
                    edge_id: edge.id,
                    module_id: edge.to.module_id,
                })?;
        let from_descriptor = registry
            .get(&from_instance.module_type)
            .expect("checked above");
        let to_descriptor = registry
            .get(&to_instance.module_type)
            .expect("checked above");
        let from_port = from_descriptor.port(&edge.from.port_id).ok_or_else(|| {
            GraphValidationError::MissingPort {
                edge_id: edge.id,
                module_id: edge.from.module_id,
                port_id: edge.from.port_id.clone(),
            }
        })?;
        let to_port = to_descriptor.port(&edge.to.port_id).ok_or_else(|| {
            GraphValidationError::MissingPort {
                edge_id: edge.id,
                module_id: edge.to.module_id,
                port_id: edge.to.port_id.clone(),
            }
        })?;

        if from_port.direction != PortDirection::Output || to_port.direction != PortDirection::Input
        {
            return Err(GraphValidationError::InvalidDirection { edge_id: edge.id });
        }
        if from_port.stream.type_id != to_port.stream.type_id
            || from_port.stream.schema_version != to_port.stream.schema_version
        {
            return Err(GraphValidationError::StreamMismatch { edge_id: edge.id });
        }
        if from_port.stream.clock != to_port.stream.clock {
            return Err(GraphValidationError::ClockMismatch { edge_id: edge.id });
        }
        if from_port.stream.format != to_port.stream.format {
            return Err(GraphValidationError::FormatMismatch { edge_id: edge.id });
        }
        if from_descriptor.execution_lane != to_descriptor.execution_lane
            && !matches!(edge.capacity, Some(capacity) if capacity > 0)
        {
            return Err(GraphValidationError::UnboundedCrossLane { edge_id: edge.id });
        }

        let count = fan_in
            .entry((edge.to.module_id, edge.to.port_id.clone()))
            .or_default();
        *count += 1;
        if *count > 1 && !to_port.allow_multiple {
            return Err(GraphValidationError::InvalidFanIn {
                module_id: edge.to.module_id,
                port_id: edge.to.port_id.clone(),
            });
        }

        if !from_descriptor.introduces_delay {
            adjacency
                .entry(edge.from.module_id)
                .or_default()
                .push(edge.to.module_id);
        }
    }

    if contains_cycle(graph.modules.keys().copied(), &adjacency) {
        return Err(GraphValidationError::CycleWithoutDelay);
    }
    Ok(())
}

fn contains_cycle(
    nodes: impl Iterator<Item = EntityId>,
    adjacency: &BTreeMap<EntityId, Vec<EntityId>>,
) -> bool {
    fn visit(
        node: EntityId,
        adjacency: &BTreeMap<EntityId, Vec<EntityId>>,
        visiting: &mut BTreeSet<EntityId>,
        visited: &mut BTreeSet<EntityId>,
    ) -> bool {
        if visiting.contains(&node) {
            return true;
        }
        if visited.contains(&node) {
            return false;
        }
        visiting.insert(node);
        if adjacency.get(&node).is_some_and(|next| {
            next.iter()
                .copied()
                .any(|child| visit(child, adjacency, visiting, visited))
        }) {
            return true;
        }
        visiting.remove(&node);
        visited.insert(node);
        false
    }

    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    nodes
        .into_iter()
        .any(|node| visit(node, adjacency, &mut visiting, &mut visited))
}

/// Synthetic descriptors used only by the portable foundation and shell proof.
pub mod synthetic {
    use super::*;
    use serde_json::json;

    pub const SOURCE: &str = "synthetic.source";
    pub const PROCESSOR: &str = "synthetic.processor";
    pub const SINK: &str = "synthetic.sink";
    pub const DELAY: &str = "synthetic.delay";

    #[must_use]
    pub fn registry() -> DescriptorRegistry {
        let mut registry = DescriptorRegistry::new();
        for descriptor in [
            descriptor(SOURCE, ExecutionLane::Asynchronous, false, false, true),
            descriptor(PROCESSOR, ExecutionLane::Asynchronous, true, false, true),
            descriptor(SINK, ExecutionLane::Storage, true, false, false),
            descriptor(DELAY, ExecutionLane::Asynchronous, true, true, true),
        ] {
            registry
                .register(descriptor)
                .expect("valid synthetic descriptor");
        }
        registry
    }

    fn descriptor(
        name: &str,
        lane: ExecutionLane,
        input: bool,
        delay: bool,
        output: bool,
    ) -> ModuleDescriptor {
        let stream = StreamDefinition {
            type_id: StreamTypeId::new("magnolia.synthetic.number").unwrap(),
            schema_version: SchemaVersion::new(1, 0),
            clock: ClockDomain::RuntimeMonotonic,
            format: BTreeMap::new(),
            delivery: DeliveryPolicy::Exact,
        };
        let mut ports = Vec::new();
        if input {
            ports.push(PortDefinition {
                id: PortId::new("in").unwrap(),
                direction: PortDirection::Input,
                stream: stream.clone(),
                allow_multiple: false,
            });
        }
        if output {
            ports.push(PortDefinition {
                id: PortId::new("out").unwrap(),
                direction: PortDirection::Output,
                stream,
                allow_multiple: false,
            });
        }
        ModuleDescriptor {
            module_type: ModuleTypeId::new(name).unwrap(),
            version: SchemaVersion::new(1, 0),
            execution_lane: lane,
            ports,
            controls: vec![ControlDefinition {
                id: ControlId::new("enabled").unwrap(),
                label: "Enabled".to_owned(),
                kind: ControlKind::Toggle,
                setting_key: "enabled".to_owned(),
                default_value: json!(true),
            }],
            capabilities: BTreeSet::from(["synthetic".to_owned()]),
            configuration_schema: json!({"type": "object"}),
            introduces_delay: delay,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance(id: u128, module_type: &str) -> ModuleInstance {
        ModuleInstance {
            id: EntityId::from_u128(id),
            module_type: ModuleTypeId::new(module_type).unwrap(),
            configuration: Value::Object(Default::default()),
        }
    }

    fn edge(id: u128, from: u128, to: u128, capacity: Option<u32>) -> Edge {
        Edge {
            id: EntityId::from_u128(id),
            from: PortRef {
                module_id: EntityId::from_u128(from),
                port_id: PortId::new("out").unwrap(),
            },
            to: PortRef {
                module_id: EntityId::from_u128(to),
                port_id: PortId::new("in").unwrap(),
            },
            capacity,
        }
    }

    fn source_sink_graph() -> WorkspaceGraph {
        let mut graph = WorkspaceGraph::default();
        graph
            .modules
            .insert(EntityId::from_u128(1), instance(1, synthetic::SOURCE));
        graph
            .modules
            .insert(EntityId::from_u128(2), instance(2, synthetic::SINK));
        graph
            .edges
            .insert(EntityId::from_u128(3), edge(3, 1, 2, Some(4)));
        graph
    }

    fn registry_with_sink_change(change: impl FnOnce(&mut ModuleDescriptor)) -> DescriptorRegistry {
        let base = synthetic::registry();
        let source = base
            .get(&ModuleTypeId::new(synthetic::SOURCE).unwrap())
            .unwrap()
            .clone();
        let mut sink = base
            .get(&ModuleTypeId::new(synthetic::SINK).unwrap())
            .unwrap()
            .clone();
        change(&mut sink);
        let mut registry = DescriptorRegistry::new();
        registry.register(source).unwrap();
        registry.register(sink).unwrap();
        registry
    }

    #[test]
    fn validates_synthetic_cross_lane_graph() {
        let registry = synthetic::registry();
        let mut graph = WorkspaceGraph::default();
        graph
            .modules
            .insert(EntityId::from_u128(1), instance(1, synthetic::SOURCE));
        graph
            .modules
            .insert(EntityId::from_u128(2), instance(2, synthetic::SINK));
        graph
            .edges
            .insert(EntityId::from_u128(3), edge(3, 1, 2, Some(4)));
        assert_eq!(registry.validate_graph(&graph), Ok(()));
    }

    #[test]
    fn rejects_unbounded_cross_lane_edge() {
        let registry = synthetic::registry();
        let mut graph = WorkspaceGraph::default();
        graph
            .modules
            .insert(EntityId::from_u128(1), instance(1, synthetic::SOURCE));
        graph
            .modules
            .insert(EntityId::from_u128(2), instance(2, synthetic::SINK));
        graph
            .edges
            .insert(EntityId::from_u128(3), edge(3, 1, 2, None));
        assert!(matches!(
            registry.validate_graph(&graph),
            Err(GraphValidationError::UnboundedCrossLane { .. })
        ));
    }

    #[test]
    fn rejects_cycle_without_delay_and_accepts_explicit_delay() {
        let registry = synthetic::registry();
        let mut graph = WorkspaceGraph::default();
        graph
            .modules
            .insert(EntityId::from_u128(1), instance(1, synthetic::PROCESSOR));
        graph
            .modules
            .insert(EntityId::from_u128(2), instance(2, synthetic::PROCESSOR));
        graph
            .edges
            .insert(EntityId::from_u128(3), edge(3, 1, 2, None));
        graph
            .edges
            .insert(EntityId::from_u128(4), edge(4, 2, 1, None));
        assert_eq!(
            registry.validate_graph(&graph),
            Err(GraphValidationError::CycleWithoutDelay)
        );

        graph
            .modules
            .get_mut(&EntityId::from_u128(2))
            .unwrap()
            .module_type = ModuleTypeId::new(synthetic::DELAY).unwrap();
        assert_eq!(registry.validate_graph(&graph), Ok(()));
    }

    #[test]
    fn duplicate_registration_does_not_replace_the_original_descriptor() {
        let mut registry = synthetic::registry();
        let original = registry
            .get(&ModuleTypeId::new(synthetic::SOURCE).unwrap())
            .unwrap()
            .clone();
        assert_eq!(
            registry.register(original.clone()),
            Err(DescriptorError::DuplicateModuleType)
        );
        assert_eq!(registry.get(&original.module_type), Some(&original));
    }

    #[test]
    fn rejects_missing_port_and_stream_schema_mismatch() {
        let mut missing_port = source_sink_graph();
        missing_port
            .edges
            .get_mut(&EntityId::from_u128(3))
            .unwrap()
            .to
            .port_id = PortId::new("missing").unwrap();
        assert!(matches!(
            synthetic::registry().validate_graph(&missing_port),
            Err(GraphValidationError::MissingPort { .. })
        ));

        let registry = registry_with_sink_change(|sink| {
            sink.ports[0].stream.schema_version = SchemaVersion::new(2, 0);
        });
        assert!(matches!(
            registry.validate_graph(&source_sink_graph()),
            Err(GraphValidationError::StreamMismatch { .. })
        ));
    }

    #[test]
    fn rejects_clock_and_format_mismatches() {
        let clock_registry = registry_with_sink_change(|sink| {
            sink.ports[0].stream.clock = ClockDomain::AudioFrames;
        });
        assert!(matches!(
            clock_registry.validate_graph(&source_sink_graph()),
            Err(GraphValidationError::ClockMismatch { .. })
        ));

        let format_registry = registry_with_sink_change(|sink| {
            sink.ports[0]
                .stream
                .format
                .insert("encoding".to_owned(), "f32".to_owned());
        });
        assert!(matches!(
            format_registry.validate_graph(&source_sink_graph()),
            Err(GraphValidationError::FormatMismatch { .. })
        ));
    }

    #[test]
    fn rejects_invalid_fan_in() {
        let mut graph = source_sink_graph();
        graph
            .modules
            .insert(EntityId::from_u128(4), instance(4, synthetic::SOURCE));
        graph
            .edges
            .insert(EntityId::from_u128(5), edge(5, 4, 2, Some(4)));
        assert!(matches!(
            synthetic::registry().validate_graph(&graph),
            Err(GraphValidationError::InvalidFanIn { .. })
        ));
    }

    #[test]
    fn rejects_reversed_port_directions() {
        let mut graph = WorkspaceGraph::default();
        graph
            .modules
            .insert(EntityId::from_u128(1), instance(1, synthetic::PROCESSOR));
        graph
            .modules
            .insert(EntityId::from_u128(2), instance(2, synthetic::PROCESSOR));
        graph.edges.insert(
            EntityId::from_u128(3),
            Edge {
                id: EntityId::from_u128(3),
                from: PortRef {
                    module_id: EntityId::from_u128(1),
                    port_id: PortId::new("in").unwrap(),
                },
                to: PortRef {
                    module_id: EntityId::from_u128(2),
                    port_id: PortId::new("out").unwrap(),
                },
                capacity: None,
            },
        );
        assert!(matches!(
            synthetic::registry().validate_graph(&graph),
            Err(GraphValidationError::InvalidDirection { .. })
        ));
    }

    #[test]
    fn deserialized_blank_descriptor_identifier_is_rejected() {
        let base = synthetic::registry();
        let descriptor = base
            .get(&ModuleTypeId::new(synthetic::SOURCE).unwrap())
            .unwrap();
        let mut value = serde_json::to_value(descriptor).unwrap();
        value["module_type"] = Value::String(String::new());
        let blank: ModuleDescriptor = serde_json::from_value(value).unwrap();
        assert_eq!(
            DescriptorRegistry::new().register(blank),
            Err(DescriptorError::BlankIdentifier)
        );
    }
}
