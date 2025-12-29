use talisman_plugin_helper::{
    export_plugin, TalismanPlugin, SignalBuffer, SignalType, SignalValue,
};
use talisman_signals::AstrologyData;

pub mod aspects;
pub mod ephemeris;
pub mod vedic;
pub mod western;
use ephemeris::{SwissEphemerisAdapter, EphemerisSettings, GeoLocation};

use std::time::{Duration, Instant};

struct AphroditePlugin {
    adapter: Option<SwissEphemerisAdapter>,
    settings: EphemerisSettings,
    location: Option<GeoLocation>,
    last_poll: Instant,
    interval: Duration,
    enabled: bool,
}

impl Default for AphroditePlugin {
    fn default() -> Self {
        // Init SwissEph
        let adapter = SwissEphemerisAdapter::new(None).ok();
        
        // Default settings
        let settings = EphemerisSettings {
             zodiac_type: "tropical".to_string(),
             ayanamsa: None,
             house_system: "placidus".to_string(),
             include_objects: vec![
                 "sun".to_string(), "moon".to_string(), "mercury".to_string(),
                 "venus".to_string(), "mars".to_string(), "jupiter".to_string(),
                 "saturn".to_string(), "uranus".to_string(), "neptune".to_string(), "pluto".to_string()
             ],
        };
        
        let location = Some(GeoLocation { lat: 51.48, lon: 0.0 }); // Greenwich default
        
        Self {
            adapter,
            settings,
            location,
            last_poll: Instant::now() - Duration::from_secs(60), // Force immediate poll
            interval: Duration::from_secs(10), // Update every 10s
            enabled: true,
        }
    }
}

fn get_sign(lon: f64) -> String {
    let signs = ["Aries", "Taurus", "Gemini", "Cancer", "Leo", "Virgo", "Libra", "Scorpio", "Sagittarius", "Capricorn", "Aquarius", "Pisces"];
    let normalized = if lon < 0.0 { lon + 360.0 } else { lon };
    let idx = ((normalized / 30.0).floor() as usize) % 12;
    signs[idx].to_string()
}

impl TalismanPlugin for AphroditePlugin {
    fn name() -> &'static str { "aphrodite" }
    fn version() -> &'static str { "0.1.0" }
    fn description() -> &'static str { "Astrological Clock" }
    fn author() -> &'static str { "Talisman" }

    fn c_id(&self) -> *const std::os::raw::c_char {
        b"aphrodite\0".as_ptr() as *const _
    }
    fn c_name(&self) -> *const std::os::raw::c_char {
        b"Aphrodite\0".as_ptr() as *const _
    }

    fn is_enabled(&self) -> bool { self.enabled }
    fn set_enabled(&mut self, enabled: bool) { self.enabled = enabled; }

    fn poll_signal(&mut self, buffer: &mut SignalBuffer) -> bool {
        if !self.enabled { return false; }
        if self.last_poll.elapsed() < self.interval { return false; }
        
        self.last_poll = Instant::now();
        
        if let Some(adapter) = &mut self.adapter {
            let now_time = chrono::Utc::now();
            if let Ok(pos) = adapter.calc_positions(now_time, self.location.clone(), &self.settings) {
                 let sun_lon = pos.planets.get("sun").map(|p| p.lon).unwrap_or(0.0);
                 let moon_lon = pos.planets.get("moon").map(|p| p.lon).unwrap_or(0.0);
                 let asc_lon = pos.houses.as_ref().and_then(|h| h.angles.get("asc")).cloned().unwrap_or(0.0);
                 
                 let planetary_positions: Vec<(String, f64)> = pos.planets.iter()
                    .map(|(k, v)| (k.clone(), v.lon))
                    .collect();

                 // Construct AstrologyData Object
                 let data = AstrologyData {
                     sun_sign: get_sign(sun_lon),
                     moon_sign: get_sign(moon_lon),
                     rising_sign: get_sign(asc_lon),
                     planetary_positions,
                 };

                 // Serialize to JSON (Host expects serialized data)
                 let json_str = serde_json::to_string(&data).unwrap_or_default();
                 let mut bytes = json_str.into_bytes();
                 let len = bytes.len();
                 let ptr = bytes.as_mut_ptr();
                 std::mem::forget(bytes); // Leak to host
                 
                 buffer.signal_type = SignalType::Astrology as u32;
                 buffer.value = SignalValue { ptr: ptr as *mut _ };
                 buffer.size = len as u64;
                 
                 return true;
            }
        }
        
        false
    }

    fn consume_signal(&mut self, _input: &SignalBuffer) -> Option<SignalBuffer> {
        // Does not consume signals yet
        None
    }
}

export_plugin!(AphroditePlugin);
