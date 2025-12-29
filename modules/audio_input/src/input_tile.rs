use std::sync::{Arc, Mutex};

use nannou::prelude::*;
use talisman_core::{TileRenderer, RenderContext, BindableAction};
use talisman_ui::{FontId, draw_text, TextAlignment};

use crate::AudioInputSettings;

pub struct AudioInputTile {
    id: String,
    settings: Arc<AudioInputSettings>,
    selected: Mutex<String>,
}

impl AudioInputTile {
    pub fn new(id: &str, settings: Arc<AudioInputSettings>) -> Self {
        let selected = settings.selected();
        Self {
            id: id.to_string(),
            settings,
            selected: Mutex::new(selected),
        }
    }
}

impl TileRenderer for AudioInputTile {
    fn id(&self) -> &str { &self.id }
    fn name(&self) -> &str { "Audio Input" }
    fn update(&mut self) {}

    fn render_monitor(&self, draw: &Draw, rect: Rect, _ctx: &RenderContext) {
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .color(srgba(0.03, 0.03, 0.06, 0.95));

        let selected = self
            .selected
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|_| "Default".to_string());

        draw_text(
            draw,
            FontId::PlexSansBold,
            "AUDIO INPUT",
            pt2(rect.x(), rect.top() - 18.0),
            12.0,
            srgba(0.6, 0.8, 0.9, 1.0),
            TextAlignment::Center,
        );

        draw_text(
            draw,
            FontId::PlexMonoRegular,
            &format!("Device: {}", selected),
            pt2(rect.x(), rect.y() - 4.0),
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
                "device": {
                    "type": "string",
                    "default": "Default",
                    "title": "Input Device"
                }
            }
        }))
    }

    fn apply_settings(&mut self, settings: &serde_json::Value) {
        if let Some(device) = settings.get("device").and_then(|v| v.as_str()) {
            if let Ok(mut current) = self.selected.lock() {
                *current = device.to_string();
            }
            self.settings.set_selected(device.to_string());
        }
    }

    fn get_settings(&self) -> serde_json::Value {
        let device = self
            .selected
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|_| "Default".to_string());
        serde_json::json!({ "device": device })
    }

    fn bindable_actions(&self) -> Vec<BindableAction> { vec![] }
}
