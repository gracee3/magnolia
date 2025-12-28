use crate::{DataType, ModuleSchema, Patch, Port, PortDirection};
use std::collections::{HashMap, HashSet};

/// PatchBay manages module connections and validates type compatibility.
/// 
/// This is the central router for the signal graph, ensuring that only
/// compatible ports can be connected.
pub struct PatchBay {
    /// Registered module schemas by ID
    modules: HashMap<String, ModuleSchema>,
    /// Active patches (connections)
    patches: Vec<Patch>,
    /// Modules in pass-thru mode (disabled but still routing signals)
    disabled_modules: HashSet<String>,
    /// Counter for generating patch IDs
    next_patch_id: u64,
}

impl Default for PatchBay {
    fn default() -> Self {
        Self::new()
    }
}

impl PatchBay {
    pub fn new() -> Self {
        Self {
            modules: HashMap::new(),
            patches: Vec::new(),
            disabled_modules: HashSet::new(),
            next_patch_id: 1,
        }
    }
    
    /// Register a module's schema with the patch bay
    pub fn register_module(&mut self, schema: ModuleSchema) {
        log::debug!("PatchBay: Registered module '{}'", schema.id);
        self.modules.insert(schema.id.clone(), schema);
    }
    
    /// Unregister a module and remove all its connections
    pub fn unregister_module(&mut self, module_id: &str) {
        self.modules.remove(module_id);
        self.patches.retain(|p| p.source_module != module_id && p.sink_module != module_id);
    }
    
    /// Get a module schema by ID
    pub fn get_module(&self, module_id: &str) -> Option<&ModuleSchema> {
        self.modules.get(module_id)
    }
    
    /// Get all registered modules
    pub fn get_modules(&self) -> Vec<&ModuleSchema> {
        self.modules.values().collect()
    }
    
    /// Check if two ports can be connected based on type compatibility
    pub fn can_connect(&self, source_port: &Port, sink_port: &Port) -> bool {
        // Direction check: source must be Output, sink must be Input
        if source_port.direction != PortDirection::Output {
            return false;
        }
        if sink_port.direction != PortDirection::Input {
            return false;
        }
        
        Self::types_compatible(&source_port.data_type, &sink_port.data_type)
    }
    
    /// Check if two data types are compatible for connection
    pub fn types_compatible(source: &DataType, sink: &DataType) -> bool {
        // Exact match
        if source == sink {
            return true;
        }
        // Any type accepts everything
        if *sink == DataType::Any {
            return true;
        }
        // Any type can connect to anything
        if *source == DataType::Any {
            return true;
        }
        false
    }
    
    /// Create a new connection between modules
    pub fn connect(
        &mut self,
        source_module: &str,
        source_port: &str,
        sink_module: &str,
        sink_port: &str,
    ) -> Result<String, PatchBayError> {
        // Validate modules exist
        let source_schema = self.modules.get(source_module)
            .ok_or_else(|| PatchBayError::ModuleNotFound(source_module.to_string()))?;
        let sink_schema = self.modules.get(sink_module)
            .ok_or_else(|| PatchBayError::ModuleNotFound(sink_module.to_string()))?;
        
        // Find ports
        let src_port = source_schema.ports.iter()
            .find(|p| p.id == source_port)
            .ok_or_else(|| PatchBayError::PortNotFound(source_module.to_string(), source_port.to_string()))?;
        let snk_port = sink_schema.ports.iter()
            .find(|p| p.id == sink_port)
            .ok_or_else(|| PatchBayError::PortNotFound(sink_module.to_string(), sink_port.to_string()))?;
        
        // Validate connection
        if !self.can_connect(src_port, snk_port) {
            return Err(PatchBayError::IncompatibleTypes {
                source_type: src_port.data_type.clone(),
                sink_type: snk_port.data_type.clone(),
            });
        }
        
        // Check for duplicate connection
        let already_exists = self.patches.iter().any(|p| {
            p.source_module == source_module
                && p.source_port == source_port
                && p.sink_module == sink_module
                && p.sink_port == sink_port
        });
        if already_exists {
            return Err(PatchBayError::DuplicateConnection);
        }
        
        // Create patch
        let patch_id = format!("patch_{}", self.next_patch_id);
        self.next_patch_id += 1;
        
        let patch = Patch {
            id: patch_id.clone(),
            source_module: source_module.to_string(),
            source_port: source_port.to_string(),
            sink_module: sink_module.to_string(),
            sink_port: sink_port.to_string(),
        };
        
        log::info!(
            "PatchBay: Connected {}:{} -> {}:{}",
            source_module, source_port, sink_module, sink_port
        );
        
        self.patches.push(patch);
        Ok(patch_id)
    }
    
    /// Remove a connection by patch ID
    pub fn disconnect(&mut self, patch_id: &str) -> bool {
        let len_before = self.patches.len();
        self.patches.retain(|p| p.id != patch_id);
        let removed = self.patches.len() < len_before;
        if removed {
            log::info!("PatchBay: Disconnected patch {}", patch_id);
        }
        removed
    }
    
    /// Get all active patches
    pub fn get_patches(&self) -> &[Patch] {
        &self.patches
    }
    
    /// Get patches where this module is the source
    pub fn get_outgoing_patches(&self, module_id: &str) -> Vec<&Patch> {
        self.patches.iter()
            .filter(|p| p.source_module == module_id)
            .collect()
    }
    
    /// Get patches where this module is the sink
    pub fn get_incoming_patches(&self, module_id: &str) -> Vec<&Patch> {
        self.patches.iter()
            .filter(|p| p.sink_module == module_id)
            .collect()
    }
    
    /// Check if a module is disabled (pass-thru mode)
    pub fn is_module_disabled(&self, module_id: &str) -> bool {
        self.disabled_modules.contains(module_id)
    }
    
    /// Disable a module (signals pass through without processing)
    pub fn disable_module(&mut self, module_id: &str) {
        self.disabled_modules.insert(module_id.to_string());
        log::info!("PatchBay: Module '{}' disabled (pass-thru mode)", module_id);
    }
    
    /// Enable a module (normal processing)
    pub fn enable_module(&mut self, module_id: &str) {
        self.disabled_modules.remove(module_id);
        log::info!("PatchBay: Module '{}' enabled", module_id);
    }
    
    /// Get all disabled modules
    pub fn get_disabled_modules(&self) -> &HashSet<String> {
        &self.disabled_modules
    }
    
    /// Check if two modules have any compatible port pairs for connection
    /// Returns all possible (source_port, sink_port) pairs
    pub fn get_compatible_ports(
        &self,
        source_module: &str,
        sink_module: &str,
    ) -> Vec<(String, String)> {
        let mut compatible = Vec::new();
        
        let source_schema = match self.modules.get(source_module) {
            Some(s) => s,
            None => return compatible,
        };
        let sink_schema = match self.modules.get(sink_module) {
            Some(s) => s,
            None => return compatible,
        };
        
        for src_port in &source_schema.ports {
            if src_port.direction != PortDirection::Output {
                continue;
            }
            for snk_port in &sink_schema.ports {
                if snk_port.direction != PortDirection::Input {
                    continue;
                }
                if Self::types_compatible(&src_port.data_type, &snk_port.data_type) {
                    compatible.push((src_port.id.clone(), snk_port.id.clone()));
                }
            }
        }
        
        compatible
    }
    
    /// Check if two modules can be connected (have any compatible port pairs)
    pub fn can_connect_modules(&self, source_module: &str, sink_module: &str) -> bool {
        !self.get_compatible_ports(source_module, sink_module).is_empty()
    }
}


/// Errors that can occur during patch bay operations
#[derive(Debug, Clone)]
pub enum PatchBayError {
    ModuleNotFound(String),
    PortNotFound(String, String),
    IncompatibleTypes {
        source_type: DataType,
        sink_type: DataType,
    },
    DuplicateConnection,
}

impl std::fmt::Display for PatchBayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ModuleNotFound(id) => write!(f, "Module not found: {}", id),
            Self::PortNotFound(module, port) => write!(f, "Port not found: {}:{}", module, port),
            Self::IncompatibleTypes { source_type, sink_type } => {
                write!(f, "Incompatible types: {:?} cannot connect to {:?}", source_type, sink_type)
            }
            Self::DuplicateConnection => write!(f, "Connection already exists"),
        }
    }
}

impl std::error::Error for PatchBayError {}

#[cfg(test)]
mod tests {
    use super::*;
    
    fn make_port(id: &str, data_type: DataType, direction: PortDirection) -> Port {
        Port {
            id: id.to_string(),
            label: id.to_string(),
            data_type,
            direction,
        }
    }
    
    fn make_schema(id: &str, ports: Vec<Port>) -> ModuleSchema {
        ModuleSchema {
            id: id.to_string(),
            name: id.to_string(),
            description: "Test module".to_string(),
            ports,
            settings_schema: None,
        }
    }
    
    #[test]
    fn test_type_compatibility_exact_match() {
        assert!(PatchBay::types_compatible(&DataType::Text, &DataType::Text));
        assert!(PatchBay::types_compatible(&DataType::Audio, &DataType::Audio));
    }
    
    #[test]
    fn test_type_compatibility_any_sink() {
        assert!(PatchBay::types_compatible(&DataType::Text, &DataType::Any));
        assert!(PatchBay::types_compatible(&DataType::Audio, &DataType::Any));
    }
    
    #[test]
    fn test_type_compatibility_any_source() {
        assert!(PatchBay::types_compatible(&DataType::Any, &DataType::Text));
        assert!(PatchBay::types_compatible(&DataType::Any, &DataType::Audio));
    }
    
    #[test]
    fn test_type_incompatibility() {
        assert!(!PatchBay::types_compatible(&DataType::Audio, &DataType::Text));
        assert!(!PatchBay::types_compatible(&DataType::Video, &DataType::Network));
    }
    
    #[test]
    fn test_connection_direction_validation() {
        let output_port = make_port("out", DataType::Text, PortDirection::Output);
        let input_port = make_port("in", DataType::Text, PortDirection::Input);
        let pb = PatchBay::new();
        
        // Valid: Output -> Input
        assert!(pb.can_connect(&output_port, &input_port));
        
        // Invalid: Input -> Output
        assert!(!pb.can_connect(&input_port, &output_port));
        
        // Invalid: Output -> Output
        assert!(!pb.can_connect(&output_port, &output_port));
        
        // Invalid: Input -> Input
        assert!(!pb.can_connect(&input_port, &input_port));
    }
    
    #[test]
    fn test_connect_modules() {
        let mut pb = PatchBay::new();
        
        let source_schema = make_schema("source", vec![
            make_port("text_out", DataType::Text, PortDirection::Output),
        ]);
        let sink_schema = make_schema("sink", vec![
            make_port("text_in", DataType::Text, PortDirection::Input),
        ]);
        
        pb.register_module(source_schema);
        pb.register_module(sink_schema);
        
        let result = pb.connect("source", "text_out", "sink", "text_in");
        assert!(result.is_ok());
        assert_eq!(pb.get_patches().len(), 1);
    }
    
    #[test]
    fn test_connect_incompatible_types() {
        let mut pb = PatchBay::new();
        
        let source_schema = make_schema("source", vec![
            make_port("audio_out", DataType::Audio, PortDirection::Output),
        ]);
        let sink_schema = make_schema("sink", vec![
            make_port("text_in", DataType::Text, PortDirection::Input),
        ]);
        
        pb.register_module(source_schema);
        pb.register_module(sink_schema);
        
        let result = pb.connect("source", "audio_out", "sink", "text_in");
        assert!(matches!(result, Err(PatchBayError::IncompatibleTypes { .. })));
    }
    
    #[test]
    fn test_disconnect() {
        let mut pb = PatchBay::new();
        
        let source_schema = make_schema("source", vec![
            make_port("out", DataType::Text, PortDirection::Output),
        ]);
        let sink_schema = make_schema("sink", vec![
            make_port("in", DataType::Text, PortDirection::Input),
        ]);
        
        pb.register_module(source_schema);
        pb.register_module(sink_schema);
        
        let patch_id = pb.connect("source", "out", "sink", "in").unwrap();
        assert_eq!(pb.get_patches().len(), 1);
        
        assert!(pb.disconnect(&patch_id));
        assert_eq!(pb.get_patches().len(), 0);
    }
}
