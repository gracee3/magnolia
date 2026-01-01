use std::collections::VecDeque;
use std::io::{self, BufRead};
use std::sync::{Arc, Mutex};
use std::thread;
use magnolia_plugin_helper::{
    export_plugin, SignalBuffer, SignalType, SignalValue, MagnoliaPlugin,
};

struct LogosPlugin {
    queue: Arc<Mutex<VecDeque<String>>>,
    enabled: bool,
}

impl Default for LogosPlugin {
    fn default() -> Self {
        let queue = Arc::new(Mutex::new(VecDeque::new()));
        let queue_clone = queue.clone();

        // Spawn stdin reader thread
        thread::spawn(move || {
            let stdin = io::stdin();
            let mut handle = stdin.lock();
            let mut line = String::new();

            while handle.read_line(&mut line).is_ok() {
                if line.trim().is_empty() {
                    line.clear();
                    continue;
                }

                if let Ok(mut q) = queue_clone.lock() {
                    q.push_back(line.trim().to_string());
                }
                line.clear();
            }
        });

        Self {
            queue,
            enabled: true,
        }
    }
}

impl MagnoliaPlugin for LogosPlugin {
    fn name() -> &'static str {
        "logos"
    }
    fn version() -> &'static str {
        "0.1.0"
    }
    fn description() -> &'static str {
        "Stdin Text Source"
    }
    fn author() -> &'static str {
        "Magnolia"
    }

    fn c_id(&self) -> *const std::os::raw::c_char {
        b"logos\0".as_ptr() as *const _
    }
    fn c_name(&self) -> *const std::os::raw::c_char {
        b"Logos\0".as_ptr() as *const _
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }
    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn poll_signal(&mut self, buffer: &mut SignalBuffer) -> bool {
        if !self.enabled {
            return false;
        }

        let msg = {
            let mut q = self.queue.lock().unwrap();
            q.pop_front()
        };

        if let Some(text) = msg {
            // Adapter expects CString for SignalType::Text
            let c_str = std::ffi::CString::new(text).unwrap_or_default();
            let ptr = c_str.into_raw();

            buffer.signal_type = SignalType::Text as u32;
            buffer.value = SignalValue { ptr: ptr as *mut _ };
            buffer.size = 0; // null-terminated, size not strictly needed by adapter but good practice? Adapter ignores size for Text.
            return true;
        }

        false
    }

    fn consume_signal(&mut self, _input: &SignalBuffer) -> Option<SignalBuffer> {
        None
    }
}

export_plugin!(LogosPlugin);
