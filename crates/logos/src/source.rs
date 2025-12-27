use async_trait::async_trait;
use talisman_core::{Source, Signal};
use tokio::io::{AsyncBufReadExt, BufReader, Stdin};

pub struct LogosSource {
    reader: BufReader<Stdin>,
}

impl LogosSource {
    pub fn new() -> Self {
        Self { reader: BufReader::new(tokio::io::stdin()) }
    }
}

#[async_trait]
impl Source for LogosSource {
    fn name(&self) -> &str { "logos_stdin" }
    
    async fn poll(&mut self) -> Option<Signal> {
        let mut line = String::new();
        match self.reader.read_line(&mut line).await {
            Ok(0) => None, // EOF
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    // Skip empty lines? Or emit empty text?
                    // Let's recurse (poll again) or return empty text. 
                    // Returning empty text is safer.
                     Some(Signal::Text(trimmed.to_string()))
                } else {
                    Some(Signal::Text(trimmed.to_string()))
                }
            },
            Err(e) => {
                eprintln!("Logos Stdin Error: {}", e);
                None
            },
        }
    }
}
