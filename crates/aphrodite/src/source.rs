use async_trait::async_trait;
use magnolia_core::{Source, Signal, ModuleSchema, Port, DataType, PortDirection};
use crate::ephemeris::{SwissEphemerisAdapter, EphemerisSettings, GeoLocation};
use std::time::Duration;
use tokio::time::{sleep, Instant};
use chrono::Utc;

pub struct AphroditeSource {
    adapter: SwissEphemerisAdapter,
    settings: EphemerisSettings,
    location: Option<GeoLocation>,
    interval: Duration,
    last_poll: Option<Instant>,
    enabled: bool,
}

impl AphroditeSource {
    pub fn new(interval_secs: u64) -> Self {
        let adapter = SwissEphemerisAdapter::new(None).expect("Failed to init SwissEph");
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
        // Default location (should come from config later)
        // Defaulting to Greenwich for now if no config
        let location = Some(GeoLocation { lat: 51.48, lon: 0.0 });

        Self { 
            adapter, 
            settings, 
            location, 
            interval: Duration::from_secs(interval_secs), 
            last_poll: None,
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

#[async_trait]
impl Source for AphroditeSource {
    fn name(&self) -> &str { "aphrodite" }
    
    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            id: "aphrodite".to_string(),
            name: "Aphrodite (Astrology)".to_string(),
            description: "Provides real-time astrological data via Swiss Ephemeris".to_string(),
            ports: vec![
                Port {
                    id: "astro_out".to_string(),
                    label: "Astrology Data".to_string(),
                    data_type: DataType::Astrology,
                    direction: PortDirection::Output,
                },
            ],
            settings_schema: None, // TODO: Location/timezone settings
        }
    }
    
    fn is_enabled(&self) -> bool { self.enabled }
    
    fn set_enabled(&mut self, enabled: bool) { self.enabled = enabled; }
    
    async fn poll(&mut self) -> Option<Signal> {
        if !self.enabled {
            sleep(self.interval).await;
            return Some(Signal::Pulse);
        }
        
        // Simple throttling
        if let Some(last) = self.last_poll {
            let elapsed = last.elapsed();
            if elapsed < self.interval {
                 sleep(self.interval - elapsed).await;
            }
        }
        self.last_poll = Some(Instant::now());

        let now_time = Utc::now();
        match self.adapter.calc_positions(now_time, self.location.clone(), &self.settings) {
            Ok(pos) => {
                 let sun_lon = pos.planets.get("sun").map(|p| p.lon).unwrap_or(0.0);
                 let moon_lon = pos.planets.get("moon").map(|p| p.lon).unwrap_or(0.0);
                 let asc_lon = pos.houses.as_ref().and_then(|h| h.angles.get("asc")).cloned().unwrap_or(0.0);
                 
                 let planetary_positions = pos.planets.iter()
                    .map(|(k, v)| (k.clone(), v.lon))
                    .collect();

                 Some(Signal::Astrology {
                     sun_sign: get_sign(sun_lon),
                     moon_sign: get_sign(moon_lon),
                     rising_sign: get_sign(asc_lon),
                     planetary_positions
                 })
            },
            Err(e) => {
                eprintln!("Aphrodite error: {}", e);
                Some(Signal::Pulse)
            }
        }
    }
}

