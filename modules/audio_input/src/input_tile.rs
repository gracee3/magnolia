use std::sync::{Arc, Mutex};

use nannou::prelude::*;
use nannou_egui::egui;
use talisman_core::{TileRenderer, RenderContext, BindableAction};

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

        draw.text("AUDIO INPUT")
            .xy(pt2(rect.x(), rect.top() - 18.0))
            .color(srgba(0.6, 0.8, 0.9, 1.0))
            .font_size(12);

        draw.text(&format!("Device: {}", selected))
            .xy(pt2(rect.x(), rect.y() - 4.0))
            .color(srgba(0.5, 0.7, 0.9, 1.0))
            .font_size(11);
    }

    fn render_controls(&self, draw: &Draw, rect: Rect, ctx: &RenderContext) -> bool {
        self.render_monitor(draw, rect, ctx);

        let Some(egui_ctx) = ctx.egui_ctx else { return false; };
        let devices = self.settings.devices();
        let mut selected = self
            .selected
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|_| "Default".to_string());

        egui::Area::new(egui::Id::new(format!("{}_audio_in_controls", self.id)))
            .fixed_pos(egui::pos2(rect.left() + 20.0, rect.top() - 50.0))
            .show(egui_ctx, |ui| {
                ui.set_max_width(280.0);
                egui::Frame::none()
                    .fill(egui::Color32::from_rgba_unmultiplied(10, 10, 15, 240))
                    .inner_margin(egui::Margin::same(12.0))
                    .show(ui, |ui| {
                        ui.heading("Input Device");
                        ui.add_space(8.0);
                        egui::ComboBox::from_id_source("audio_input_device")
                            .selected_text(&selected)
                            .width(240.0)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut selected, "Default".to_string(), "Default");
                                for dev in devices {
                                    ui.selectable_value(&mut selected, dev.clone(), dev);
                                }
                            });
                    });
            });

        if let Ok(mut current) = self.selected.lock() {
            if *current != selected {
                *current = selected.clone();
                self.settings.set_selected(selected);
            }
        }

        true
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
