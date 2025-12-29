use crate::rendering::primitives::Color;
use std::collections::HashMap;

/// Visual styling configuration for chart elements
#[derive(Debug, Clone)]
pub struct VisualConfig {
    pub ring_width: Option<f32>,
    pub ring_spacing: Option<f32>,
    pub sign_colors: Vec<Color>,
    pub house_colors: Vec<Color>,
    pub planet_colors: HashMap<String, Color>,
    pub aspect_colors: HashMap<String, Color>,
    pub aspect_stroke_width: Option<f32>,
    pub background_color: Color,
    pub stroke_color: Color,
    pub stroke_width: Option<f32>,
}

impl Default for VisualConfig {
    fn default() -> Self {
        // Default traditional dark theme colors
        let sign_colors = vec![
            Color::from_hex("#DC143C").unwrap_or(Color::WHITE),      // Aries - crimson
            Color::from_hex("#8B4513").unwrap_or(Color::WHITE),      // Taurus - saddle brown
            Color::from_hex("#FFD700").unwrap_or(Color::WHITE),      // Gemini - gold
            Color::from_hex("#87CEEB").unwrap_or(Color::WHITE),      // Cancer - sky blue
            Color::from_hex("#FFA500").unwrap_or(Color::WHITE),      // Leo - orange
            Color::from_hex("#90EE90").unwrap_or(Color::WHITE),      // Virgo - light green
            Color::from_hex("#FFB6C1").unwrap_or(Color::WHITE),      // Libra - light pink
            Color::from_hex("#8B0000").unwrap_or(Color::WHITE),      // Scorpio - dark red
            Color::from_hex("#FFD700").unwrap_or(Color::WHITE),      // Sagittarius - gold
            Color::from_hex("#696969").unwrap_or(Color::WHITE),      // Capricorn - dim gray
            Color::from_hex("#00CED1").unwrap_or(Color::WHITE),      // Aquarius - dark turquoise
            Color::from_hex("#9370DB").unwrap_or(Color::WHITE),      // Pisces - medium purple
        ];

        let house_colors = vec![
            Color::from_hex("#2A2A2A").unwrap_or(Color::WHITE),      // House 1
            Color::from_hex("#333333").unwrap_or(Color::WHITE),      // House 2
            Color::from_hex("#3A3A3A").unwrap_or(Color::WHITE),      // House 3
            Color::from_hex("#404040").unwrap_or(Color::WHITE),      // House 4
            Color::from_hex("#474747").unwrap_or(Color::WHITE),      // House 5
            Color::from_hex("#4D4D4D").unwrap_or(Color::WHITE),      // House 6
            Color::from_hex("#2A2A2A").unwrap_or(Color::WHITE),      // House 7
            Color::from_hex("#333333").unwrap_or(Color::WHITE),      // House 8
            Color::from_hex("#3A3A3A").unwrap_or(Color::WHITE),      // House 9
            Color::from_hex("#404040").unwrap_or(Color::WHITE),      // House 10
            Color::from_hex("#474747").unwrap_or(Color::WHITE),      // House 11
            Color::from_hex("#4D4D4D").unwrap_or(Color::WHITE),      // House 12
        ];

        let mut planet_colors = HashMap::new();
        planet_colors.insert("sun".to_string(), Color::from_hex("#FFD700").unwrap_or(Color::WHITE));
        planet_colors.insert("moon".to_string(), Color::from_hex("#C0C0C0").unwrap_or(Color::WHITE));
        planet_colors.insert("mercury".to_string(), Color::from_hex("#8B7355").unwrap_or(Color::WHITE));
        planet_colors.insert("venus".to_string(), Color::from_hex("#FFC0CB").unwrap_or(Color::WHITE));
        planet_colors.insert("mars".to_string(), Color::from_hex("#DC143C").unwrap_or(Color::WHITE));
        planet_colors.insert("jupiter".to_string(), Color::from_hex("#FFA500").unwrap_or(Color::WHITE));
        planet_colors.insert("saturn".to_string(), Color::from_hex("#808080").unwrap_or(Color::WHITE));
        planet_colors.insert("uranus".to_string(), Color::from_hex("#87CEEB").unwrap_or(Color::WHITE));
        planet_colors.insert("neptune".to_string(), Color::from_hex("#4169E1").unwrap_or(Color::WHITE));
        planet_colors.insert("pluto".to_string(), Color::from_hex("#2F4F4F").unwrap_or(Color::WHITE));
        planet_colors.insert("chiron".to_string(), Color::from_hex("#8B7355").unwrap_or(Color::WHITE));
        planet_colors.insert("north_node".to_string(), Color::from_hex("#00CED1").unwrap_or(Color::WHITE));
        planet_colors.insert("south_node".to_string(), Color::from_hex("#00CED1").unwrap_or(Color::WHITE));

        let mut aspect_colors = HashMap::new();
        aspect_colors.insert("conjunction".to_string(), Color::from_hex("#DC143C").unwrap_or(Color::WHITE));
        aspect_colors.insert("opposition".to_string(), Color::from_hex("#4169E1").unwrap_or(Color::WHITE));
        aspect_colors.insert("trine".to_string(), Color::from_hex("#228B22").unwrap_or(Color::WHITE));
        aspect_colors.insert("square".to_string(), Color::from_hex("#FF0000").unwrap_or(Color::WHITE));
        aspect_colors.insert("sextile".to_string(), Color::from_hex("#FFA500").unwrap_or(Color::WHITE));

        Self {
            ring_width: Some(30.0),
            ring_spacing: Some(10.0),
            sign_colors,
            house_colors,
            planet_colors,
            aspect_colors,
            aspect_stroke_width: Some(2.0),
            background_color: Color::BLACK,
            stroke_color: Color::from_hex("#d4af37").unwrap_or(Color::WHITE), // Gold
            stroke_width: Some(1.0),
        }
    }
}

/// Glyph configuration
#[derive(Debug, Clone)]
pub struct GlyphConfig {
    pub sign_glyphs: HashMap<u8, String>,
    pub planet_glyphs: HashMap<String, String>,
    pub aspect_glyphs: HashMap<String, String>,
    pub glyph_size: Option<f32>,
    pub glyph_font: Option<String>,
}

impl Default for GlyphConfig {
    fn default() -> Self {
        let mut sign_glyphs = HashMap::new();
        sign_glyphs.insert(0, "♈".to_string());  // Aries
        sign_glyphs.insert(1, "♉".to_string());  // Taurus
        sign_glyphs.insert(2, "♊".to_string());  // Gemini
        sign_glyphs.insert(3, "♋".to_string());  // Cancer
        sign_glyphs.insert(4, "♌".to_string());  // Leo
        sign_glyphs.insert(5, "♍".to_string());  // Virgo
        sign_glyphs.insert(6, "♎".to_string());  // Libra
        sign_glyphs.insert(7, "♏".to_string());  // Scorpio
        sign_glyphs.insert(8, "♐".to_string());  // Sagittarius
        sign_glyphs.insert(9, "♑".to_string());  // Capricorn
        sign_glyphs.insert(10, "♒".to_string()); // Aquarius
        sign_glyphs.insert(11, "♓".to_string()); // Pisces

        let mut planet_glyphs = HashMap::new();
        planet_glyphs.insert("sun".to_string(), "☉".to_string());
        planet_glyphs.insert("moon".to_string(), "☽".to_string());
        planet_glyphs.insert("mercury".to_string(), "☿".to_string());
        planet_glyphs.insert("venus".to_string(), "♀".to_string());
        planet_glyphs.insert("mars".to_string(), "♂".to_string());
        planet_glyphs.insert("jupiter".to_string(), "♃".to_string());
        planet_glyphs.insert("saturn".to_string(), "♄".to_string());
        planet_glyphs.insert("uranus".to_string(), "♅".to_string());
        planet_glyphs.insert("neptune".to_string(), "♆".to_string());
        planet_glyphs.insert("pluto".to_string(), "♇".to_string());
        planet_glyphs.insert("chiron".to_string(), "⚷".to_string());
        planet_glyphs.insert("north_node".to_string(), "☊".to_string());
        planet_glyphs.insert("south_node".to_string(), "☋".to_string());

        Self {
            sign_glyphs,
            planet_glyphs,
            aspect_glyphs: HashMap::new(),
            glyph_size: Some(12.0),
            glyph_font: None,
        }
    }
}

