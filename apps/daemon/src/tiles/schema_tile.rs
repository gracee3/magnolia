use talisman_core::{TileRenderer, RenderContext, Signal, ControlSignal};
use nannou::prelude::*;
use serde_json::Value;
use tokio::sync::mpsc::Sender;
use std::sync::Mutex;

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
    fn id(&self) -> &str { &self.id }
    fn name(&self) -> &str { &self.name }

    fn render_monitor(&self, draw: &Draw, rect: Rect, _ctx: &RenderContext) {
        // Background
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .color(rgba(0.05, 0.05, 0.05, 1.0))
            .stroke(rgba(0.2, 0.2, 0.2, 1.0))
            .stroke_weight(1.0);
            
        // Name
        draw.text(&self.name)
            .xy(rect.xy())
            .color(WHITESMOKE)
            .font_size(14);
            
        // Status indicator (green dot for "Connected" since we have a sender)
        draw.ellipse()
            .x_y(rect.right() - 10.0, rect.top() - 10.0)
            .radius(3.0)
            .color(GREEN);
    }

    fn render_controls(&self, _draw: &Draw, _rect: Rect, ctx: &RenderContext) -> bool {
        let mut captured = false;
        
        if let Some(egui_ctx) = ctx.egui_ctx {
             let window_name = format!("{} Settings", self.name);
             nannou_egui::egui::Window::new(&window_name)
                .resizable(true)
                .collapsible(false)
                .show(egui_ctx, |ui| {
                    if let Some(schema) = &self.schema {
                        if let Ok(mut settings) = self.settings.lock() {
                            // Order: ui, settings, schema
                            if crate::ui::schema::render_schema_ui(ui, &mut *settings, schema) {
                                // Settings changed!
                                self.send_update(settings.clone());
                            }
                        }
                    } else {
                        ui.label("No settings schema available for this plugin.");
                    }
                });
             
             captured = egui_ctx.wants_pointer_input() || egui_ctx.wants_keyboard_input();
        }
        
        captured
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
