use async_trait::async_trait;
use regex::Regex;
use std::sync::{Arc, Mutex};
use talisman_core::{DataType, ModuleSchema, Port, PortDirection, Result, Signal, Sink};

// --- Word Count Sink ---
pub struct WordCountSink {
    enabled: bool,
    last_count: Arc<Mutex<usize>>,
}

impl WordCountSink {
    /// Create a new WordCountSink
    ///
    /// Note: The tx parameter is no longer needed - signals are returned from consume()
    pub fn new(_tx: Option<std::sync::mpsc::Sender<Signal>>) -> Self {
        Self {
            enabled: true,
            last_count: Arc::new(Mutex::new(0)),
        }
    }
}

#[async_trait]
impl Sink for WordCountSink {
    fn name(&self) -> &str {
        "word_count"
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            id: "word_count".to_string(),
            name: "Word Counter".to_string(),
            description: "Counts words in text input and emits the count".to_string(),
            ports: vec![
                Port {
                    id: "text_in".to_string(),
                    label: "Text Input".to_string(),
                    data_type: DataType::Text,
                    direction: PortDirection::Input,
                },
                Port {
                    id: "count_out".to_string(),
                    label: "Word Count".to_string(),
                    data_type: DataType::Numeric,
                    direction: PortDirection::Output,
                },
            ],
            settings_schema: None,
        }
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn render_output(&self) -> Option<String> {
        let count = *self.last_count.lock().unwrap();
        Some(count.to_string())
    }

    async fn consume(&self, signal: Signal) -> Result<Option<Signal>> {
        if !self.enabled {
            return Ok(None);
        }

        if let Signal::Text(text) = signal {
            let count = text.split_whitespace().count();
            *self.last_count.lock().unwrap() = count;
            log::debug!("[WORD_COUNT] Count: {} | Text: '{}'", count, text);

            // Return the computed signal directly instead of using a channel
            return Ok(Some(Signal::Computed {
                source: "word_count".to_string(),
                content: count.to_string(),
            }));
        }
        Ok(None)
    }
}

// --- Devowelizer Sink ---
pub struct DevowelizerSink {
    re: Regex,
    enabled: bool,
    last_output: Arc<Mutex<String>>,
}

impl DevowelizerSink {
    /// Create a new DevowelizerSink
    ///
    /// Note: The tx parameter is no longer needed - signals are returned from consume()
    pub fn new(_tx: Option<std::sync::mpsc::Sender<Signal>>) -> Self {
        Self {
            re: Regex::new(r"(?i)[aeiou]").expect("Invalid regex"),
            enabled: true,
            last_output: Arc::new(Mutex::new(String::new())),
        }
    }
}

#[async_trait]
impl Sink for DevowelizerSink {
    fn name(&self) -> &str {
        "devowelizer"
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            id: "devowelizer".to_string(),
            name: "Devowelizer".to_string(),
            description: "Removes vowels from text and converts to uppercase".to_string(),
            ports: vec![
                Port {
                    id: "text_in".to_string(),
                    label: "Text Input".to_string(),
                    data_type: DataType::Text,
                    direction: PortDirection::Input,
                },
                Port {
                    id: "text_out".to_string(),
                    label: "Devoweled Text".to_string(),
                    data_type: DataType::Text,
                    direction: PortDirection::Output,
                },
            ],
            settings_schema: None,
        }
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn render_output(&self) -> Option<String> {
        let output = self.last_output.lock().unwrap().clone();
        if output.is_empty() {
            None
        } else {
            Some(output)
        }
    }

    async fn consume(&self, signal: Signal) -> Result<Option<Signal>> {
        if !self.enabled {
            return Ok(None);
        }

        if let Signal::Text(text) = signal {
            let devoweled = self.re.replace_all(&text, "").to_string().to_uppercase();
            *self.last_output.lock().unwrap() = devoweled.clone();
            log::debug!("[DEVOWELIZER] '{}'", devoweled);

            // Return the computed signal directly instead of using a channel
            return Ok(Some(Signal::Computed {
                source: "devowelizer".to_string(),
                content: devoweled,
            }));
        }
        Ok(None)
    }
}
