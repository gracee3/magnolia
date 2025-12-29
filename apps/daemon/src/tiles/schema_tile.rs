use nannou::prelude::*;
use serde_json::Value;
use std::sync::Mutex;
use talisman_core::{ControlSignal, RenderContext, Signal, TileRenderer};
use talisman_ui::{draw_text, FontId, TextAlignment};
use tokio::sync::mpsc::Sender;

pub struct SchemaTile {
    id: String,
    name: String,
    schema: Option<Value>,
    settings: Mutex<Value>,
    sender: Sender<Signal>,
}

impl SchemaTile {
    pub fn new(id: &str, name: &str, schema: Option<Value>, sender: Sender<Signal>) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            schema,
            settings: Mutex::new(Value::Null),
            sender,
        }
    }

    fn send_update(&self, settings: Value) {
        let signal = Signal::Control(ControlSignal::Settings(settings));
        let _ = self.sender.try_send(signal);
    }
}

impl TileRenderer for SchemaTile {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        &self.name
    }

    fn render_monitor(&self, draw: &Draw, rect: Rect, _ctx: &RenderContext) {
        // Background
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .color(rgba(0.05, 0.05, 0.05, 1.0))
            .stroke(rgba(0.2, 0.2, 0.2, 1.0))
            .stroke_weight(1.0);

        // Name
        draw_text(
            draw,
            FontId::PlexSansRegular,
            &self.name,
            rect.xy(),
            14.0,
            WHITESMOKE.into(),
            TextAlignment::Center,
        );

        // Status indicator (green dot for "Connected" since we have a sender)
        draw.ellipse()
            .x_y(rect.right() - 10.0, rect.top() - 10.0)
            .radius(3.0)
            .color(GREEN);
    }

    fn render_controls(&self, draw: &Draw, rect: Rect, _ctx: &RenderContext) -> bool {
        // Fullscreen placeholder
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .color(rgba(0.0, 0.0, 0.0, 0.9));

        draw_text(
            draw,
            FontId::PlexSansBold,
            &format!("{} - SETTINGS", self.name.to_uppercase()),
            rect.xy(),
            32.0,
            CYAN.into(),
            TextAlignment::Center,
        );

        draw_text(
            draw,
            FontId::PlexSansRegular,
            "Custom Nannou controls coming soon...",
            pt2(rect.x(), rect.y() - 40.0),
            14.0,
            GRAY.into(),
            TextAlignment::Center,
        );

        false
    }

    fn settings_schema(&self) -> Option<Value> {
        self.schema.clone()
    }

    fn apply_settings(&mut self, settings: &Value) {
        if let Ok(mut guard) = self.settings.lock() {
            *guard = settings.clone();
            self.send_update(settings.clone());
        }
    }

    fn get_settings(&self) -> Value {
        self.settings.lock().unwrap().clone()
    }

    fn update(&mut self) {
        // Nothing for now
    }
}
