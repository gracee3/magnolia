use std::str::FromStr;
use crate::ephemeris::LayerPositions;
use crate::rendering::glyphs::Glyph;

#[derive(Debug, Clone)]
pub struct PlanetData {
    pub position: f32, // degrees 0-360
    pub speed: f32,
}

#[derive(Debug, Clone)]
pub struct ChartData {
    pub planets: Vec<(Glyph, PlanetData)>,
    pub cusps: Vec<f32>,
}

impl From<&LayerPositions> for ChartData {
    fn from(pos: &LayerPositions) -> Self {
        let mut planets = Vec::new();
        
        for (key, body) in &pos.planets {
            let glyph_str = match key.as_str() {
                "asc" => "As",
                "desc" => "Ds",
                "mc" => "Mc",
                "ic" => "Ic",
                s => s,
            };
            
            let cap_glyph_str = if glyph_str.len() > 1 {
                 let mut c = glyph_str.chars();
                 match c.next() {
                     None => String::new(),
                     Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                 }
            } else {
                glyph_str.to_string()
            };

            if let Ok(glyph) = Glyph::from_str(&cap_glyph_str) {
                 planets.push((glyph, PlanetData {
                     position: body.lon as f32,
                     speed: body.speed_lon as f32,
                 }));
            }
        }
        
        // Handle HousePositions
        let mut cusps = Vec::new();
        if let Some(houses) = &pos.houses {
             // Add Angles (Asc, Mc, etc) if not already in planets?
             // Usually they are separate.
             for (key, val) in &houses.angles {
                 let glyph = match key.to_lowercase().as_str() {
                     "asc" => Some(Glyph::Ascendant),
                     "mc" => Some(Glyph::MC),
                     "desc" | "dsc" => Some(Glyph::Descendant),
                     "ic" => Some(Glyph::IC),
                     _ => None,
                 };
                 if let Some(g) = glyph {
                     planets.push((g, PlanetData { position: *val as f32, speed: 0.0 }));
                 }
             }
             
             // Cusps 1..12
             let mut cups_vec = vec![0.0; 12];
             for (k, v) in &houses.cusps {
                 if let Ok(n) = k.parse::<usize>() {
                     if n >= 1 && n <= 12 {
                         cups_vec[n-1] = *v as f32;
                     }
                 }
             }
             cusps = cups_vec;
        } else {
            // Default cusps if missing? Or empty.
            cusps = vec![0.0; 12];
        }
        
        Self {
            planets,
            cusps,
        }
    }
}
