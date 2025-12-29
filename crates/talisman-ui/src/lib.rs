#[cfg(feature = "tile-rendering")]
use nannou::lyon::math::point as lpoint;
#[cfg(feature = "tile-rendering")]
use nannou::lyon::path::Path;
#[cfg(feature = "tile-rendering")]
use nannou::prelude::*;

pub mod theme;
pub mod tweaks;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontId {
    PlexSansRegular,
    PlexSansBold,
    PlexMonoRegular,
    PlexMonoMedium,
}

#[derive(Clone, Copy, Debug)]
pub enum GlyphOp {
    M(f32, f32),
    L(f32, f32),
    Q(f32, f32, f32, f32),
    C(f32, f32, f32, f32, f32, f32),
    Z,
}

#[derive(Clone, Copy, Debug)]
pub struct GlyphBounds {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct GlyphMetrics {
    pub advance_width: f32,
    pub left_side_bearing: f32,
}

#[cfg(feature = "tile-rendering")]
pub mod plex_sans_regular {
    include!(concat!(env!("OUT_DIR"), "/plex_sans_regular_ops.rs"));
}
#[cfg(feature = "tile-rendering")]
pub mod plex_sans_bold {
    include!(concat!(env!("OUT_DIR"), "/plex_sans_bold_ops.rs"));
}
#[cfg(feature = "tile-rendering")]
pub mod plex_mono_regular {
    include!(concat!(env!("OUT_DIR"), "/plex_mono_regular_ops.rs"));
}
#[cfg(feature = "tile-rendering")]
pub mod plex_mono_medium {
    include!(concat!(env!("OUT_DIR"), "/plex_mono_medium_ops.rs"));
}

#[cfg(feature = "tile-rendering")]
pub fn build_path(ops: &[GlyphOp]) -> Path {
    let mut builder = Path::builder();
    let mut open = false;
    for op in ops {
        match *op {
            GlyphOp::M(x, y) => {
                if open {
                    builder.end(false);
                }
                builder.begin(lpoint(x, y));
                open = true;
            }
            GlyphOp::L(x, y) => {
                if !open {
                    builder.begin(lpoint(x, y));
                    open = true;
                }
                builder.line_to(lpoint(x, y));
            }
            GlyphOp::Q(x1, y1, x, y) => {
                if !open {
                    builder.begin(lpoint(x, y));
                    open = true;
                }
                builder.quadratic_bezier_to(lpoint(x1, y1), lpoint(x, y));
            }
            GlyphOp::C(x1, y1, x2, y2, x, y) => {
                if !open {
                    builder.begin(lpoint(x, y));
                    open = true;
                }
                builder.cubic_bezier_to(lpoint(x1, y1), lpoint(x2, y2), lpoint(x, y));
            }
            GlyphOp::Z => {
                if open {
                    builder.end(true);
                    open = false;
                }
            }
        }
    }
    if open {
        builder.end(false);
    }
    builder.build()
}

#[cfg(feature = "tile-rendering")]
pub fn glyph_path(font: FontId, c: char) -> Option<Path> {
    match font {
        FontId::PlexSansRegular => plex_sans_regular::ops_for_ascii(c).map(build_path),
        FontId::PlexSansBold => plex_sans_bold::ops_for_ascii(c).map(build_path),
        FontId::PlexMonoRegular => plex_mono_regular::ops_for_ascii(c).map(build_path),
        FontId::PlexMonoMedium => plex_mono_medium::ops_for_ascii(c).map(build_path),
    }
}

#[cfg(feature = "tile-rendering")]
pub fn glyph_metrics(font: FontId, c: char) -> Option<GlyphMetrics> {
    match font {
        FontId::PlexSansRegular => plex_sans_regular::metrics_for_ascii(c),
        FontId::PlexSansBold => plex_sans_bold::metrics_for_ascii(c),
        FontId::PlexMonoRegular => plex_mono_regular::metrics_for_ascii(c),
        FontId::PlexMonoMedium => plex_mono_medium::metrics_for_ascii(c),
    }
}

#[cfg(feature = "tile-rendering")]
pub fn font_glyph_ops_bounds(font: FontId, c: char) -> Option<(&'static [GlyphOp], GlyphBounds)> {
    match font {
        FontId::PlexSansRegular => {
            if let (Some(ops), Some(bounds)) = (plex_sans_regular::ops_for_ascii(c), plex_sans_regular::bounds_for_ascii(c)) {
                Some((ops, bounds))
            } else {
                None
            }
        },
        FontId::PlexSansBold => {
            if let (Some(ops), Some(bounds)) = (plex_sans_bold::ops_for_ascii(c), plex_sans_bold::bounds_for_ascii(c)) {
                Some((ops, bounds))
            } else {
                None
            }
        },
        FontId::PlexMonoRegular => {
            if let (Some(ops), Some(bounds)) = (plex_mono_regular::ops_for_ascii(c), plex_mono_regular::bounds_for_ascii(c)) {
                Some((ops, bounds))
            } else {
                None
            }
        },
        FontId::PlexMonoMedium => {
            if let (Some(ops), Some(bounds)) = (plex_mono_medium::ops_for_ascii(c), plex_mono_medium::bounds_for_ascii(c)) {
                Some((ops, bounds))
            } else {
                None
            }
        },
    }
}

#[cfg(feature = "tile-rendering")]
pub fn build_path_fit(ops: &[GlyphOp], bounds: GlyphBounds, rect: Rect) -> Path {
    let width = bounds.max_x - bounds.min_x;
    let height = bounds.max_y - bounds.min_y;
    if width <= 0.0 || height <= 0.0 {
        return build_path(ops);
    }

    let s = (rect.w() / width).min(rect.h() / height);
    let bounds_cx = (bounds.min_x + bounds.max_x) * 0.5;
    let bounds_cy = (bounds.min_y + bounds.max_y) * 0.5;
    let center = rect.xy();
    let tx = center.x - bounds_cx * s;
    let ty = center.y - bounds_cy * s;

    let mut builder = Path::builder();
    let mut open = false;
    for op in ops {
        match *op {
            GlyphOp::M(x, y) => {
                if open {
                    builder.end(false);
                }
                builder.begin(lpoint(x * s + tx, y * s + ty));
                open = true;
            }
            GlyphOp::L(x, y) => {
                if !open {
                    builder.begin(lpoint(x * s + tx, y * s + ty));
                    open = true;
                }
                builder.line_to(lpoint(x * s + tx, y * s + ty));
            }
            GlyphOp::Q(x1, y1, x, y) => {
                if !open {
                    builder.begin(lpoint(x * s + tx, y * s + ty));
                    open = true;
                }
                builder.quadratic_bezier_to(
                    lpoint(x1 * s + tx, y1 * s + ty),
                    lpoint(x * s + tx, y * s + ty),
                );
            }
            GlyphOp::C(x1, y1, x2, y2, x, y) => {
                if !open {
                    builder.begin(lpoint(x * s + tx, y * s + ty));
                    open = true;
                }
                builder.cubic_bezier_to(
                    lpoint(x1 * s + tx, y1 * s + ty),
                    lpoint(x2 * s + tx, y2 * s + ty),
                    lpoint(x * s + tx, y * s + ty),
                );
            }
            GlyphOp::Z => {
                if open {
                    builder.end(true);
                    open = false;
                }
            }
        }
    }
    if open {
        builder.end(false);
    }
    builder.build()
}

#[cfg(feature = "tile-rendering")]
pub fn draw_glyph(draw: &Draw, ops: &[GlyphOp], bounds: GlyphBounds, center: Point2, size: f32, color: Srgba) {
    let rect = Rect::from_xy_wh(center, vec2(size, size));
    let path = build_path_fit(ops, bounds, rect);
    draw.path()
        .fill()
        .color(color)
        .events(path.iter());
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlignment {
    Left,
    Center,
    Right,
}

#[cfg(feature = "tile-rendering")]
pub fn text_width(font: FontId, text: &str, size: f32) -> f32 {
    let mut width = 0.0;
    for c in text.chars() {
        if let Some(metrics) = glyph_metrics(font, c) {
            width += metrics.advance_width * size;
        } else if c == ' ' {
            width += 0.3 * size;
        }
    }
    width
}

#[cfg(feature = "tile-rendering")]
pub fn draw_text(
    draw: &Draw,
    font: FontId,
    text: &str,
    pos: Point2,
    size: f32,
    color: Srgba,
    align: TextAlignment,
) {
    let total_width = text_width(font, text, size);
    let mut x_offset = match align {
        TextAlignment::Left => 0.0,
        TextAlignment::Center => -total_width / 2.0,
        TextAlignment::Right => -total_width,
    };

    for c in text.chars() {
        if let Some((ops, bounds)) = font_glyph_ops_bounds(font, c) {
            if let Some(metrics) = glyph_metrics(font, c) {
                let glyph_center = pt2(pos.x + x_offset + metrics.advance_width * size * 0.5, pos.y);
                let rect = Rect::from_xy_wh(glyph_center, vec2(size, size));
                let path = build_path_fit(ops, bounds, rect);
                draw.path().fill().color(color).events(path.iter());
                x_offset += metrics.advance_width * size;
            }
        } else if c == ' ' {
            x_offset += 0.3 * size;
        }
    }
}
