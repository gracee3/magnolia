use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct AudioInputSettings {
    devices: Mutex<Vec<String>>,
    selected: Mutex<String>,
    pending: AtomicBool,
}

impl AudioInputSettings {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            devices: Mutex::new(Vec::new()),
            selected: Mutex::new("Default".to_string()),
            pending: AtomicBool::new(false),
        })
    }

    pub fn set_devices(&self, devices: Vec<String>) {
        if let Ok(mut list) = self.devices.lock() {
            *list = devices;
        }
    }

    pub fn devices(&self) -> Vec<String> {
        self.devices.lock().map(|d| d.clone()).unwrap_or_default()
    }

    pub fn set_selected(&self, device: String) {
        if let Ok(mut sel) = self.selected.lock() {
            *sel = device;
        }
        self.pending.store(true, Ordering::Relaxed);
    }

    pub fn selected(&self) -> String {
        self.selected.lock().map(|s| s.clone()).unwrap_or_else(|_| "Default".to_string())
    }

    pub fn take_pending(&self) -> bool {
        self.pending.swap(false, Ordering::Relaxed)
    }
}
