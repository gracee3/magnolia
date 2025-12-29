#[cfg(feature = "tile-rendering")]
use nannou::lyon::path::Path;
#[cfg(feature = "tile-rendering")]
use nannou::prelude::*;
use std::str::FromStr;

pub use talisman_ui::{FontId, GlyphOp, GlyphBounds, GlyphMetrics};
#[cfg(feature = "tile-rendering")]
pub use talisman_ui::{build_path, build_path_fit, font_glyph_ops_bounds};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Glyph {
    Sun, Moon, Mercury, Venus, Mars, Jupiter, Saturn, Uranus, Neptune, Pluto,
    Chiron, Lilith, NNode, SNode, Fortune,
    Aries, Taurus, Gemini, Cancer, Leo, Virgo, Libra, Scorpio, Sagittarius, Capricorn, Aquarius, Pisces,
    Ascendant, Descendant, MC, IC,
    House(u8),
    Unknown,
}

impl FromStr for Glyph {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
         match s {
            "Sun" => Ok(Glyph::Sun),
            "Moon" => Ok(Glyph::Moon),
            "Mercury" => Ok(Glyph::Mercury),
            "Venus" => Ok(Glyph::Venus),
            "Mars" => Ok(Glyph::Mars),
            "Jupiter" => Ok(Glyph::Jupiter),
            "Saturn" => Ok(Glyph::Saturn),
            "Uranus" => Ok(Glyph::Uranus),
            "Neptune" => Ok(Glyph::Neptune),
            "Pluto" => Ok(Glyph::Pluto),
            "Chiron" => Ok(Glyph::Chiron),
            "Lilith" => Ok(Glyph::Lilith),
            "NNode" => Ok(Glyph::NNode),
            "SNode" => Ok(Glyph::SNode),
            "Fortune" => Ok(Glyph::Fortune),
            "Aries" => Ok(Glyph::Aries),
            "Taurus" => Ok(Glyph::Taurus),
            "Gemini" => Ok(Glyph::Gemini),
            "Cancer" => Ok(Glyph::Cancer),
            "Leo" => Ok(Glyph::Leo),
            "Virgo" => Ok(Glyph::Virgo),
            "Libra" => Ok(Glyph::Libra),
            "Scorpio" => Ok(Glyph::Scorpio),
            "Sagittarius" => Ok(Glyph::Sagittarius),
            "Capricorn" => Ok(Glyph::Capricorn),
            "Aquarius" => Ok(Glyph::Aquarius),
            "Pisces" => Ok(Glyph::Pisces),
            "As" => Ok(Glyph::Ascendant),
            "Ds" => Ok(Glyph::Descendant),
            "Mc" => Ok(Glyph::MC),
            "Ic" => Ok(Glyph::IC),
            s if s.chars().all(|c| c.is_numeric()) => {
                let n = s.parse::<u8>().map_err(|_| ())?;
                if n >= 1 && n <= 12 { Ok(Glyph::House(n)) } else { Ok(Glyph::Unknown) }
            },
            _ => Ok(Glyph::Unknown),
        }
    }
}

#[cfg(feature = "tile-rendering")]
pub mod glyph_paths {
    include!(concat!(env!("OUT_DIR"), "/glyph_paths.rs"));
}

#[cfg(feature = "tile-rendering")]
pub fn glyph_path(font: FontId, c: char) -> Option<Path> {
    talisman_ui::glyph_path(font, c)
}

#[cfg(feature = "tile-rendering")]
pub fn glyph_metrics(font: FontId, c: char) -> Option<GlyphMetrics> {
    talisman_ui::glyph_metrics(font, c)
}

#[cfg(feature = "tile-rendering")]
pub fn glyph_ops_bounds(glyph: Glyph) -> Option<(&'static [GlyphOp], GlyphBounds)> {
    match glyph {
        Glyph::Sun => Some((glyph_paths::SUN_OPS, glyph_paths::SUN_BOUNDS)),
        Glyph::Moon => Some((glyph_paths::MOON_OPS, glyph_paths::MOON_BOUNDS)),
        Glyph::Mercury => Some((glyph_paths::MERCURY_OPS, glyph_paths::MERCURY_BOUNDS)),
        Glyph::Venus => Some((glyph_paths::VENUS_OPS, glyph_paths::VENUS_BOUNDS)),
        Glyph::Mars => Some((glyph_paths::MARS_OPS, glyph_paths::MARS_BOUNDS)),
        Glyph::Jupiter => Some((glyph_paths::JUPITER_OPS, glyph_paths::JUPITER_BOUNDS)),
        Glyph::Saturn => Some((glyph_paths::SATURN_OPS, glyph_paths::SATURN_BOUNDS)),
        Glyph::Uranus => Some((glyph_paths::URANUS_OPS, glyph_paths::URANUS_BOUNDS)),
        Glyph::Neptune => Some((glyph_paths::NEPTUNE_OPS, glyph_paths::NEPTUNE_BOUNDS)),
        Glyph::Pluto => Some((glyph_paths::PLUTO_OPS, glyph_paths::PLUTO_BOUNDS)),
        Glyph::Chiron => Some((glyph_paths::CHIRON_OPS, glyph_paths::CHIRON_BOUNDS)),
        Glyph::Lilith => Some((glyph_paths::BLACK_MOON_OPS, glyph_paths::BLACK_MOON_BOUNDS)),
        Glyph::NNode => Some((glyph_paths::NORTH_NODE_OPS, glyph_paths::NORTH_NODE_BOUNDS)),
        Glyph::SNode => Some((glyph_paths::SOUTH_NODE_OPS, glyph_paths::SOUTH_NODE_BOUNDS)),
        Glyph::Fortune => Some((glyph_paths::PART_OF_FORTUNE_OPS, glyph_paths::PART_OF_FORTUNE_BOUNDS)),
        Glyph::Aries => Some((glyph_paths::ARIES_OPS, glyph_paths::ARIES_BOUNDS)),
        Glyph::Taurus => Some((glyph_paths::TAURUS_OPS, glyph_paths::TAURUS_BOUNDS)),
        Glyph::Gemini => Some((glyph_paths::GEMINI_OPS, glyph_paths::GEMINI_BOUNDS)),
        Glyph::Cancer => Some((glyph_paths::CANCER_OPS, glyph_paths::CANCER_BOUNDS)),
        Glyph::Leo => Some((glyph_paths::LEO_OPS, glyph_paths::LEO_BOUNDS)),
        Glyph::Virgo => Some((glyph_paths::VIRGO_OPS, glyph_paths::VIRGO_BOUNDS)),
        Glyph::Libra => Some((glyph_paths::LIBRA_OPS, glyph_paths::LIBRA_BOUNDS)),
        Glyph::Scorpio => Some((glyph_paths::SCORPIO_OPS, glyph_paths::SCORPIO_BOUNDS)),
        Glyph::Sagittarius => Some((glyph_paths::SAGITTARIUS_OPS, glyph_paths::SAGITTARIUS_BOUNDS)),
        Glyph::Capricorn => Some((glyph_paths::CAPRICORN_OPS, glyph_paths::CAPRICORN_BOUNDS)),
        Glyph::Aquarius => Some((glyph_paths::AQUARIUS_OPS, glyph_paths::AQUARIUS_BOUNDS)),
        Glyph::Pisces => Some((glyph_paths::PISCES_OPS, glyph_paths::PISCES_BOUNDS)),
        Glyph::Ascendant => Some((glyph_paths::ASCENDANT_OPS, glyph_paths::ASCENDANT_BOUNDS)),
        Glyph::Descendant => Some((glyph_paths::DESCENDANT_OPS, glyph_paths::DESCENDANT_BOUNDS)),
        Glyph::MC => Some((glyph_paths::MC_OPS, glyph_paths::MC_BOUNDS)),
        Glyph::IC => Some((glyph_paths::IC_OPS, glyph_paths::IC_BOUNDS)),
        Glyph::House(n) => match n {
            1 => Some((glyph_paths::HOUSE_1_OPS, glyph_paths::HOUSE_1_BOUNDS)),
            2 => Some((glyph_paths::HOUSE_2_OPS, glyph_paths::HOUSE_2_BOUNDS)),
            3 => Some((glyph_paths::HOUSE_3_OPS, glyph_paths::HOUSE_3_BOUNDS)),
            4 => Some((glyph_paths::HOUSE_4_OPS, glyph_paths::HOUSE_4_BOUNDS)),
            5 => Some((glyph_paths::HOUSE_5_OPS, glyph_paths::HOUSE_5_BOUNDS)),
            6 => Some((glyph_paths::HOUSE_6_OPS, glyph_paths::HOUSE_6_BOUNDS)),
            7 => Some((glyph_paths::HOUSE_7_OPS, glyph_paths::HOUSE_7_BOUNDS)),
            8 => Some((glyph_paths::HOUSE_8_OPS, glyph_paths::HOUSE_8_BOUNDS)),
            9 => Some((glyph_paths::HOUSE_9_OPS, glyph_paths::HOUSE_9_BOUNDS)),
            10 => Some((glyph_paths::HOUSE_10_OPS, glyph_paths::HOUSE_10_BOUNDS)),
            11 => Some((glyph_paths::HOUSE_11_OPS, glyph_paths::HOUSE_11_BOUNDS)),
            12 => Some((glyph_paths::HOUSE_12_OPS, glyph_paths::HOUSE_12_BOUNDS)),
            _ => None,
        },
        Glyph::Unknown => None,
    }
}

#[cfg(feature = "tile-rendering")]
pub fn draw_glyph(draw: &Draw, glyph: Glyph, center: Point2, size: f32, color: Srgba, stroke_width: f32) {
    let rect = Rect::from_xy_wh(center, vec2(size, size));
    if let Some((ops, bounds)) = glyph_ops_bounds(glyph) {
        let path = build_path_fit(ops, bounds, rect);
        draw.path()
            .stroke()
            .weight(stroke_width)
            .color(color)
            .events(path.iter());
    } else {
        draw.ellipse()
            .xy(center)
            .radius(size / 2.0)
            .no_fill()
            .stroke_color(color)
            .stroke_weight(stroke_width);
    }
}
