use async_trait::async_trait;
use talisman_core::{Source, Signal};
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

        Self { adapter, settings, location, interval: Duration::from_secs(interval_secs), last_poll: None }
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
    
    async fn poll(&mut self) -> Option<Signal> {
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
                // Return Pulse as error/keepalive? Or just None to stop? 
                // Let's return Pulse for now to indicate aliveness but failure
                // Or better, just log and loop next time. But poll returns one item.
                // We'll return Pulse.
                Some(Signal::Pulse)
            }
        }
    }
}
