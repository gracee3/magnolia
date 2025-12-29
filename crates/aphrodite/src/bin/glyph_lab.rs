use aphrodite::rendering::glyphs::{self, Glyph, GlyphOp};
use aphrodite::rendering::tweaks::GlyphTweaks;
use nannou::lyon::math::point as lpoint;
use nannou::lyon::path::Path;
use nannou::prelude::*;

fn main() {
    nannou::app(model).update(update).run();
}

struct Model {
    tweaks: GlyphTweaks,
    stroke_width: f32,
    tolerance: f32,
    line_join: LineJoin,
    line_cap: LineCap,
    fill: bool,
    stroke: bool,
    show_bounds: bool,
    cell_size: f32,
    big_glyph_size: f32,
    small_glyph_size: f32,
    glyphs: Vec<(Glyph, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum LineJoin {
    Miter,
    Round,
    Bevel,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum LineCap {
    Butt,
    Round,
    Square,
}

fn model(app: &App) -> Model {
    app.new_window()
        .title("Glyph Lab")
        .size(1600, 1200)
        .key_pressed(key_pressed)
        .view(view)
        .build()
        .unwrap();

    let tweaks_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../configs/glyph_tweaks.toml");
    let tweaks = GlyphTweaks::load_from_file(tweaks_path).unwrap_or_else(|e| {
        eprintln!("Failed to load tweaks: {}", e);
        GlyphTweaks::default()
    });

    let glyphs = vec![
        // Signs
        (Glyph::Aries, "Aries"), (Glyph::Taurus, "Taurus"), (Glyph::Gemini, "Gemini"),
        (Glyph::Cancer, "Cancer"), (Glyph::Leo, "Leo"), (Glyph::Virgo, "Virgo"),
        (Glyph::Libra, "Libra"), (Glyph::Scorpio, "Scorpio"), (Glyph::Sagittarius, "Sagittarius"),
        (Glyph::Capricorn, "Capricorn"), (Glyph::Aquarius, "Aquarius"), (Glyph::Pisces, "Pisces"),
        // Planets
        (Glyph::Sun, "Sun"), (Glyph::Moon, "Moon"), (Glyph::Mercury, "Mercury"),
        (Glyph::Venus, "Venus"), (Glyph::Mars, "Mars"), (Glyph::Jupiter, "Jupiter"),
        (Glyph::Saturn, "Saturn"), (Glyph::Uranus, "Uranus"), (Glyph::Neptune, "Neptune"),
        (Glyph::Pluto, "Pluto"),
        // Angles
        (Glyph::Ascendant, "Asc"), (Glyph::Descendant, "Dsc"), (Glyph::MC, "MC"), (Glyph::IC, "IC"),
        // Houses
        (Glyph::House(1), "H1"), (Glyph::House(2), "H2"), (Glyph::House(3), "H3"),
        (Glyph::House(4), "H4"), (Glyph::House(5), "H5"), (Glyph::House(6), "H6"),
        (Glyph::House(7), "H7"), (Glyph::House(8), "H8"), (Glyph::House(9), "H9"),
        (Glyph::House(10), "H10"), (Glyph::House(11), "H11"), (Glyph::House(12), "H12"),
        // Others
        (Glyph::NNode, "NNode"), (Glyph::SNode, "SNode"), (Glyph::Lilith, "Lilith"),
        (Glyph::Chiron, "Chiron"), (Glyph::Fortune, "Fortune"),
    ].into_iter().map(|(g, s)| (g, s.to_string())).collect();

    Model {
        tweaks,
        stroke_width: 2.0,
        tolerance: 0.1,
        line_join: LineJoin::Round,
        line_cap: LineCap::Round,
        fill: false,
        stroke: true,
        show_bounds: false,
        cell_size: 200.0,
        big_glyph_size: 140.0,
        small_glyph_size: 24.0,
        glyphs,
    }
}

fn update(_app: &App, _model: &mut Model, _update: Update) {}

fn key_pressed(_app: &App, model: &mut Model, key: Key) {
    match key {
        Key::LBracket => model.stroke_width = (model.stroke_width - 0.5).max(0.1),
        Key::RBracket => model.stroke_width += 0.5,
        Key::Comma => model.tolerance = (model.tolerance - 0.01).max(0.01),
        Key::Period => model.tolerance += 0.01,
        Key::J => {
            model.line_join = match model.line_join {
                LineJoin::Miter => LineJoin::Round,
                LineJoin::Round => LineJoin::Bevel,
                LineJoin::Bevel => LineJoin::Miter,
            }
        }
        Key::C => {
            model.line_cap = match model.line_cap {
                LineCap::Butt => LineCap::Round,
                LineCap::Round => LineCap::Square,
                LineCap::Square => LineCap::Butt,
            }
        }
        Key::F => model.fill = !model.fill,
        Key::S => model.stroke = !model.stroke,
        Key::B => model.show_bounds = !model.show_bounds,
        Key::Key1 => { model.big_glyph_size = 100.0; }
        Key::Key2 => { model.big_glyph_size = 140.0; }
        Key::Key3 => { model.big_glyph_size = 180.0; }
        Key::R => {
            model.stroke_width = 2.0;
            model.tolerance = 0.1;
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
}

fn build_path_for_glyph(_glyph: Glyph, ops: &[GlyphOp], bounds: glyphs::GlyphBounds, rect: Rect, tweaks: &GlyphTweaks, glyph_name: &str) -> Path {
    let width = bounds.max_x - bounds.min_x;
    let height = bounds.max_y - bounds.min_y;
    
    // Apply tweak (Rotation -> Scale -> Translate) in normalized space (or rather, modify the bounds/ops effectively before fit)
    // Actually, simple fit logic:
    // 1. Center the glyph based on its bounds
    // 2. Apply tweaks (which might shift it relative to its own center or scale it)
    // 3. Fit to target rect
    
    let tweak = tweaks.get(glyph_name.to_lowercase().as_str());

    // Original center
    let bounds_cx = (bounds.min_x + bounds.max_x) * 0.5;
    let bounds_cy = (bounds.min_y + bounds.max_y) * 0.5;

    // We can just construct the path directly from ops, transforming each point
    let mut builder = Path::builder();
    let mut open = false;
    
    // Fitting scale
    let s = (rect.w() / width).min(rect.h() / height);
    // Base fit transform: translate center to 0,0, apply scale s, move to rect center
    let center = rect.xy();
    
    // We want to apply tweak BEFORE fitting to rect, or rather, as a modification of the glyph shape itself.
    // User asked: "apply the affine tweak transform (scale → rotate → translate) in glyph-local space then apply the 'fit-to-rect' transform"

    let rot_rad = tweak.rot_deg.to_radians();
    let (sin, cos) = rot_rad.sin_cos();

    let transform = |x: f32, y: f32| -> nannou::lyon::math::Point {
        // 1. Center relative to bounds (local coords)
        let mut lx = x - bounds_cx;
        let mut ly = y - bounds_cy;
        
        // 2. Scale
        lx *= tweak.sx;
        ly *= tweak.sy;
        
        // 3. Rotate
        let rx = lx * cos - ly * sin;
        let ry = lx * sin + ly * cos;
        lx = rx;
        ly = ry;
        
        // 4. Translate relative to width/height
        lx += tweak.dx * width;
        ly += tweak.dy * height;

        // 5. Fit to rect (scale s, translate to rect center)
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
    let cols = (win.w() / model.cell_size).floor() as usize;
    if cols == 0 { return; }

    let start_x = win.left() + model.cell_size * 0.5;
    let start_y = win.top() - model.cell_size * 0.5;

    for (i, (glyph, name)) in model.glyphs.iter().enumerate() {
        let col = i % cols;
        let row = i / cols;
        let x = start_x + col as f32 * model.cell_size;
        let y = start_y - row as f32 * model.cell_size;
        
        let center = pt2(x, y);
        
        // Draw cell Bounds
        draw.rect().xy(center).w_h(model.cell_size, model.cell_size).no_fill().stroke(GRAY).stroke_weight(1.0);
        
        if model.show_bounds {
             draw.rect().xy(center).w_h(model.big_glyph_size, model.big_glyph_size).no_fill().stroke(RED).stroke_weight(1.0);
        }

        // Draw Name
        draw.text(name).xy(center + vec2(0.0, -model.cell_size * 0.4)).color(WHITE).font_size(12);
        
        // Draw Large Glyph
        if let Some((ops, bounds)) = glyphs::glyph_ops_bounds(*glyph) {
             let big_rect = Rect::from_xy_wh(center, vec2(model.big_glyph_size, model.big_glyph_size));
             let path = build_path_for_glyph(*glyph, ops, bounds, big_rect, &model.tweaks, name);
             
             if model.fill {
                 draw.path().fill().events(path.iter()).color(WHITE);
             } else {
                 let join = match model.line_join {
                     LineJoin::Miter => nannou::lyon::tessellation::LineJoin::Miter,
                     LineJoin::Round => nannou::lyon::tessellation::LineJoin::Round,
                     LineJoin::Bevel => nannou::lyon::tessellation::LineJoin::Bevel,
                 };
                 let cap = match model.line_cap {
                     LineCap::Butt => nannou::lyon::tessellation::LineCap::Butt,
                     LineCap::Round => nannou::lyon::tessellation::LineCap::Round,
                     LineCap::Square => nannou::lyon::tessellation::LineCap::Square,
                 };
                 draw.path()
                    .stroke()
                    .weight(model.stroke_width)
                    .join(join)
                    .caps(cap)
                    .events(path.iter())
                    .color(WHITE);
             }

             // Draw Small Glyph
             let small_xy = center + vec2(model.cell_size * 0.35, model.cell_size * 0.35);
             let small_rect = Rect::from_xy_wh(small_xy, vec2(model.small_glyph_size, model.small_glyph_size));
             let small_path = build_path_for_glyph(*glyph, ops, bounds, small_rect, &model.tweaks, name);
             
             if model.fill {
                  draw.path().fill().events(small_path.iter()).color(WHITE);
             } else {
                  draw.path().stroke().weight(1.0).events(small_path.iter()).color(WHITE);
             }
        }
    }
    
    // Draw HUD

    let hud_text = format!(
        "FPS: {:.1} | Stroke: {:.1} | Tol: {:.2} | Join: {:?} | Cap: {:?} | Fill: {} | Size: {:.0}/{:.0}",
        app.fps(), model.stroke_width, model.tolerance, model.line_join, model.line_cap, model.fill, model.big_glyph_size, model.small_glyph_size
    );
    draw.text(&hud_text)
        .xy(win.top_left() + vec2(300.0, -20.0))
        .color(YELLOW)
        .font_size(16);

    draw.to_frame(app, &frame).unwrap();
}
