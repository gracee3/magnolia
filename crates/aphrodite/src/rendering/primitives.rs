use serde::{Deserialize, Serialize};

/// Point in 2D space
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// Color in RGBA format
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const WHITE: Color = Color {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };

    pub const BLACK: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };

    /// Create color from hex string (e.g., "#FF0000" or "#FF0000FF")
    pub fn from_hex(hex: &str) -> Option<Self> {
        let hex = hex.trim_start_matches('#');
        if hex.len() == 6 {
            // RGB
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(Color { r, g, b, a: 255 })
        } else if hex.len() == 8 {
            // RGBA
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            Some(Color { r, g, b, a })
        } else {
            None
        }
    }

    /// Convert to CSS string
    pub fn to_css_string(&self) -> String {
        if self.a == 255 {
            format!("rgb({}, {}, {})", self.r, self.g, self.b)
        } else {
            format!(
                "rgba({}, {}, {}, {})",
                self.r,
                self.g,
                self.b,
                self.a as f32 / 255.0
            )
        }
    }
}

/// Stroke style
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stroke {
    pub color: Color,
    pub width: f32,
    pub dash_array: Option<Vec<f32>>,
}

/// Text anchor position
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TextAnchor {
    Start,
    Middle,
    End,
}

/// Line style
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum LineStyle {
    Solid,
    Dashed,
    Dotted,
}

/// Shape primitives for chart rendering
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Shape {
    Circle {
        center: Point,
        radius: f32,
        fill: Option<Color>,
        stroke: Option<Stroke>,
    },
    Arc {
        center: Point,
        radius_inner: f32,
        radius_outer: f32,
        start_angle: f32, // degrees, 0 = top, clockwise
        end_angle: f32,
        fill: Option<Color>,
        stroke: Option<Stroke>,
    },
    Line {
        from: Point,
        to: Point,
        stroke: Stroke,
    },
    Path {
        points: Vec<Point>,
        closed: bool,
        fill: Option<Color>,
        stroke: Option<Stroke>,
    },
    Text {
        position: Point,
        content: String,
        size: f32,
        color: Color,
        anchor: TextAnchor,
        rotation: Option<f32>, // degrees
    },
    PlanetGlyph {
        center: Point,
        planet_id: String,
        size: f32,
        color: Color,
        retrograde: bool,
    },
    AspectLine {
        from: Point,
        to: Point,
        aspect_type: String, // "conjunction", "trine", etc.
        color: Color,
        width: f32,
        style: LineStyle,
    },
    HouseSegment {
        center: Point,
        house_num: u8,
        start_angle: f32,
        end_angle: f32,
        radius_inner: f32,
        radius_outer: f32,
        fill: Color,
        stroke: Option<Stroke>,
    },
    SignSegment {
        center: Point,
        sign_index: u8, // 0-11
        start_angle: f32,
        end_angle: f32,
        radius_inner: f32,
        radius_outer: f32,
        fill: Color,
        stroke: Option<Stroke>,
    },
}

