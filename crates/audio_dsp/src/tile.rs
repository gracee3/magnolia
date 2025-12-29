use std::sync::{Arc, Mutex};

use nannou::prelude::*;
use talisman_core::{BindableAction, RenderContext, TileRenderer};
use talisman_ui::{draw_text, FontId, TextAlignment};

use crate::AudioDspState;

pub struct AudioDspTile {
    id: String,
    state: Arc<AudioDspState>,
    gain: Mutex<f32>,
    lowpass_hz: Mutex<f32>,
    lowpass_enabled: Mutex<bool>,
    is_muted: Mutex<bool>,
}

impl AudioDspTile {
    pub fn new(id: &str, state: Arc<AudioDspState>) -> Self {
        Self {
            id: id.to_string(),
            state,
            gain: Mutex::new(1.0),
            lowpass_hz: Mutex::new(2000.0),
            lowpass_enabled: Mutex::new(false),
            is_muted: Mutex::new(true),
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
        let muted = self.is_muted.lock().map(|v| *v).unwrap_or(true);

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

        if muted {
            draw_text(
                draw,
                FontId::PlexSansBold,
                "MUTE",
                pt2(rect.right() - 25.0, rect.top() - 18.0),
                10.0,
                srgba(1.0, 0.2, 0.2, 1.0),
                TextAlignment::Right,
            );
        }
    }

    fn render_controls(&self, draw: &Draw, rect: Rect, ctx: &RenderContext) -> bool {
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .color(srgba(0.01, 0.01, 0.02, 1.0));

        draw_text(
            draw,
            FontId::PlexSansBold,
            "AUDIO DSP SETTINGS",
            pt2(rect.x(), rect.top() - 40.0),
            20.0,
            srgba(0.0, 1.0, 1.0, 1.0),
            TextAlignment::Center,
        );

        let muted = self.is_muted.lock().map(|v| *v).unwrap_or(true);
        let gain = self.gain.lock().map(|v| *v).unwrap_or(1.0);
        let lowpass = self.lowpass_enabled.lock().map(|v| *v).unwrap_or(false);
        let cutoff = self.lowpass_hz.lock().map(|v| *v).unwrap_or(2000.0);

        let mut y = rect.top() - 100.0;
        let spacing = 30.0;

        // Mute Row
        let mute_color = if muted { srgba(1.0, 0.3, 0.3, 1.0) } else { srgba(0.5, 0.5, 0.5, 1.0) };
        draw_text(draw, FontId::PlexSansBold, "MUTE [M]", pt2(rect.left() + 100.0, y), 14.0, mute_color, TextAlignment::Left);
        draw_text(draw, FontId::PlexSansBold, if muted { "MUTED" } else { "ACTIVE" }, pt2(rect.right() - 100.0, y), 14.0, mute_color, TextAlignment::Right);
        y -= spacing * 1.5;

        // Settings
        draw_text(draw, FontId::PlexSansRegular, &format!("Gain: {:.2}", gain), pt2(rect.left() + 100.0, y), 14.0, srgba(0.7, 0.7, 0.7, 1.0), TextAlignment::Left);
        y -= spacing;
        draw_text(draw, FontId::PlexSansRegular, &format!("Lowpass: {}", if lowpass { "Enabled" } else { "Disabled" }), pt2(rect.left() + 100.0, y), 14.0, srgba(0.7, 0.7, 0.7, 1.0), TextAlignment::Left);
        y -= spacing;
        draw_text(draw, FontId::PlexSansRegular, &format!("Cutoff: {:.0} Hz", cutoff), pt2(rect.left() + 100.0, y), 14.0, srgba(0.7, 0.7, 0.7, 1.0), TextAlignment::Left);

        // Preview box
        let preview_rect = Rect::from_x_y_w_h(rect.x(), rect.bottom() + 100.0, rect.w() * 0.6, 150.0);
        self.render_monitor(draw, preview_rect, ctx);

        false
    }

    fn bindable_actions(&self) -> Vec<BindableAction> {
        vec![BindableAction::new("mute", "Toggle Mute", true)]
    }

    fn execute_action(&mut self, action: &str) -> bool {
        match action {
            "mute" => {
                let mut muted = self.is_muted.lock().unwrap();
                *muted = !*muted;
                self.state.set_muted(*muted);
                true
            }
            _ => false,
        }
    }

    fn handle_key(&mut self, key: Key, _ctrl: bool, _shift: bool) -> bool {
        if key == Key::M {
            let mut muted = self.is_muted.lock().unwrap();
            *muted = !*muted;
            self.state.set_muted(*muted);
            return true;
        }
        false
    }

    fn settings_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "gain": { "type": "number", "default": 1.0, "minimum": 0.0, "maximum": 4.0 },
                "lowpass_enabled": { "type": "boolean", "default": false },
                "lowpass_hz": { "type": "number", "default": 2000.0, "minimum": 80.0, "maximum": 8000.0 },
                "is_muted": { "type": "boolean", "default": true }
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
        if let Some(muted) = settings.get("is_muted").and_then(|v| v.as_bool()) {
            if let Ok(mut current) = self.is_muted.lock() {
                *current = muted;
            }
            self.state.set_muted(muted);
        }
    }

    fn get_settings(&self) -> serde_json::Value {
        let gain = self.gain.lock().map(|v| *v).unwrap_or(1.0);
        let lowpass_enabled = self.lowpass_enabled.lock().map(|v| *v).unwrap_or(false);
        let lowpass_hz = self.lowpass_hz.lock().map(|v| *v).unwrap_or(2000.0);
        let is_muted = self.is_muted.lock().map(|v| *v).unwrap_or(true);
        serde_json::json!({
            "gain": gain,
            "lowpass_enabled": lowpass_enabled,
            "lowpass_hz": lowpass_hz,
            "is_muted": is_muted,
        })
    }
}
