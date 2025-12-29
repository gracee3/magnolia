use nannou::prelude::*;
use talisman_core::Patch;

/// Render a Bézier curve cable between two points
pub fn draw_cable(draw: &Draw, start: Vec2, end: Vec2, color: Srgb<u8>, thickness: f32) {
    // Calculate control points for a nice curve
    let control_offset = (end.x - start.x).abs() * 0.5;
    let control1 = pt2(start.x + control_offset, start.y);
    let control2 = pt2(end.x - control_offset, end.y);

    // Draw the curve using multiple small line segments
    let segments = 50;
    for i in 0..segments {
        let t1 = i as f32 / segments as f32;
        let t2 = (i + 1) as f32 / segments as f32;

        let p1 = cubic_bezier(start, control1, control2, end, t1);
        let p2 = cubic_bezier(start, control1, control2, end, t2);

        draw.line()
            .start(p1)
            .end(p2)
            .color(color)
            .stroke_weight(thickness);
    }
}

/// Cubic Bézier curve helper
fn cubic_bezier(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, t: f32) -> Vec2 {
    let u = 1.0 - t;
    let tt = t * t;
    let uu = u * u;
    let uuu = uu * u;
    let ttt = tt * t;

    let mut p = p0 * uuu;
    p += p1 * 3.0 * uu * t;
    p += p2 * 3.0 * u * tt;
    p += p3 * ttt;
    p
}

/// Render all patch cables
/// For now we'll render with a default color since we need module schemas to determine data types
pub fn render_patches(
    draw: &Draw,
    patches: &[Patch],
    tile_rects: &[(String, Rect)], // (module_id, rect)
) {
    for patch in patches {
        // Find source and sink tile rects
        let source_rect = tile_rects
            .iter()
            .find(|(id, _)| id == &patch.source_module)
            .map(|(_, rect)| rect);

        let sink_rect = tile_rects
            .iter()
            .find(|(id, _)| id == &patch.sink_module)
            .map(|(_, rect)| rect);

        if let (Some(src), Some(dst)) = (source_rect, sink_rect) {
            // Calculate connection points (right center of source, left center of sink)
            let start = pt2(src.right(), src.y());
            let end = pt2(dst.left(), dst.y());

            // Use a default color for now (we'll add signal type later)
            let color = rgb(150, 200, 255);

            draw_cable(draw, start, end, color, 2.0);

            // Draw connection dots
            draw.ellipse().xy(start).radius(4.0).color(color);

            draw.ellipse().xy(end).radius(4.0).color(color);
        }
    }
}
