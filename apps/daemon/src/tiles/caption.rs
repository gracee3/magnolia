use super::{BindableAction, RenderContext, TileRenderer};
use caption_state::CaptionState;
use magnolia_ui::{draw_text, FontId, TextAlignment};
use nannou::prelude::*;
use std::sync::{Arc, Mutex};

/// Monitor tile for stable and provisional speech recognition text.
pub struct CaptionTile {
    id: String,
    state: Arc<Mutex<CaptionState>>,
}

impl CaptionTile {
    pub fn new(id: &str, state: Arc<Mutex<CaptionState>>) -> Self {
        Self {
            id: id.to_string(),
            state,
        }
    }

    fn lines(text: &str, max_chars: usize) -> Vec<String> {
        let mut lines = Vec::new();
        let mut current = String::new();
        for word in text.split_whitespace() {
            if !current.is_empty() && current.len() + word.len() + 1 > max_chars {
                lines.push(std::mem::take(&mut current));
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
        if !current.is_empty() {
            lines.push(current);
        }
        lines
    }
}

impl TileRenderer for CaptionTile {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        "Live Captions"
    }
    fn update(&mut self) {}

    fn render_monitor(&self, draw: &Draw, rect: Rect, _ctx: &RenderContext) {
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .color(srgba(0.02, 0.02, 0.05, 0.96));

        draw_text(
            draw,
            FontId::PlexSansBold,
            "LIVE CAPTIONS",
            pt2(rect.x(), rect.top() - 18.0),
            12.0,
            srgba(0.0, 1.0, 1.0, 0.9),
            TextAlignment::Center,
        );

        let Ok(state) = self.state.lock() else { return };
        let text = state.display_text();
        if text.is_empty() {
            draw_text(
                draw,
                FontId::PlexSansRegular,
                "Listening for speech…",
                rect.xy(),
                14.0,
                srgba(0.45, 0.48, 0.55, 1.0),
                TextAlignment::Center,
            );
            return;
        }

        let lines = Self::lines(&text, ((rect.w() / 9.0) as usize).max(18));
        let line_height = 18.0;
        let first_y = rect.y() + (lines.len() as f32 - 1.0) * line_height / 2.0;
        for (index, line) in lines.iter().enumerate() {
            let is_provisional = state.provisional.is_some() && index + 1 == lines.len();
            draw_text(
                draw,
                FontId::PlexSansRegular,
                line,
                pt2(rect.x(), first_y - index as f32 * line_height),
                14.0,
                if is_provisional {
                    srgba(0.70, 0.72, 0.80, 0.9)
                } else {
                    srgba(0.95, 0.96, 1.0, 1.0)
                },
                TextAlignment::Center,
            );
        }
    }

    fn render_controls(&self, draw: &Draw, rect: Rect, ctx: &RenderContext) -> bool {
        self.render_monitor(draw, rect, ctx);
        draw_text(
            draw,
            FontId::PlexMonoRegular,
            "Partial text is dimmed; endpoint text is committed",
            pt2(rect.x(), rect.bottom() + 22.0),
            11.0,
            srgba(0.45, 0.48, 0.55, 1.0),
            TextAlignment::Center,
        );
        false
    }

    fn get_display_text(&self) -> Option<String> {
        self.state.lock().ok().map(|state| state.display_text())
    }

    fn bindable_actions(&self) -> Vec<BindableAction> {
        Vec::new()
    }
}
