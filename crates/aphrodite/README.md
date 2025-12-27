# Aphrodite Rust - Core Engine

This is the Rust implementation of the Gaia Tools astrology platform core engine, migrated from Python/TypeScript.
It acts as a high-level wrapper around the Swiss Ephemeris (`swisseph`) for astrological calculations.

## Status

- ✅ Core Computation Engine (Ephemeris, Aspects)
- ✅ Full Jyotish (Vedic Astrology)
- ✅ Dignities, Rulers, Decans

## Structure

```
aphrodite-rust/
├── Cargo.toml              # Package configuration
├── src/
│   ├── lib.rs
│   ├── ephemeris/          # Swiss Ephemeris adapter
│   ├── aspects/            # Aspect calculation engine
│   ├── vedic/              # Vedic astrology (nakshatras, vargas, dashas, yogas)
│   └── western/            # Western astrology (dignities, rulers, decans)
├── tests/                  # Unit tests
└── benches/                # Performance benchmarks
```

## Features

### Core Computation Engine

- **Swiss Ephemeris Integration**: Uses `swisseph` crate for planetary and house calculations
- **Ephemeris Calculations**: 
  - Planetary positions (all major planets, Chiron, nodes)
  - House systems (Placidus, Whole Sign, Koch, Equal, Regiomontanus, Campanus, Alcabitius, Morinus)
  - Tropical/Sidereal zodiac support
  - Multiple ayanamsas (Lahiri, Fagan-Bradley, Raman, etc.)
- **Aspect Calculation Engine**:
  - Intra-layer and inter-layer aspects
  - Support for conjunction, opposition, trine, square, sextile
  - Orb settings per aspect type
  - Applying/separating detection
  - Retrograde detection

### Full Jyotish (Vedic Astrology)

- **Nakshatras**: 27 lunar mansions with padas (quarters)
- **Vargas**: 16 divisional charts (D2-D60)
- **Dashas**: Vimshottari, Yogini, Ashtottari, Kalachakra
- **Yogas**: Classic Vedic planetary combinations

### Dignities, Rulers, Decans

- **Dignities**: Planetary strength indicators (Rulership, Exaltation, Fall, Detriment)
- **Sign Rulers**: Traditional and modern rulerships
- **Decans**: Three decans per sign (10 degrees each)

## Dependencies

- `swisseph` - Swiss Ephemeris Rust bindings
- `serde` / `serde_json` - JSON serialization
- `chrono` - Date/time handling
- `thiserror` - Error handling
- `anyhow` - Error context
- `uuid` - ID generation
- `lazy_static` - Static initialization

## Usage

### Calculating Ephemeris Positions

```rust
use aphrodite_core::ephemeris::{SwissEphemerisAdapter, EphemerisSettings, GeoLocation};
use chrono::Utc;

let mut adapter = SwissEphemerisAdapter::new(None)?;
let settings = EphemerisSettings {
    zodiac_type: "tropical".to_string(),
    ayanamsa: None,
    house_system: "placidus".to_string(),
    include_objects: vec!["sun".to_string(), "moon".to_string()],
};
let location = Some(GeoLocation { lat: 40.7128, lon: -74.0060 });
let positions = adapter.calc_positions(Utc::now(), location, &settings)?;
```

### Calculating Aspects

```rust
use aphrodite_core::aspects::{AspectCalculator, AspectSettings};
use std::collections::HashMap;

let calculator = AspectCalculator::new();
let orb_settings = HashMap::from([
    ("conjunction".to_string(), 8.0),
    ("opposition".to_string(), 8.0),
    ("trine".to_string(), 7.0),
    ("square".to_string(), 6.0),
    ("sextile".to_string(), 4.0),
]);
let settings = AspectSettings {
    orb_settings,
    include_objects: vec![],
    only_major: None,
};
let aspect_set = calculator.compute_intra_layer_aspects("natal", &positions, &settings);
```

### Calculating Vedic Data

```rust
use aphrodite_core::vedic::{
    annotate_layer_nakshatras, build_varga_layers, identify_yogas,
    compute_vimshottari_dasha, DashaLevel,
};

// Nakshatras
let nakshatra_placements = annotate_layer_nakshatras(&positions, true, None);

// Vargas
let varga_layers = build_varga_layers("natal", &positions, &vec!["d9".to_string()]);

// Yogas
let yogas = identify_yogas(&positions);

// Dashas
let dashas = compute_vimshottari_dasha(birth_datetime, &positions, DashaLevel::Pratyantardasha)?;
```

## Testing

Unit tests are available for all modules:
```bash
cargo test
```

## License

This project is licensed under the **GNU Affero General Public License v3.0 or later** (AGPL-3.0-or-later).

See the [LICENSE](LICENSE) file for the full license text.

### Swiss Ephemeris Licensing

This project uses the `swisseph` crate, which provides Rust bindings to the **Swiss Ephemeris** library. The Swiss Ephemeris is available under a dual licensing system:

1. **GNU Affero General Public License (AGPL)**: The Swiss Ephemeris can be used under the AGPL, which requires that any software incorporating it must also be distributed under the AGPL or a compatible license. This is compatible with this project's AGPL-3.0-or-later license.

2. **Swiss Ephemeris Professional License**: For commercial use or if you prefer not to release your source code under the AGPL, you can purchase a professional license from Astrodienst. Information on obtaining this license is available at: [Swiss Ephemeris Professional License](https://www.astro.com/swisseph/swephprg.htm)

Please ensure that your use of this project and the Swiss Ephemeris complies with the applicable licensing terms.
