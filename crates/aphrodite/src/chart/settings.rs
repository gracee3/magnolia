use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChartSettings {
    // Layout
    pub margin: f32,
    pub padding: f32,
    pub symbol_scale: f32,

    // Theme Colors (Dark Mode Defaults)
    pub color_background: String,
    pub color_points: String,
    pub color_circles: String,
    pub color_lines: String,
    pub color_axis: String,
    pub color_cusps: String,
    pub color_signs: String, // Default fallback

    // Stroke Widths
    pub stroke_points: f32,
    pub stroke_signs: f32,
    pub stroke_circles: f32,
    pub stroke_axis: f32,
    pub stroke_cusps: f32,

    // Radii Ratios
    pub indoor_circle_radius_ratio: f32,
    pub inner_circle_radius_ratio: f32,
    pub ruler_radius: f32,

    // Zodiac Colors (Modern Rainbow)
    pub sign_colors: Vec<String>,

    // Aspects
    pub aspect_colors: AspectColors,

    // Options
    pub shift_in_degrees: f32,
    pub stroke_only: bool,

    pub show_dignities: bool,

    // Transit overlay styling
    pub transit_color_points: String,
    pub transit_symbol_scale: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AspectColors {
    pub conjunction: String,
    pub square: String,
    pub trine: String,
    pub opposition: String,
}

impl Default for ChartSettings {
    fn default() -> Self {
        Self {
            margin: 50.0,
            padding: 18.0,
            symbol_scale: 1.0,

            // Traditional Dark Theme
            color_background: "#1a1a1a".to_string(), // Dark Gray/Black
            color_points: "#eeeeee".to_string(),     // Near White
            color_circles: "#444444".to_string(),    // Dark Gray
            color_lines: "#444444".to_string(),
            color_axis: "#eeeeee".to_string(),
            color_cusps: "#cccccc".to_string(),
            color_signs: "#ffffff".to_string(),

            stroke_points: 1.8,
            stroke_signs: 1.5,
            stroke_circles: 2.0,
            stroke_axis: 1.6,
            stroke_cusps: 1.0,

            indoor_circle_radius_ratio: 2.0,
            inner_circle_radius_ratio: 8.0,
            ruler_radius: 4.0,

            // Modern Rainbow Zodiac
            sign_colors: vec![
                "#FF4500".to_string(), // Aries (Red-Orange)
                "#8B4513".to_string(), // Taurus (Brown)
                "#87CEEB".to_string(), // Gemini (Sky Blue)
                "#27AE60".to_string(), // Cancer (Green)
                "#FFD700".to_string(), // Leo (Gold) - Changed from Red for rainbow
                "#9ACD32".to_string(), // Virgo (YellowGreen)
                "#FF69B4".to_string(), // Libra (HotPink)
                "#8B0000".to_string(), // Scorpio (DarkRed)
                "#800080".to_string(), // Sagittarius (Purple)
                "#708090".to_string(), // Capricorn (SlateGray)
                "#00FFFF".to_string(), // Aquarius (Cyan)
                "#2E8B57".to_string(), // Pisces (SeaGreen)
            ],

            aspect_colors: AspectColors {
                conjunction: "transparent".to_string(),
                square: "#FF4500".to_string(),     // OrangeRed
                trine: "#27AE60".to_string(),      // Green
                opposition: "#FF0000".to_string(), // Red
            },

            shift_in_degrees: 180.0, // 0 is West
            stroke_only: false,
            show_dignities: true,

            // Transit overlay styling (distinct from natal)
            transit_color_points: "#FF8C00".to_string(), // DarkOrange
            transit_symbol_scale: 0.85,                  // Slightly smaller than natal
        }
    }
}
