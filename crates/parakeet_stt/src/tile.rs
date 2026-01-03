use nannou::prelude::*;
use serde_json::json;
use magnolia_core::{BindableAction, ControlSignal, RenderContext, Signal, TileRenderer};
use magnolia_ui::{draw_text, text_width, FontId, TextAlignment};
use tokio::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use crate::{ParakeetSttState, TranscriptionState};

pub struct ParakeetSttControlTile {
    id: String,
    sender: Sender<Signal>,
    state: Arc<Mutex<ParakeetSttState>>,
}

impl ParakeetSttControlTile {
    pub fn new(id: &str, sender: Sender<Signal>, state: Arc<Mutex<ParakeetSttState>>) -> Self {
        Self {
            id: id.to_string(),
            sender,
            state,
        }
    }

    fn send_action(&self, action: &str) {
        let payload = json!({ "action": action });
        let _ = self
            .sender
            .try_send(Signal::Control(ControlSignal::Settings(payload)));
        if let Ok(mut s) = self.state.lock() {
            s.status = action.to_string();
        }
    }
}

impl TileRenderer for ParakeetSttControlTile {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        "Parakeet STT"
    }

    fn update(&mut self) {}

    fn render_monitor(&self, draw: &Draw, rect: Rect, _ctx: &RenderContext) {
        draw.rect()
            .xy(rect.xy())
            .wh(rect.wh())
            .color(srgba(0.02, 0.02, 0.03, 0.95));

        draw_text(
            draw,
            FontId::PlexSansBold,
            "PARAKEET STT",
            pt2(rect.x(), rect.top() - 18.0),
            12.0,
            srgba(0.8, 0.9, 1.0, 1.0),
            TextAlignment::Center,
        );

        let (status, latency, decode, rtf) = if let Ok(s) = self.state.lock() {
            (s.status.clone(), s.latency_ms, s.decode_ms, s.rtf)
        } else {
            ("unknown".to_string(), 0, 0, 0.0)
        };

        draw_text(
            draw,
            FontId::PlexMonoRegular,
            &format!("Status: {}", status),
            pt2(rect.left() + 12.0, rect.y() + 15.0),
            11.0,
            srgba(0.6, 0.8, 0.9, 1.0),
            TextAlignment::Left,
        );

        draw_text(
            draw,
            FontId::PlexMonoRegular,
            &format!("Lat: {}ms  Dec: {}ms", latency, decode),
            pt2(rect.left() + 12.0, rect.y() - 5.0),
            10.0,
            srgba(0.5, 0.7, 1.0, 0.8),
            TextAlignment::Left,
        );

        draw_text(
            draw,
            FontId::PlexMonoRegular,
            &format!("RTF: {:.2}", rtf),
            pt2(rect.left() + 12.0, rect.y() - 22.0),
            10.0,
            if rtf < 1.0 { srgba(0.0, 1.0, 0.5, 0.9) } else { srgba(1.0, 0.5, 0.0, 0.9) },
            TextAlignment::Left,
        );

        draw_text(
            draw,
            FontId::PlexMonoRegular,
            "[S] start  [T] stop  [R] reset",
            pt2(rect.left() + 12.0, rect.bottom() + 15.0),
            9.0,
            srgba(0.4, 0.4, 0.45, 1.0),
            TextAlignment::Left,
        );
    }

    fn render_controls(&self, draw: &Draw, rect: Rect, ctx: &RenderContext) -> bool {
        self.render_monitor(draw, rect, ctx);
        false
    }

    fn bindable_actions(&self) -> Vec<BindableAction> {
        vec![
            BindableAction::new("start", "Start STT", true),
            BindableAction::new("stop", "Stop STT", true),
            BindableAction::new("reset", "Reset STT", true),
        ]
    }

    fn execute_action(&mut self, action: &str) -> bool {
        match action {
            "start" => {
                self.send_action("start");
                true
            }
            "stop" => {
                self.send_action("stop");
                true
            }
            "reset" => {
                self.send_action("reset");
                true
            }
            _ => false,
        }
    }

    fn handle_key(&mut self, key: Key, _ctrl: bool, _shift: bool) -> bool {
        match key {
            Key::S => {
                self.send_action("start");
                true
            }
            Key::T => {
                self.send_action("stop");
                true
            }
            Key::R => {
                self.send_action("reset");
                true
            }
            _ => false,
        }
    }

    fn prefers_gpu(&self) -> bool {
        false
    }
}

pub struct TranscriptionTile {
    id: String,
    state: Arc<Mutex<TranscriptionState>>,
}

impl TranscriptionTile {
    pub fn new(id: &str, state: Arc<Mutex<TranscriptionState>>) -> Self {
        Self {
            id: id.to_string(),
            state,
        }
    }
}

fn wrap_text_lines(text: &str, font: FontId, size: f32, max_width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for segment in text.split('\n') {
        let mut current = String::new();
        for word in segment.split_whitespace() {
            let candidate = if current.is_empty() {
                word.to_string()
            } else {
                format!("{} {}", current, word)
            };
            if text_width(font, &candidate, size) > max_width && !current.is_empty() {
                lines.push(current);
                current = word.to_string();
            } else {
                current = candidate;
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
    }
    lines
}

impl TileRenderer for TranscriptionTile {
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
            .color(srgba(0.02, 0.02, 0.03, 0.95));

        draw_text(
            draw,
            FontId::PlexSansBold,
            "LIVE CAPTIONS",
            pt2(rect.x(), rect.top() - 18.0),
            12.0,
            srgba(0.9, 0.9, 0.95, 1.0),
            TextAlignment::Center,
        );

        let (committed, partial, latency_ms, stt_ms, decode_ms, err, dropped) = if let Ok(s) = self.state.lock() {
            (
                s.committed_text.clone(),
                s.partial_text.clone(),
                s.last_latency_ms,
                s.last_stt_latency_ms,
                s.last_decode_ms,
                s.last_error.clone(),
                s.dropped_audio_total,
            )
        } else {
            (String::new(), String::new(), 0, 0, 0, None, 0)
        };

        let padding = 12.0;
        let font_size = 14.0;
        let line_height = font_size * 1.35;
        let max_width = rect.w() - padding * 2.0;
        let mut y = rect.top() - 42.0;

        let committed_lines = wrap_text_lines(&committed, FontId::PlexMonoRegular, font_size, max_width);
        for line in committed_lines {
            if y < rect.bottom() + 60.0 {
                break;
            }
            draw_text(
                draw,
                FontId::PlexMonoRegular,
                &line,
                pt2(rect.left() + padding, y),
                font_size,
                srgba(0.9, 0.9, 0.95, 1.0),
                TextAlignment::Left,
            );
            y -= line_height;
        }

        if !partial.trim().is_empty() && y > rect.bottom() + 60.0 {
            y -= line_height * 0.25;
            let partial_lines = wrap_text_lines(&partial, FontId::PlexMonoRegular, font_size, max_width);
            for line in partial_lines {
                if y < rect.bottom() + 40.0 {
                    break;
                }
                draw_text(
                    draw,
                    FontId::PlexMonoRegular,
                    &line,
                    pt2(rect.left() + padding, y),
                    font_size,
                    srgba(0.6, 0.8, 1.0, 0.9),
                    TextAlignment::Left,
                );
                y -= line_height;
            }
        }

        let stats = format!("E2E: {}ms  STT: {}ms  Dec: {}ms  Drop: {}", latency_ms, stt_ms, decode_ms, dropped);
        draw_text(
            draw,
            FontId::PlexMonoRegular,
            &stats,
            pt2(rect.left() + padding, rect.bottom() + 18.0),
            9.5,
            srgba(0.5, 0.7, 0.9, 0.8),
            TextAlignment::Left,
        );

        if let Some(msg) = err {
            draw_text(
                draw,
                FontId::PlexMonoRegular,
                &format!("ERR: {}", msg),
                pt2(rect.left() + padding, rect.bottom() + 6.0),
                9.0,
                srgba(1.0, 0.4, 0.3, 0.9),
                TextAlignment::Left,
            );
        }
    }

    fn render_controls(&self, draw: &Draw, rect: Rect, ctx: &RenderContext) -> bool {
        self.render_monitor(draw, rect, ctx);
        false
    }

    fn prefers_gpu(&self) -> bool {
        false
    }
}
