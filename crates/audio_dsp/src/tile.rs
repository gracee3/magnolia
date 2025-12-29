use std::sync::{Arc, Mutex};

use nannou::prelude::*;
use talisman_core::{RenderContext, TileRenderer};
use talisman_ui::{draw_text, FontId, TextAlignment};

use crate::AudioDspState;

pub struct AudioDspTile {
    id: String,
    state: Arc<AudioDspState>,
    gain: Mutex<f32>,
    lowpass_hz: Mutex<f32>,
    lowpass_enabled: Mutex<bool>,
}

impl AudioDspTile {
    pub fn new(id: &str, state: Arc<AudioDspState>) -> Self {
        Self {
            id: id.to_string(),
            state,
            gain: Mutex::new(1.0),
            lowpass_hz: Mutex::new(2000.0),
            lowpass_enabled: Mutex::new(false),
        }
    }
}

impl TileRenderer for AudioDspTile {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        "Audio DSP"
    }
    fn update(&mut self) {}

    fn render_monitor(&self, draw: &Draw, rect: Rect, _ctx: &RenderContext) {
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .color(srgba(0.03, 0.03, 0.06, 0.95));

        let gain = self.gain.lock().map(|v| *v).unwrap_or(1.0);
        let lowpass = self.lowpass_enabled.lock().map(|v| *v).unwrap_or(false);
        let cutoff = self.lowpass_hz.lock().map(|v| *v).unwrap_or(2000.0);

        draw_text(
            draw,
            FontId::PlexSansBold,
            "AUDIO DSP",
            pt2(rect.x(), rect.top() - 18.0),
            12.0,
            srgba(0.6, 0.8, 0.9, 1.0),
            TextAlignment::Center,
        );

        draw_text(
            draw,
            FontId::PlexMonoRegular,
            &format!("Gain: {:.2}", gain),
            pt2(rect.x(), rect.y() + 8.0),
            11.0,
            srgba(0.5, 0.7, 0.9, 1.0),
            TextAlignment::Center,
        );

        let lp_label = if lowpass { "On" } else { "Off" };
        draw_text(
            draw,
            FontId::PlexMonoRegular,
            &format!("Lowpass: {} @ {:.0} Hz", lp_label, cutoff),
            pt2(rect.x(), rect.y() - 12.0),
            11.0,
            srgba(0.5, 0.7, 0.9, 1.0),
            TextAlignment::Center,
        );
    }

    fn render_controls(&self, draw: &Draw, rect: Rect, ctx: &RenderContext) -> bool {
        self.render_monitor(draw, rect, ctx);
        false
    }

    fn settings_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "gain": { "type": "number", "default": 1.0, "minimum": 0.0, "maximum": 4.0 },
                "lowpass_enabled": { "type": "boolean", "default": false },
                "lowpass_hz": { "type": "number", "default": 2000.0, "minimum": 80.0, "maximum": 8000.0 }
            }
        }))
    }

    fn apply_settings(&mut self, settings: &serde_json::Value) {
        if let Some(gain) = settings.get("gain").and_then(|v| v.as_f64()) {
            let gain = gain as f32;
            if let Ok(mut current) = self.gain.lock() {
                *current = gain;
            }
            self.state.set_gain(gain);
        }
        if let Some(enabled) = settings.get("lowpass_enabled").and_then(|v| v.as_bool()) {
            if let Ok(mut current) = self.lowpass_enabled.lock() {
                *current = enabled;
            }
            self.state.set_lowpass_enabled(enabled);
        }
        if let Some(hz) = settings.get("lowpass_hz").and_then(|v| v.as_f64()) {
            let hz = hz as f32;
            if let Ok(mut current) = self.lowpass_hz.lock() {
                *current = hz;
            }
            self.state.set_lowpass_hz(hz);
        }
    }

    fn get_settings(&self) -> serde_json::Value {
        let gain = self.gain.lock().map(|v| *v).unwrap_or(1.0);
        let lowpass_enabled = self.lowpass_enabled.lock().map(|v| *v).unwrap_or(false);
        let lowpass_hz = self.lowpass_hz.lock().map(|v| *v).unwrap_or(2000.0);
        serde_json::json!({
            "gain": gain,
            "lowpass_enabled": lowpass_enabled,
            "lowpass_hz": lowpass_hz,
        })
    }
}
