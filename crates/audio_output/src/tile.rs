use std::sync::{Arc, Mutex};

use nannou::prelude::*;
use talisman_core::{RenderContext, TileError, TileRenderer};
use talisman_ui::{draw_text, FontId, TextAlignment};

use crate::{AudioOutputSettings, AudioOutputState};

pub struct AudioOutputTile {
    id: String,
    state: Arc<AudioOutputState>,
    settings: Arc<AudioOutputSettings>,
    selected: Mutex<String>,
    focus: Mutex<usize>,
}

impl AudioOutputTile {
    pub fn new(id: &str, state: Arc<AudioOutputState>, settings: Arc<AudioOutputSettings>) -> Self {
        let selected = settings.selected();
        Self {
            id: id.to_string(),
            state,
            settings,
            selected: Mutex::new(selected),
            focus: Mutex::new(0),
        }
    }
}

impl TileRenderer for AudioOutputTile {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        "Audio Output"
    }
    fn update(&mut self) {}

    fn render_monitor(&self, draw: &Draw, rect: Rect, _ctx: &RenderContext) {
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .color(srgba(0.03, 0.03, 0.06, 0.95));

        let latency_ms = self.state.latency_us() as f32 / 1000.0;
        let level = self.state.level_milli() as f32 / 1000.0;

        draw_text(
            draw,
            FontId::PlexSansBold,
            "AUDIO OUT",
            pt2(rect.x(), rect.top() - 18.0),
            12.0,
            srgba(0.6, 0.8, 0.9, 1.0),
            TextAlignment::Center,
        );

        draw_text(
            draw,
            FontId::PlexMonoRegular,
            &format!("Latency: {:.1} ms", latency_ms),
            pt2(rect.x(), rect.y() + 10.0),
            11.0,
            srgba(0.5, 0.7, 0.9, 1.0),
            TextAlignment::Center,
        );

        let selected_id = self
            .selected
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|_| "Default".to_string());
        let active = self.settings.active_device().unwrap_or(selected_id.clone());
        draw_text(
            draw,
            FontId::PlexSansRegular,
            &format!("Device: {}", active),
            pt2(rect.x(), rect.y() - 6.0),
            11.0,
            srgba(0.5, 0.7, 0.9, 1.0),
            TextAlignment::Center,
        );

        draw_text(
            draw,
            FontId::PlexMonoRegular,
            &format!("Level: {:.2}", level),
            pt2(rect.x(), rect.y() - 22.0),
            11.0,
            srgba(0.7, 0.9, 0.6, 1.0),
            TextAlignment::Center,
        );
    }

    fn render_controls(&self, draw: &Draw, rect: Rect, ctx: &RenderContext) -> bool {
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .color(srgba(0.02, 0.02, 0.05, 0.98));

        draw_text(
            draw,
            FontId::PlexSansBold,
            "AUDIO OUTPUT SETTINGS",
            pt2(rect.x(), rect.top() - 30.0),
            18.0,
            srgba(0.0, 1.0, 1.0, 1.0),
            TextAlignment::Center,
        );

        draw_text(
            draw,
            FontId::PlexSansRegular,
            "[Up/Down] Select  [Enter] Apply  [R] Refresh",
            pt2(rect.x(), rect.top() - 55.0),
            12.0,
            srgba(0.5, 0.5, 0.55, 1.0),
            TextAlignment::Center,
        );

        let selected = self.settings.selected();
        let active = self.settings.active_device().unwrap_or_else(|| "None".to_string());
        let fmt = self
            .settings
            .format()
            .map(|(sr, ch)| format!("{} Hz / {} ch", sr, ch))
            .unwrap_or_else(|| "Unknown".to_string());

        draw_text(
            draw,
            FontId::PlexMonoRegular,
            &format!("Selected: {}", selected),
            pt2(rect.left() + 20.0, rect.top() - 90.0),
            12.0,
            srgba(0.6, 0.7, 0.9, 1.0),
            TextAlignment::Left,
        );
        draw_text(
            draw,
            FontId::PlexMonoRegular,
            &format!("Active: {}", active),
            pt2(rect.left() + 20.0, rect.top() - 110.0),
            12.0,
            srgba(0.6, 0.7, 0.9, 1.0),
            TextAlignment::Left,
        );
        draw_text(
            draw,
            FontId::PlexMonoRegular,
            &format!("Format: {}", fmt),
            pt2(rect.left() + 20.0, rect.top() - 130.0),
            12.0,
            srgba(0.6, 0.7, 0.9, 1.0),
            TextAlignment::Left,
        );

        if let Some(err) = self.settings.last_error() {
            draw_text(
                draw,
                FontId::PlexMonoRegular,
                &format!("Error: {}", err),
                pt2(rect.left() + 20.0, rect.top() - 155.0),
                11.0,
                srgba(1.0, 0.3, 0.3, 0.9),
                TextAlignment::Left,
            );
        }

        let mut devices: Vec<(String, String)> =
            vec![("Default".to_string(), "Default".to_string())];
        for d in self.settings.devices() {
            devices.push((d.id, d.name));
        }

        let mut focus = self.focus.lock().map(|v| *v).unwrap_or(0);
        if focus >= devices.len() {
            focus = devices.len().saturating_sub(1);
        }

        let list_top = rect.top() - 190.0;
        let row_h = 26.0;
        let max_rows = ((rect.h() - 220.0) / row_h).floor().max(3.0) as usize;
        let start = focus.saturating_sub(max_rows / 2);
        let end = (start + max_rows).min(devices.len());

        for (i, (id, name)) in devices[start..end].iter().enumerate() {
            let idx = start + i;
            let y = list_top - (i as f32) * row_h;
            let row = Rect::from_x_y_w_h(rect.x(), y, rect.w() * 0.8, row_h - 2.0);
            let focused = idx == focus;

            draw.rect()
                .xy(row.xy())
                .wh(row.wh())
                .color(if focused {
                    srgba(0.0, 0.25, 0.25, 0.55)
                } else {
                    srgba(0.08, 0.08, 0.10, 0.9)
                });

            let label = if *id == "Default" {
                "Default".to_string()
            } else {
                format!("{} ({})", name, id)
            };

            draw_text(
                draw,
                FontId::PlexSansRegular,
                &label,
                pt2(row.left() + 12.0, row.y()),
                12.0,
                srgba(0.85, 0.85, 0.88, 1.0),
                TextAlignment::Left,
            );
        }

        let _ = ctx.is_maximized;
        false
    }

    fn handle_key(&mut self, key: nannou::prelude::Key, _ctrl: bool, _shift: bool) -> bool {
        let devices_len = 1 + self.settings.devices().len(); // + Default
        let mut focus = self.focus.lock().map(|v| *v).unwrap_or(0);
        focus = focus.min(devices_len.saturating_sub(1));

        match key {
            Key::Up => focus = focus.saturating_sub(1),
            Key::Down => focus = (focus + 1).min(devices_len.saturating_sub(1)),
            Key::Return => {
                let device_id = if focus == 0 {
                    "Default".to_string()
                } else {
                    self.settings
                        .devices()
                        .get(focus - 1)
                        .map(|d| d.id.clone())
                        .unwrap_or_else(|| "Default".to_string())
                };
                if let Ok(mut current) = self.selected.lock() {
                    *current = device_id.clone();
                }
                self.settings.set_selected(device_id);
            }
            Key::R => {
                let cur = self.settings.selected();
                self.settings.set_selected(cur);
            }
            _ => return false,
        }

        if let Ok(mut guard) = self.focus.lock() {
            *guard = focus;
        }
        true
    }

    fn get_error(&self) -> Option<TileError> {
        self.settings
            .last_error()
            .map(|e| TileError::new("Audio output backend error").with_details(&e))
    }

    fn settings_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "device": {
                    "type": "string",
                    "default": "Default",
                    "title": "Output Device"
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
}
