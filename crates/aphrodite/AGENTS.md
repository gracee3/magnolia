# Aphrodite-Core: Agent Guidelines

## 1. Project Overview
**Aphrodite-Core** is a specialized, lightweight Rust astrological calculation engine powered by the Swiss Ephemeris (`swisseph`). It is designed to be a high-performance backend component for other applications (like `sigil_daemon`), stripping away all UI, API servers, and unnecessary runtime overhead.

**Goal**: Provide accurate planetary positions, house cusps, and aspect data with minimal friction.

## 2. Core Architecture
The library is a pure Rust crate (`aphrodite-core`) typically imported via a path dependency or git.

### Key Modules
- **`ephemeris`**: The primary interface for position calculations.
    - `SwissEphemerisAdapter`: The main entry point.
    - `EphemerisSettings`: Configuration for calculations (Zodiac, House System, etc.).
    - `GeoLocation`: Struct for latitude/longitude.
- **`aspects`**: (Optional) Tools for calculating angular relationships between bodies.
- **`vedic` / `western`**: (Optional) specialized calculation modes.

## 3. Usage Pattern (The "Sigil" Standard)
When integrating `aphrodite-core` into a Rust application, follow this standard pattern established in `sigil_daemon`.

### Step 1: Add Dependency
In your `Cargo.toml`:
```toml
[dependencies]
aphrodite-core = { path = "../path/to/aphrodite-rust" } # or git dependency
chrono = "0.4"
```

### Step 2: Initialize & Configure
You need to create an adapter instance and a settings struct.

```rust
use aphrodite_core::ephemeris::{SwissEphemerisAdapter, EphemerisSettings, GeoLocation};

// 1. Initialize Adapter
// Passing `None` uses default system paths (e.g., /usr/local/share/swisseph)
// or the SWISS_EPHEMERIS_PATH env var.
let mut adapter = SwissEphemerisAdapter::new(None).expect("Failed to init Swiss Ephemeris");

// 2. Define Settings
let settings = EphemerisSettings {
    zodiac_type: "tropical".to_string(), // or "sidereal"
    ayanamsa: None,                      // Only for sidereal (e.g., Some("lahiri".to_string()))
    house_system: "placidus".to_string(), // "placidus", "whole_sign", "koch", etc.
    include_objects: vec![
        "sun".to_string(), "moon".to_string(), "asc".to_string(),
        "mercury".to_string(), "venus".to_string(), "mars".to_string(),
        "jupiter".to_string(), "saturn".to_string()
    ],
};
```

### Step 3: Calculate Positions
Execute the calculation for a specific UTC timestamp and location.

```rust
let now = chrono::Utc::now();
let loc = Some(GeoLocation {
    lat: 40.7128, // Latitude
    lon: -74.0060 // Longitude
});

match adapter.calc_positions(now, loc, &settings) {
    Ok(positions) => {
        // Access Planetary Data
        if let Some(sun) = positions.planets.get("sun") {
             println!("Sun Longitude: {}", sun.lon);
             println!("Sun Speed: {}", sun.speed_lon);
        }

        // Access House Data
        if let Some(houses) = positions.houses {
            println!("Ascendant: {}", houses.angles.get("asc").unwrap_or(&0.0));
        }
    },
    Err(e) => eprintln!("Calculation Error: {}", e),
}
```

## 4. Technical Notes for Agents
- **Blocking Operations**: `SwissEphemerisAdapter::new` and `calc_positions` are synchronous and blocking. If integrating into a real-time UI loop (like Nannou) or an Async runtime, wrap these calls in `std::thread::spawn` or `tokio::task::spawn_blocking`.
- **Ephemeris Files**: The system requires valid Swiss Ephemeris data files (`.se1`, etc.) to be present on the system. The adapter looks in `/usr/local/share/swisseph` by default.
- **Thread Safety**: `SwissEphemerisAdapter` is not `Sync` (due to underlying C-library thread-local state or just safety). Create a fresh adapter per thread or protect it with a Mutex if needed, though ephemeral instantiation (init, calc, drop) is often cleaner for occasional lookups.

## 5. Standard Object IDs
When requesting objects in `include_objects`, use these standard keys:
- `sun`, `moon`
- `mercury`, `venus`, `mars`, `jupiter`, `saturn`, `uranus`, `neptune`, `pluto`
- `north_node`, `south_node`
- `chiron`
