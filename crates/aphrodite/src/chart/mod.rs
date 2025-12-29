pub mod animation;
pub mod data;
pub mod radix;
pub mod settings;
pub mod transit;

pub use animation::ChartAnimation;
pub use data::ChartData;
pub use radix::RadixChart;
pub use settings::ChartSettings;
pub use transit::TransitChart;

use nannou::prelude::*;

pub fn get_point_position(
    cx: f32,
    cy: f32,
    radius: f32,
    angle_degrees: f32,
    shift_degrees: f32,
) -> Point2 {
    let angle_rad = (shift_degrees + angle_degrees).to_radians();
    let x = cx + radius * angle_rad.cos();
    let y = cy + radius * angle_rad.sin();
    vec2(x, y)
}

pub fn parse_hex_color(hex: &str) -> Srgb {
    let hex = hex.trim_start_matches('#');
    if hex.len() == 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
        Srgb::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
    } else {
        Srgb::new(0.0, 0.0, 0.0)
    }
}
