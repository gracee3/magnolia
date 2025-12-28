use std::sync::Arc;

use nannou::prelude::*;
use talisman_core::{TileRenderer, RenderContext};

use crate::AudioOutputState;

pub struct AudioOutputTile {
    id: String,
    state: Arc<AudioOutputState>,
}

impl AudioOutputTile {
    pub fn new(id: &str, state: Arc<AudioOutputState>) -> Self {
        Self {
            id: id.to_string(),
            state,
        }
    }
}

impl TileRenderer for AudioOutputTile {
    fn id(&self) -> &str { &self.id }
    fn name(&self) -> &str { "Audio Output" }
    fn update(&mut self) {}
    
    fn render_monitor(&self, draw: &Draw, rect: Rect, _ctx: &RenderContext) {
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .color(srgba(0.03, 0.03, 0.06, 0.95));

        let latency_ms = self.state.latency_us() as f32 / 1000.0;
        let level = self.state.level_milli() as f32 / 1000.0;

        draw.text("AUDIO OUT")
            .xy(pt2(rect.x(), rect.top() - 18.0))
            .color(srgba(0.6, 0.8, 0.9, 1.0))
            .font_size(12);

        draw.text(&format!("Latency: {:.1} ms", latency_ms))
            .xy(pt2(rect.x(), rect.y() + 10.0))
            .color(srgba(0.5, 0.7, 0.9, 1.0))
            .font_size(11);

        draw.text(&format!("Level: {:.2}", level))
            .xy(pt2(rect.x(), rect.y() - 12.0))
            .color(srgba(0.7, 0.9, 0.6, 1.0))
            .font_size(11);
    }
}
