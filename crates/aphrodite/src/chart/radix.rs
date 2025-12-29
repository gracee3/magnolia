use nannou::geom::Path;
use nannou::lyon::math::{point as lpoint, vector, Angle};
use nannou::lyon::path::builder::SvgPathBuilder;
use nannou::lyon::path::ArcFlags;
use nannou::prelude::*;

use super::data::ChartData;
use super::settings::ChartSettings;
use super::{get_point_position, parse_hex_color};
use crate::rendering::glyphs::{draw_glyph, Glyph};

pub struct RadixChart<'a> {
    settings: &'a ChartSettings,
    data: &'a ChartData,
    cx: f32,
    cy: f32,
    radius: f32,
    shift: f32,
}

impl<'a> RadixChart<'a> {
    pub fn new(
        cx: f32,
        cy: f32,
        radius: f32,
        data: &'a ChartData,
        settings: &'a ChartSettings,
    ) -> Self {
        // Calculate shift based on Ascendant (cusp[0])
        // AstroChart: shift = 180 - (ascendant + shift_in_degrees)
        // Wait, radix.ts: this.shift = 180 - (this.data.cusps[0] + this.settings.SHIFT_IN_DEGREES);
        let shift = 180.0 - (data.cusps[0] + settings.shift_in_degrees);

        Self {
            settings,
            data,
            cx,
            cy,
            radius,
            shift,
        }
    }

    pub fn draw_bg(&self, draw: &Draw) {
        if self.settings.stroke_only {
            return;
        }

        let c = parse_hex_color(&self.settings.color_background);
        let color = rgba(c.red, c.green, c.blue, 1.0); // Full opacity background?
                                                       // AstroChart draws a segment. Here we draw a circle for simplicity as base.
                                                       // Actually AstroChart draws a "hemisphere" segment.
                                                       // But implementation in radix.ts usually covers full circle 0-360.
                                                       // So ellipse is fine.
        draw.ellipse()
            .x_y(self.cx, self.cy)
            .radius(self.radius) // + radius/inner?
            // radix.ts: radius + radius/INNER.
            // Let's stick to base radius for now.
            .color(color);
    }

    pub fn draw_universe(&self, draw: &Draw) {
        let universe = vec![
            Glyph::Aries,
            Glyph::Taurus,
            Glyph::Gemini,
            Glyph::Cancer,
            Glyph::Leo,
            Glyph::Virgo,
            Glyph::Libra,
            Glyph::Scorpio,
            Glyph::Sagittarius,
            Glyph::Capricorn,
            Glyph::Aquarius,
            Glyph::Pisces,
        ];

        let sign_colors = &self.settings.sign_colors;
        let inner_r = self.radius / self.settings.indoor_circle_radius_ratio;
        let outer_r = self.radius; // Or radius + ...

        for (i, sign) in universe.iter().enumerate() {
            let start_angle = (i as f32) * 30.0;
            let end_angle = (i as f32 + 1.0) * 30.0;

            // Draw Sector
            let c = parse_hex_color(&sign_colors[i % 12]); // settings string array?
                                                           // settings.sign_colors is [String; 12].
            let color = rgba(c.red, c.green, c.blue, 1.0);

            self.draw_sector(draw, outer_r, inner_r, start_angle, end_angle, color);

            // Draw Glyph
            let mid_angle = (start_angle + end_angle) / 2.0;
            // Position: r = (outer + inner) / 2
            let r_glyph = (outer_r + inner_r) / 2.0;
            let pos = get_point_position(self.cx, self.cy, r_glyph, mid_angle, self.shift);

            let c_glyph = parse_hex_color(&self.settings.color_signs);
            let glyph_color = rgba(c_glyph.red, c_glyph.green, c_glyph.blue, 1.0);

            draw_glyph(
                draw,
                *sign,
                pos,
                self.settings.symbol_scale * 20.0,
                glyph_color,
            );
        }
    }

    fn draw_sector(
        &self,
        draw: &Draw,
        r_out: f32,
        r_in: f32,
        start_deg: f32,
        end_deg: f32,
        color: Srgba,
    ) {
        // Calculate points
        let p_start_out = get_point_position(self.cx, self.cy, r_out, start_deg, self.shift);
        let p_end_out = get_point_position(self.cx, self.cy, r_out, end_deg, self.shift);
        let p_end_in = get_point_position(self.cx, self.cy, r_in, end_deg, self.shift);
        let p_start_in = get_point_position(self.cx, self.cy, r_in, start_deg, self.shift);

        let mut builder = Path::builder().with_svg();

        // Move to start outer
        builder.move_to(lpoint(p_start_out.x, p_start_out.y));

        // Arc to end outer
        // Sweep true for CCW (180->210)
        builder.arc_to(
            vector(r_out, r_out),
            Angle::degrees(0.0),
            ArcFlags {
                large_arc: false,
                sweep: true,
            },
            lpoint(p_end_out.x, p_end_out.y),
        );

        // Line to end inner
        builder.line_to(lpoint(p_end_in.x, p_end_in.y));

        // Arc to start inner
        // Sweep false for CW (210->180)
        builder.arc_to(
            vector(r_in, r_in),
            Angle::degrees(0.0),
            ArcFlags {
                large_arc: false,
                sweep: false,
            },
            lpoint(p_start_in.x, p_start_in.y),
        );

        builder.close();
        let path = builder.build();

        draw.path().fill().color(color).events(path.iter());
    }

    pub fn draw_points(&self, draw: &Draw) {
        if self.data.planets.is_empty() {
            return;
        }

        let ruler_r =
            (self.radius / self.settings.inner_circle_radius_ratio) / self.settings.ruler_radius;
        let inner_ring_r = self.radius / self.settings.inner_circle_radius_ratio;
        let point_radius = self.radius
            - (inner_ring_r + 2.0 * ruler_r + (self.settings.padding * self.settings.symbol_scale));

        let c_pts = parse_hex_color(&self.settings.color_points);
        let color = rgba(c_pts.red, c_pts.green, c_pts.blue, 1.0); // usually white

        let c_lines = parse_hex_color(&self.settings.color_axis); // Pointer lines
        let color_lines = rgba(c_lines.red, c_lines.green, c_lines.blue, 1.0);

        for (glyph, planet_data) in &self.data.planets {
            let angle = planet_data.position;
            let pos = get_point_position(self.cx, self.cy, point_radius, angle, self.shift);

            // Draw simple tick/line to center or inner ring?
            // AstroChart draws a line from 'pointerRadius' to 'pointerRadius - rulerRadius/2'
            // pointerRadius = radius - (inner_ring_r + ruler_r)
            let ptr_r = self.radius - (inner_ring_r + ruler_r);
            let p_start = get_point_position(self.cx, self.cy, ptr_r, angle, self.shift);
            let p_end =
                get_point_position(self.cx, self.cy, ptr_r - ruler_r / 2.0, angle, self.shift);

            draw.line()
                .start(p_start)
                .end(p_end)
                .color(color_lines)
                .weight(self.settings.stroke_cusps * self.settings.symbol_scale);

            // Draw Glyph
            draw_glyph(
                draw,
                *glyph,
                pos,
                self.settings.symbol_scale * 12.0,
                color,
                self.settings.stroke_points,
            );
        }
    }

    pub fn draw_axis(&self, draw: &Draw) {
        if self.data.cusps.len() < 12 {
            return;
        }

        let c = parse_hex_color(&self.settings.color_axis);
        let color = rgba(c.red, c.green, c.blue, 1.0);

        // Indices for AS, IC, DS, MC
        let indices = [
            (0, Glyph::Ascendant),
            (3, Glyph::IC),
            (6, Glyph::Descendant),
            (9, Glyph::MC),
        ];

        let axis_radius =
            self.radius + ((self.radius / self.settings.inner_circle_radius_ratio) / 4.0);

        for (i, glyph) in indices.iter() {
            let angle = self.data.cusps[*i];
            let p_start = get_point_position(self.cx, self.cy, self.radius, angle, self.shift);
            let p_end = get_point_position(self.cx, self.cy, axis_radius, angle, self.shift);

            draw.line()
                .start(p_start)
                .end(p_end)
                .color(color)
                .weight(self.settings.stroke_axis * self.settings.symbol_scale);

            // Draw label
            // Offset text slightly outward
            let text_pos = get_point_position(
                self.cx,
                self.cy,
                axis_radius + (20.0 * self.settings.symbol_scale),
                angle,
                self.shift,
            );
            draw_glyph(
                draw,
                *glyph,
                text_pos,
                self.settings.symbol_scale * 14.0,
                color,
                self.settings.stroke_axis,
            );
        }
    }

    pub fn draw_cusps(&self, draw: &Draw) {
        if self.data.cusps.len() < 12 {
            return;
        }

        let c = parse_hex_color(&self.settings.color_cusps);
        let color = rgba(c.red, c.green, c.blue, 1.0);
        let stroke_weight = self.settings.stroke_cusps * self.settings.symbol_scale;

        let inner_r = self.radius / self.settings.indoor_circle_radius_ratio;
        let ruler_r =
            (self.radius / self.settings.inner_circle_radius_ratio) / self.settings.ruler_radius;
        let outer_limit =
            self.radius - (self.radius / self.settings.inner_circle_radius_ratio + ruler_r);

        for (i, angle) in self.data.cusps.iter().enumerate() {
            let p_start = get_point_position(self.cx, self.cy, inner_r, *angle, self.shift);
            let p_end = get_point_position(self.cx, self.cy, outer_limit, *angle, self.shift);

            draw.line()
                .start(p_start)
                .end(p_end)
                .color(color)
                .weight(stroke_weight);

            let num_r = inner_r + (self.settings.symbol_scale * 10.0);
            let next_angle = self.data.cusps[(i + 1) % 12];
            let diff = if next_angle < *angle {
                360.0 + next_angle - *angle
            } else {
                next_angle - *angle
            };
            let mid_angle = *angle + diff / 2.0;

            let num_pos = get_point_position(self.cx, self.cy, num_r, mid_angle, self.shift);

            draw_glyph(
                draw,
                Glyph::House((i + 1) as u8),
                num_pos,
                self.settings.symbol_scale * 10.0,
                color,
                1.0,
            );
        }
    }
}
