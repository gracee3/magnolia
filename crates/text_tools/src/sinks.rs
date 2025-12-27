use async_trait::async_trait;
use talisman_core::{Sink, Signal, Result};
use regex::Regex;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

// --- Word Count Sink ---
pub struct WordCountSink {
    tx: Option<Arc<Mutex<Sender<Signal>>>>,
}

impl WordCountSink {
    pub fn new(tx: Option<Sender<Signal>>) -> Self { 
        Self { 
            tx: tx.map(|t| Arc::new(Mutex::new(t))) 
        } 
    }
}

#[async_trait]
impl Sink for WordCountSink {
    fn name(&self) -> &str { "word_count" }

    async fn consume(&self, signal: Signal) -> Result<()> {
        if let Signal::Text(text) = signal {
            let count = text.split_whitespace().count();
            // println!("\x1b[33m[WORD_COUNT]\x1b[0m Count: {} | Text: '{}'", count, text);
            log::debug!("[WORD_COUNT] Count: {} | Text: '{}'", count, text);
            
            if let Some(tx) = &self.tx {
                // Emit computed signal back
                let _ = tx.lock().unwrap().send(Signal::Computed {
                    source: "word_count".to_string(),
                    content: format!("{}", count),
                });
            }
        }
        Ok(())
    }
}

// --- Devowelizer Sink ---
pub struct DevowelizerSink {
    re: Regex,
    tx: Option<Arc<Mutex<Sender<Signal>>>>,
}

impl DevowelizerSink {
    pub fn new(tx: Option<Sender<Signal>>) -> Self {
        Self {
            re: Regex::new(r"(?i)[aeiou]").expect("Invalid regex"),
            tx: tx.map(|t| Arc::new(Mutex::new(t))),
        }
    }
}

#[async_trait]
impl Sink for DevowelizerSink {
    fn name(&self) -> &str { "devowelizer" }

    async fn consume(&self, signal: Signal) -> Result<()> {
        if let Signal::Text(text) = signal {
            let devoweled = self.re.replace_all(&text, "").to_string().to_uppercase();
            // println!("\x1b[35m[DEVOWELIZER]\x1b[0m '{}'", devoweled);
            log::debug!("[DEVOWELIZER] '{}'", devoweled);
            
            if let Some(tx) = &self.tx {
                let _ = tx.lock().unwrap().send(Signal::Computed {
                    source: "devowelizer".to_string(),
                    content: devoweled,
                });
            }
        }
        Ok(())
    }
}
