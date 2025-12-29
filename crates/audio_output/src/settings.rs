use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct AudioOutputSettings {
    devices: Mutex<Vec<AudioDeviceEntry>>,
    selected: Mutex<String>,
    pending: AtomicBool,
    last_error: Mutex<Option<String>>,
    active_device: Mutex<Option<String>>,
    sample_rate: AtomicU32,
    channels: AtomicU32,
    is_muted: AtomicBool,
}

#[derive(Clone, Debug)]
pub struct AudioDeviceEntry {
    pub id: String,
    pub name: String,
}

impl AudioOutputSettings {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            devices: Mutex::new(Vec::new()),
            selected: Mutex::new("Default".to_string()),
            pending: AtomicBool::new(false),
            last_error: Mutex::new(None),
            active_device: Mutex::new(None),
            sample_rate: AtomicU32::new(0),
            channels: AtomicU32::new(0),
            is_muted: AtomicBool::new(true),
        })
    }

    pub fn set_devices(&self, devices: Vec<AudioDeviceEntry>) {
        if let Ok(mut list) = self.devices.lock() {
            *list = devices;
        }
    }

    pub fn devices(&self) -> Vec<AudioDeviceEntry> {
        self.devices.lock().map(|d| d.clone()).unwrap_or_default()
    }

    pub fn set_selected(&self, device: String) {
        if let Ok(mut sel) = self.selected.lock() {
            *sel = device;
        }
        self.pending.store(true, Ordering::Relaxed);
    }

    pub fn selected(&self) -> String {
        self.selected
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|_| "Default".to_string())
    }

    pub fn take_pending(&self) -> bool {
        self.pending.swap(false, Ordering::Relaxed)
    }

    pub fn set_last_error(&self, err: Option<String>) {
        if let Ok(mut e) = self.last_error.lock() {
            *e = err;
        }
    }

    pub fn last_error(&self) -> Option<String> {
        self.last_error.lock().ok().and_then(|e| e.clone())
    }

    pub fn set_active_device(&self, name: Option<String>) {
        if let Ok(mut a) = self.active_device.lock() {
            *a = name;
        }
    }

    pub fn active_device(&self) -> Option<String> {
        self.active_device.lock().ok().and_then(|a| a.clone())
    }

    pub fn set_format(&self, sample_rate: u32, channels: u16) {
        self.sample_rate.store(sample_rate, Ordering::Relaxed);
        self.channels.store(channels as u32, Ordering::Relaxed);
    }

    pub fn format(&self) -> Option<(u32, u16)> {
        let sr = self.sample_rate.load(Ordering::Relaxed);
        let ch = self.channels.load(Ordering::Relaxed) as u16;
        if sr == 0 || ch == 0 {
            None
        } else {
            Some((sr, ch))
        }
    }

    pub fn is_muted(&self) -> bool {
        self.is_muted.load(Ordering::Relaxed)
    }

    pub fn set_muted(&self, muted: bool) {
        self.is_muted.store(muted, Ordering::Relaxed);
    }
}
