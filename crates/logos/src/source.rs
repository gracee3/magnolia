use async_trait::async_trait;
use talisman_core::{Source, Signal, ModuleSchema, Port, DataType, PortDirection};
use tokio::io::{AsyncBufReadExt, BufReader, Stdin};

pub struct LogosSource {
    reader: BufReader<Stdin>,
    enabled: bool,
}

impl LogosSource {
    pub fn new() -> Self {
        Self { 
            reader: BufReader::new(tokio::io::stdin()),
            enabled: true,
        }
    }
}

impl Default for LogosSource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Source for LogosSource {
    fn name(&self) -> &str { "logos_stdin" }
    
    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            id: "logos_stdin".to_string(),
            name: "Logos (Stdin)".to_string(),
            description: "Reads text input from standard input".to_string(),
            ports: vec![
                Port {
                    id: "text_out".to_string(),
                    label: "Text Output".to_string(),
                    data_type: DataType::Text,
                    direction: PortDirection::Output,
                },
            ],
            settings_schema: None,
        }
    }
    
    fn is_enabled(&self) -> bool { self.enabled }
    
    fn set_enabled(&mut self, enabled: bool) { self.enabled = enabled; }
    
    async fn poll(&mut self) -> Option<Signal> {
        if !self.enabled {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            return Some(Signal::Pulse);
        }
        
        let mut line = String::new();
        match self.reader.read_line(&mut line).await {
            Ok(0) => None, // EOF
            Ok(_) => {
                let trimmed = line.trim();
                Some(Signal::Text(trimmed.to_string()))
            },
            Err(e) => {
                eprintln!("Logos Stdin Error: {}", e);
                None
            },
        }
    }
}

