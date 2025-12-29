use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;

use serde::Deserialize;
use ttf_parser::{Face, OutlineBuilder};

#[derive(Debug, Deserialize)]
struct GlyphMap {
    planets: Option<HashMap<String, String>>,
    asteroids: Option<HashMap<String, String>>,
    nodes: Option<HashMap<String, String>>,
    lilith: Option<HashMap<String, String>>,
    signs: Option<HashMap<String, String>>,
    houses: Option<HashMap<String, String>>,
    angles: Option<HashMap<String, String>>,
    lots: Option<HashMap<String, String>>,
    aspects: Option<HashMap<String, String>>,
    aliases: Option<HashMap<String, String>>,
}

impl GlyphMap {
    fn get(&self, section: &str, key: &str) -> Option<&str> {
        match section {
            "planets" => self.planets.as_ref()?.get(key).map(String::as_str),
            "asteroids" => self.asteroids.as_ref()?.get(key).map(String::as_str),
            "nodes" => self.nodes.as_ref()?.get(key).map(String::as_str),
            "lilith" => self.lilith.as_ref()?.get(key).map(String::as_str),
            "signs" => self.signs.as_ref()?.get(key).map(String::as_str),
            "houses" => self.houses.as_ref()?.get(key).map(String::as_str),
            "angles" => self.angles.as_ref()?.get(key).map(String::as_str),
            "lots" => self.lots.as_ref()?.get(key).map(String::as_str),
            "aspects" => self.aspects.as_ref()?.get(key).map(String::as_str),
            "aliases" => self.aliases.as_ref()?.get(key).map(String::as_str),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Op {
    M(f32, f32),
    L(f32, f32),
    Q(f32, f32, f32, f32),
    C(f32, f32, f32, f32, f32, f32),
    Z,
}

#[derive(Clone, Copy, Debug)]
struct Bounds {
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
    has_bounds: bool,
}

impl Bounds {
    fn new() -> Self {
        Self {
            min_x: 0.0,
            min_y: 0.0,
            max_x: 0.0,
            max_y: 0.0,
            has_bounds: false,
        }
    }

    fn update(&mut self, x: f32, y: f32) {
        if !self.has_bounds {
            self.min_x = x;
            self.max_x = x;
            self.min_y = y;
            self.max_y = y;
            self.has_bounds = true;
            return;
        }
        if x < self.min_x {
            self.min_x = x;
        }
        if x > self.max_x {
            self.max_x = x;
        }
        if y < self.min_y {
            self.min_y = y;
        }
        if y > self.max_y {
            self.max_y = y;
        }
    }
}

struct OutlineCollector {
    ops: Vec<Op>,
    bounds: Bounds,
    scale: f32,
    flip_y: bool,
}

impl OutlineCollector {
    fn new(scale: f32, flip_y: bool) -> Self {
        Self {
            ops: Vec::new(),
            bounds: Bounds::new(),
            scale,
            flip_y,
        }
    }

    fn norm(&self, x: f32, y: f32) -> (f32, f32) {
        let mut ny = y * self.scale;
        if self.flip_y {
            ny = -ny;
        }
        (x * self.scale, ny)
    }

    fn note(&mut self, x: f32, y: f32) {
        self.bounds.update(x, y);
    }
}

impl OutlineBuilder for OutlineCollector {
    fn move_to(&mut self, x: f32, y: f32) {
        let (nx, ny) = self.norm(x, y);
        self.ops.push(Op::M(nx, ny));
        self.note(nx, ny);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let (nx, ny) = self.norm(x, y);
        self.ops.push(Op::L(nx, ny));
        self.note(nx, ny);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (nx1, ny1) = self.norm(x1, y1);
        let (nx, ny) = self.norm(x, y);
        self.ops.push(Op::Q(nx1, ny1, nx, ny));
        self.note(nx1, ny1);
        self.note(nx, ny);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (nx1, ny1) = self.norm(x1, y1);
        let (nx2, ny2) = self.norm(x2, y2);
        let (nx, ny) = self.norm(x, y);
        self.ops.push(Op::C(nx1, ny1, nx2, ny2, nx, ny));
        self.note(nx1, ny1);
        self.note(nx2, ny2);
        self.note(nx, ny);
    }

    fn close(&mut self) {
        self.ops.push(Op::Z);
    }
}

struct GlyphSpec {
    const_name: String,
    func_name: String,
    section: &'static str,
    key: String,
}

fn fmt_f32(value: f32) -> String {
    let mut s = format!("{:.6}", value);
    if s == "-0.000000" {
        s = "0.0".to_string();
    }
    while s.contains('.') && s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.push('0');
    }
    if s == "-0" {
        s = "0.0".to_string();
    }
    s
}

fn generate_astronomicon(manifest_dir: &PathBuf, out_dir: &PathBuf) {
    let font_path = manifest_dir.join("../../assets/fonts/Astronomicon.ttf");
    let map_path = manifest_dir.join("../../assets/fonts/glyph_map.toml");

    println!("cargo:rerun-if-changed={}", font_path.display());
    println!("cargo:rerun-if-changed={}", map_path.display());

    let font_bytes = fs::read(&font_path).expect("Failed to read Astronomicon.ttf");
    let face = Face::parse(&font_bytes, 0).expect("Failed to parse Astronomicon.ttf");
    let units_per_em = face.units_per_em();
    let scale = 1.0 / units_per_em as f32;
    let flip_y = false;

    let map_src = fs::read_to_string(&map_path).expect("Failed to read glyph_map.toml");
    let glyph_map: GlyphMap = toml::from_str(&map_src).expect("Failed to parse glyph_map.toml");

    let mut specs = Vec::new();
    let mut push_spec = |const_name: &str, func_name: &str, section: &'static str, key: &str| {
        specs.push(GlyphSpec {
            const_name: const_name.to_string(),
            func_name: func_name.to_string(),
            section,
            key: key.to_string(),
        });
    };

    push_spec("SUN", "sun_path", "planets", "sun");
    push_spec("MOON", "moon_path", "planets", "moon");
    push_spec("MERCURY", "mercury_path", "planets", "mercury");
    push_spec("VENUS", "venus_path", "planets", "venus");
    push_spec("MARS", "mars_path", "planets", "mars");
    push_spec("JUPITER", "jupiter_path", "planets", "jupiter");
    push_spec("SATURN", "saturn_path", "planets", "saturn");
    push_spec("URANUS", "uranus_path", "planets", "uranus");
    push_spec("NEPTUNE", "neptune_path", "planets", "neptune");
    push_spec("PLUTO", "pluto_path", "planets", "pluto");

    push_spec("CHIRON", "chiron_path", "asteroids", "chiron");

    push_spec("NORTH_NODE", "north_node_path", "nodes", "north_node");
    push_spec("SOUTH_NODE", "south_node_path", "nodes", "south_node");

    push_spec("BLACK_MOON", "black_moon_path", "lilith", "black_moon");

    push_spec("ARIES", "aries_path", "signs", "aries");
    push_spec("TAURUS", "taurus_path", "signs", "taurus");
    push_spec("GEMINI", "gemini_path", "signs", "gemini");
    push_spec("CANCER", "cancer_path", "signs", "cancer");
    push_spec("LEO", "leo_path", "signs", "leo");
    push_spec("VIRGO", "virgo_path", "signs", "virgo");
    push_spec("LIBRA", "libra_path", "signs", "libra");
    push_spec("SCORPIO", "scorpio_path", "signs", "scorpio");
    push_spec("SAGITTARIUS", "sagittarius_path", "signs", "sagittarius");
    push_spec("CAPRICORN", "capricorn_path", "signs", "capricorn");
    push_spec("AQUARIUS", "aquarius_path", "signs", "aquarius");
    push_spec("PISCES", "pisces_path", "signs", "pisces");

    push_spec("ASCENDANT", "ascendant_path", "angles", "ac");
    push_spec("DESCENDANT", "descendant_path", "angles", "dc");
    push_spec("MC", "mc_path", "angles", "mc");
    push_spec("IC", "ic_path", "angles", "ic");

    push_spec(
        "PART_OF_FORTUNE",
        "part_of_fortune_path",
        "lots",
        "part_of_fortune",
    );

    for n in 1..=12 {
        let const_name = format!("HOUSE_{}", n);
        let func_name = format!("house_{}_path", n);
        let key = format!("h{}", n);
        specs.push(GlyphSpec {
            const_name,
            func_name,
            section: "houses",
            key,
        });
    }

    let out_path = out_dir.join("glyph_paths.rs");
    let mut out = String::new();

    // Header - we now reuse types from crate::rendering::glyphs
    out.push_str("// @generated by build.rs - do not edit\n");
    out.push_str("use nannou::lyon::path::Path;\n");
    out.push_str("use crate::rendering::glyphs::{GlyphOp, GlyphBounds, build_path};\n\n");

    for spec in specs {
        let value = glyph_map.get(spec.section, &spec.key).unwrap_or_else(|| {
            panic!(
                "Missing mapping for {}.{} in glyph_map.toml",
                spec.section, spec.key
            )
        });
        let ch = value.chars().next().unwrap_or_else(|| {
            panic!(
                "Mapping for {}.{} is empty in glyph_map.toml",
                spec.section, spec.key
            )
        });
        let glyph_id = face
            .glyph_index(ch)
            .unwrap_or_else(|| panic!("No glyph for '{}' in Astronomicon.ttf", ch));

        let mut collector = OutlineCollector::new(scale, flip_y);
        let has_outline = face.outline_glyph(glyph_id, &mut collector).is_some();
        if !has_outline {
            collector.ops.clear();
        }

        out.push_str(&format!(
            "pub const {}_OPS: &[GlyphOp] = &[\n",
            spec.const_name
        ));
        for op in &collector.ops {
            match *op {
                Op::M(x, y) => out.push_str(&format!(
                    "    GlyphOp::M({}, {}),\n",
                    fmt_f32(x),
                    fmt_f32(y)
                )),
                Op::L(x, y) => out.push_str(&format!(
                    "    GlyphOp::L({}, {}),\n",
                    fmt_f32(x),
                    fmt_f32(y)
                )),
                Op::Q(x1, y1, x, y) => out.push_str(&format!(
                    "    GlyphOp::Q({}, {}, {}, {}),\n",
                    fmt_f32(x1),
                    fmt_f32(y1),
                    fmt_f32(x),
                    fmt_f32(y)
                )),
                Op::C(x1, y1, x2, y2, x, y) => out.push_str(&format!(
                    "    GlyphOp::C({}, {}, {}, {}, {}, {}),\n",
                    fmt_f32(x1),
                    fmt_f32(y1),
                    fmt_f32(x2),
                    fmt_f32(y2),
                    fmt_f32(x),
                    fmt_f32(y)
                )),
                Op::Z => out.push_str("    GlyphOp::Z,\n"),
            }
        }
        out.push_str("];\n");

        let bounds = collector.bounds;
        let (min_x, min_y, max_x, max_y) = if bounds.has_bounds {
            (bounds.min_x, bounds.min_y, bounds.max_x, bounds.max_y)
        } else {
            (0.0, 0.0, 0.0, 0.0)
        };

        out.push_str(&format!(
            "pub const {}_BOUNDS: GlyphBounds = GlyphBounds {{ min_x: {}, min_y: {}, max_x: {}, max_y: {} }};\n\n",
            spec.const_name, fmt_f32(min_x), fmt_f32(min_y), fmt_f32(max_x), fmt_f32(max_y)
        ));

        // Legacy helper functions
        out.push_str(&format!(
            "pub fn {}() -> Path {{ build_path({}_OPS) }}\n\n",
            spec.func_name, spec.const_name
        ));
    }

    fs::write(&out_path, out).expect("Failed to write glyph_paths.rs");
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    generate_astronomicon(&manifest_dir, &out_dir);
}
