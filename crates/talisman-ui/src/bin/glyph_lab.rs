use talisman_ui::{self, FontId, GlyphOp, theme};
use talisman_ui::tweaks::GlyphTweaks;
use nannou::lyon::math::point as lpoint;
use nannou::lyon::path::Path;
use nannou::prelude::*;

fn main() {
    nannou::app(model).update(update).run();
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ViewMode {
    Grid,
    Detail,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum DisplayGlyph {
    // We allow a "None" or placeholder if needed, but for now just Font
    Font(FontId, char),
}

struct Model {
    tweaks: GlyphTweaks,
    stroke_width: f32,
    tolerance: f32,
    line_join: nannou::lyon::tessellation::LineJoin,
    line_cap: nannou::lyon::tessellation::LineCap,
    fill: bool,
    stroke: bool,
    show_bounds: bool,
    cell_size: f32,
    big_glyph_size: f32,
    glyphs: Vec<(DisplayGlyph, String)>,
    selected_index: usize,
    view_mode: ViewMode,
    scroll_y: f32,
}

fn model(app: &App) -> Model {
    app.new_window()
        .title("Talisman Glyph Lab")
        .size(1600, 1000)
        .key_pressed(key_pressed)
        .view(view)
        .build()
        .unwrap();

    let tweaks_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../configs/glyph_tweaks.toml");
    let tweaks = GlyphTweaks::load_from_file(tweaks_path).unwrap_or_else(|e| {
        eprintln!("Failed to load tweaks: {}", e);
        GlyphTweaks::default()
    });

    let mut glyphs: Vec<(DisplayGlyph, String)> = Vec::new();

    let fonts = [
        (FontId::PlexSansRegular, "SansReg"),
        (FontId::PlexSansBold, "SansBold"),
        (FontId::PlexMonoRegular, "MonoReg"),
        (FontId::PlexMonoMedium, "MonoMed"),
    ];

    let sample_chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*()_+-=[]{};:'\",.<>/?|\\`~";
    
    for (font, prefix) in fonts {
        for c in sample_chars.chars() {
            glyphs.push((DisplayGlyph::Font(font, c), format!("{}-{}", prefix, c)));
        }
    }

    Model {
        tweaks,
        stroke_width: 2.0,
        tolerance: 0.1,
        line_join: nannou::lyon::tessellation::LineJoin::Round,
        line_cap: nannou::lyon::tessellation::LineCap::Round,
        fill: false,
        stroke: true,
        show_bounds: false,
        cell_size: 160.0,
        big_glyph_size: 100.0,
        glyphs,
        selected_index: 0,
        view_mode: ViewMode::Grid,
        scroll_y: 0.0,
    }
}

fn update(_app: &App, _model: &mut Model, _update: Update) {
    // Smoothen scrolling if needed, but for now just instant
}

fn key_pressed(app: &App, model: &mut Model, key: Key) {
    let win = app.window_rect();
    let hud_height = 80.0;
    let grid_rect = Rect::from_corners(
        win.top_left() + vec2(0.0, -hud_height),
        win.bottom_right()
    );
    let cols = (grid_rect.w() / model.cell_size).floor() as usize;
    if cols == 0 { return; }
    let count = model.glyphs.len();

    match key {
        Key::Space => {
            model.view_mode = match model.view_mode {
                ViewMode::Grid => ViewMode::Detail,
                ViewMode::Detail => ViewMode::Grid,
            };
        }
        Key::Escape => {
            model.view_mode = ViewMode::Grid;
        }
        Key::Right => {
            if model.selected_index + 1 < count {
                model.selected_index += 1;
            }
        }
        Key::Left => {
            if model.selected_index > 0 {
                model.selected_index -= 1;
            }
        }
        Key::Down => {
            if model.selected_index + cols < count {
                model.selected_index += cols;
            }
        }
        Key::Up => {
            if model.selected_index >= cols {
                model.selected_index -= cols;
            }
        }
        Key::LBracket => model.stroke_width = (model.stroke_width - 0.5).max(0.1),
        Key::RBracket => model.stroke_width += 0.5,
        Key::Comma => model.tolerance = (model.tolerance - 0.01).max(0.01),
        Key::Period => model.tolerance += 0.01,
        Key::J => {
            model.line_join = match model.line_join {
                nannou::lyon::tessellation::LineJoin::Miter => nannou::lyon::tessellation::LineJoin::Round,
                nannou::lyon::tessellation::LineJoin::Round => nannou::lyon::tessellation::LineJoin::Bevel,
                nannou::lyon::tessellation::LineJoin::Bevel => nannou::lyon::tessellation::LineJoin::Miter,
                _ => model.line_join,
            }
        }
        Key::C => {
            model.line_cap = match model.line_cap {
                nannou::lyon::tessellation::LineCap::Butt => nannou::lyon::tessellation::LineCap::Round,
                nannou::lyon::tessellation::LineCap::Round => nannou::lyon::tessellation::LineCap::Square,
                nannou::lyon::tessellation::LineCap::Square => nannou::lyon::tessellation::LineCap::Butt,
                _ => model.line_cap,
            }
        }
        Key::F => model.fill = !model.fill,
        Key::S => model.stroke = !model.stroke,
        Key::B => model.show_bounds = !model.show_bounds,
        Key::R => {
            model.stroke_width = 2.0;
            model.tolerance = 0.1;
            model.line_join = nannou::lyon::tessellation::LineJoin::Round;
            model.line_cap = nannou::lyon::tessellation::LineCap::Round;
            model.fill = false;
            model.stroke = true;
            model.big_glyph_size = 100.0;
            model.show_bounds = false;
        }
        Key::P => {
            let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../screenshots/glyph_lab.png");
            app.main_window().capture_frame(path);
            println!("Screenshot saved to {}", path);
        }
        Key::T => {
             let tweaks_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../configs/glyph_tweaks.toml");
             if let Ok(tweaks) = GlyphTweaks::load_from_file(tweaks_path) {
                 model.tweaks = tweaks;
                 println!("Reloaded tweaks");
             }
        }
        _ => {}
    }

    // Auto-scroll logic
    if model.view_mode == ViewMode::Grid {
        let row = model.selected_index / cols;
        let y_pos = row as f32 * model.cell_size;
        let visible_height = grid_rect.h();
        
        let view_top = model.scroll_y;
        let view_bottom = model.scroll_y + visible_height;
        
        if y_pos < view_top {
            model.scroll_y = y_pos;
        } else if y_pos + model.cell_size > view_bottom {
            model.scroll_y = y_pos + model.cell_size - visible_height;
        }
    }
}

fn build_path_for_glyph(_item: DisplayGlyph, ops: &[GlyphOp], bounds: talisman_ui::GlyphBounds, rect: Rect, tweaks: &GlyphTweaks, glyph_name: &str) -> Path {
    let width = bounds.max_x - bounds.min_x;
    let height = bounds.max_y - bounds.min_y;
    
    let tweak = tweaks.get(glyph_name.to_lowercase().as_str());

    let bounds_cx = (bounds.min_x + bounds.max_x) * 0.5;
    let bounds_cy = (bounds.min_y + bounds.max_y) * 0.5;

    let mut builder = Path::builder();
    let mut open = false;
    
    let s = (rect.w() / width).min(rect.h() / height);
    let center = rect.xy();

    let rot_rad = tweak.rot_deg.to_radians();
    let (sin, cos) = rot_rad.sin_cos();

    let transform = |x: f32, y: f32| -> nannou::lyon::math::Point {
        let mut lx = x - bounds_cx;
        let mut ly = y - bounds_cy;
        
        lx *= tweak.sx;
        ly *= tweak.sy;
        
        let rx = lx * cos - ly * sin;
        let ry = lx * sin + ly * cos;
        lx = rx;
        ly = ry;
        
        lx += tweak.dx * width;
        ly += tweak.dy * height;

        let final_x = lx * s + center.x;
        let final_y = ly * s + center.y;
        
        lpoint(final_x, final_y)
    };

    for op in ops {
        match *op {
            GlyphOp::M(x, y) => {
                if open { builder.end(false); }
                builder.begin(transform(x, y));
                open = true;
            }
            GlyphOp::L(x, y) => {
                 if !open { builder.begin(transform(x, y)); open = true; }
                 builder.line_to(transform(x, y));
            }
            GlyphOp::Q(x1, y1, x, y) => {
                if !open { builder.begin(transform(x1, y1)); open = true; }
                builder.quadratic_bezier_to(transform(x1, y1), transform(x, y));
            }
            GlyphOp::C(x1, y1, x2, y2, x, y) => {
                if !open { builder.begin(transform(x1, y1)); open = true; }
                builder.cubic_bezier_to(transform(x1, y1), transform(x2, y2), transform(x, y));
            }
            GlyphOp::Z => {
                if open { builder.end(true); open = false; }
            }
        }
    }
    if open { builder.end(false); }
    builder.build()
}


fn view(app: &App, model: &Model, frame: Frame) {
    let draw = app.draw();
    draw.background().color(BLACK);

    let win = app.window_rect();
    let hud_height = 80.0;
    
    match model.view_mode {
        ViewMode::Grid => {
            let grid_rect = Rect::from_corners(
                win.top_left() + vec2(0.0, -hud_height),
                win.bottom_right()
            );

            let cols = (grid_rect.w() / model.cell_size).floor() as usize;
            if cols > 0 {
                let start_x = grid_rect.left() + model.cell_size * 0.5;
                let start_y = grid_rect.top() - model.cell_size * 0.5 + model.scroll_y;

                for (i, (item, name)) in model.glyphs.iter().enumerate() {
                    let col = i % cols;
                    let row = i / cols;
                    let x = start_x + col as f32 * model.cell_size;
                    let y = start_y - row as f32 * model.cell_size;
                    
                    // Optimization: don't draw if off-screen
                    if y > grid_rect.top() + model.cell_size || y < grid_rect.bottom() - model.cell_size {
                        continue;
                    }

                    let center = pt2(x, y);
                    let is_selected = i == model.selected_index;
                    
                    if is_selected {
                        draw.rect().xy(center).w_h(model.cell_size, model.cell_size).no_fill().stroke(CYAN).stroke_weight(3.0);
                    } else {
                        draw.rect().xy(center).w_h(model.cell_size, model.cell_size).no_fill().stroke(theme::muted_stroke()).stroke_weight(1.0);
                    }

                    if model.show_bounds {
                         draw.rect().xy(center).w_h(model.big_glyph_size, model.big_glyph_size).no_fill().stroke(RED).stroke_weight(1.0);
                    }

                    let name_color = if is_selected { CYAN } else { WHITE };
                    draw.text(name).xy(center + vec2(0.0, -model.cell_size * 0.4)).color(name_color).font_size(10);
                    
                    let DisplayGlyph::Font(font, c) = item;
                    if let Some((ops, bounds)) = talisman_ui::font_glyph_ops_bounds(*font, *c) {
                         let big_rect = Rect::from_xy_wh(center, vec2(model.big_glyph_size, model.big_glyph_size));
                         let path = build_path_for_glyph(*item, ops, bounds, big_rect, &model.tweaks, name);
                         
                         if model.fill {
                             draw.path().fill().events(path.iter()).color(WHITE);
                         }
                         if model.stroke {
                             draw.path().stroke().weight(model.stroke_width).join(model.line_join).caps(model.line_cap).events(path.iter()).color(WHITE);
                         }
                    }
                }
            }
        }
        ViewMode::Detail => {
            let detail_rect = Rect::from_corners(
                win.top_left() + vec2(0.0, -hud_height),
                win.bottom_right()
            ).pad(40.0);

            let (item, name) = &model.glyphs[model.selected_index];
            let DisplayGlyph::Font(font, c) = item;
            
            if let Some((ops, bounds)) = talisman_ui::font_glyph_ops_bounds(*font, *c) {
                let path = build_path_for_glyph(*item, ops, bounds, detail_rect, &model.tweaks, name);
                
                if model.fill {
                    draw.path().fill().events(path.iter()).color(WHITE);
                }
                if model.stroke {
                    draw.path().stroke().weight(model.stroke_width).join(model.line_join).caps(model.line_cap).events(path.iter()).color(WHITE);
                }
                
                if model.show_bounds {
                    draw.rect().xy(detail_rect.xy()).w_h(detail_rect.w(), detail_rect.h()).no_fill().stroke(RED).stroke_weight(1.0);
                }
            }
            
            draw.text(name)
                .xy(detail_rect.bottom_left() + vec2(50.0, 50.0))
                .color(CYAN)
                .font_size(32);
        }
    }
    
    // Draw HUD
    let sel_name = &model.glyphs[model.selected_index].1;
    let sel_tweak = model.tweaks.get(&sel_name.to_lowercase());

    let hud_text = format!(
        "FPS: {:.1} | Mode: {:?} | Stroke: {:.1} | Tol: {:.2} | Join: {:?} | Cap: {:?} | Fill: {} | Stroke: {} | Size: {:.0}\n\
         SELECTED: {} | dx: {:.3} dy: {:.3} sx: {:.3} sy: {:.3} rot: {:.1}",
        app.fps(), model.view_mode, model.stroke_width, model.tolerance, model.line_join, model.line_cap, model.fill, model.stroke, model.big_glyph_size,
        sel_name, sel_tweak.dx, sel_tweak.dy, sel_tweak.sx, sel_tweak.sy, sel_tweak.rot_deg
    );
    
    draw.text(&hud_text)
        .xy(win.top_left() + vec2(400.0, -40.0))
        .color(YELLOW)
        .font_size(16);

    draw.to_frame(app, &frame).unwrap();
}
