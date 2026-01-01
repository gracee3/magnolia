use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct GlyphTweak {
    #[serde(default)]
    pub dx: f32,
    #[serde(default)]
    pub dy: f32,
    #[serde(default = "default_scale")]
    pub sx: f32,
    #[serde(default = "default_scale")]
    pub sy: f32,
    #[serde(default)]
    pub rot_deg: f32,
}

impl Default for GlyphTweak {
    fn default() -> Self {
        Self {
            dx: 0.0,
            dy: 0.0,
            sx: 1.0,
            sy: 1.0,
            rot_deg: 0.0,
        }
    }
}

fn default_scale() -> f32 {
    1.0
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct GlyphTweaks {
    #[serde(flatten)]
    pub tweaks: HashMap<String, GlyphTweak>,
}

impl GlyphTweaks {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let content = fs::read_to_string(path)?;
        let tweaks: GlyphTweaks = toml::from_str(&content)?;
        Ok(tweaks)
    }

    pub fn get(&self, name: &str) -> GlyphTweak {
        self.tweaks.get(name).cloned().unwrap_or_default()
    }
}
