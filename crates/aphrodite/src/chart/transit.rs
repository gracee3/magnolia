use nannou::prelude::*;
use super::settings::ChartSettings;
use super::data::ChartData;
use super::{get_point_position, parse_hex_color};
use crate::rendering::glyphs::{draw_glyph, Glyph};

pub struct TransitChart<'a> {
    settings: &'a ChartSettings,
    data: &'a ChartData,
    cx: f32,
    cy: f32,
    radix_radius: f32,
    shift: f32,
}

impl<'a> TransitChart<'a> {
    pub fn new(cx: f32, cy: f32, radix_radius: f32, shift: f32, data: &'a ChartData, settings: &'a ChartSettings) -> Self {
        Self {
            settings,
            data,
            cx,
            cy,
            radix_radius,
            shift,
        }
    }

    pub fn draw_points(&self, draw: &Draw) {
        if self.data.planets.is_empty() { return; }

        let inner_ratio = self.settings.inner_circle_radius_ratio;
        // Transit points are OUTSIDE the radix radius
        // TS: pointRadius = radius + (radius / inner_ratio + padding * scale)
        let transit_scale = self.settings.transit_symbol_scale * self.settings.symbol_scale;
        let point_radius = self.radix_radius + (self.radix_radius / inner_ratio + (self.settings.padding * transit_scale));
        
        // Use transit-specific color (distinct from natal)
        let c_pts = parse_hex_color(&self.settings.transit_color_points);
        let color = rgba(c_pts.red, c_pts.green, c_pts.blue, 1.0);
        
        let c_lines = parse_hex_color(&self.settings.color_axis);
        let color_lines = rgba(c_lines.red, c_lines.green, c_lines.blue, 1.0);
        
        // Pointer Radius
        let pointer_radius = self.radix_radius + (self.radix_radius / inner_ratio);
        let ruler_r = (self.radix_radius / inner_ratio) / self.settings.ruler_radius;

        for (glyph, planet_data) in &self.data.planets {
            let angle = planet_data.position;
            let pos = get_point_position(self.cx, self.cy, point_radius, angle, self.shift);
            
            // Pointer Line
            let p_start = get_point_position(self.cx, self.cy, pointer_radius, angle, self.shift);
            let p_end = get_point_position(self.cx, self.cy, pointer_radius + ruler_r/2.0, angle, self.shift);
            
            draw.line()
                .start(p_start)
                .end(p_end)
                .color(color_lines)
                .weight(self.settings.stroke_cusps * transit_scale);
                
            // Draw Glyph (using transit scale for differentiation)
            draw_glyph(draw, *glyph, pos, transit_scale * 12.0, color);
        }
    }
    
    pub fn draw_cusps(&self, draw: &Draw) {
        if self.data.cusps.len() < 12 { return; }
        
        let c = parse_hex_color(&self.settings.color_cusps);
        let color = rgba(c.red, c.green, c.blue, 1.0);
        let stroke_weight = self.settings.stroke_cusps * self.settings.symbol_scale;
        
        let inner_ratio = self.settings.inner_circle_radius_ratio;
        let ruler_r = (self.radix_radius / inner_ratio) / self.settings.ruler_radius;
        
        // TS: Cusp lines for transit
        // start = radius
        // end = radius + radius/indoor - ruler
        // Wait, TS says radius + radius/inner - ruler
        let p_start_r = self.radix_radius;
        let p_end_r = self.radix_radius + (self.radix_radius / inner_ratio) - ruler_r;
        
        // Numbers Radius
        let num_r = self.radix_radius + ((self.radix_radius / inner_ratio - ruler_r) / 2.0);

        for (i, angle) in self.data.cusps.iter().enumerate() {
            let p_start = get_point_position(self.cx, self.cy, p_start_r, *angle, self.shift);
            let p_end = get_point_position(self.cx, self.cy, p_end_r, *angle, self.shift);
            
            draw.line().start(p_start).end(p_end).color(color).weight(stroke_weight);
            
            // Number
            let next_angle = self.data.cusps[(i + 1) % 12];
            let diff = if next_angle < *angle { 360.0 + next_angle - *angle } else { next_angle - *angle };
            let mid_angle = *angle + diff / 2.0;
            
            let num_pos = get_point_position(self.cx, self.cy, num_r, mid_angle, self.shift);
            draw_glyph(draw, Glyph::House((i + 1) as u8), num_pos, self.settings.symbol_scale * 10.0, color);
        }
    }
}
