use criterion::{black_box, criterion_group, criterion_main, Criterion};
use aphrodite::ephemeris::{EphemerisSettings, GeoLocation, SwissEphemerisAdapter};
use chrono::{Utc, TimeZone};

fn bench_calc_positions_basic(c: &mut Criterion) {
    let mut adapter = SwissEphemerisAdapter::new(None).unwrap();
    
    let settings = EphemerisSettings {
        zodiac_type: "tropical".to_string(),
        ayanamsa: None,
        house_system: "placidus".to_string(),
        include_objects: vec![
            "sun".to_string(),
            "moon".to_string(),
            "mercury".to_string(),
            "venus".to_string(),
            "mars".to_string(),
        ],
    };
    
    let location = Some(GeoLocation {
        lat: 40.7128,
        lon: -74.0060,
    });
    
    let dt = Utc::now();
    
    c.bench_function("calc_positions_basic", |b| {
        b.iter(|| {
            adapter.calc_positions(
                black_box(dt),
                black_box(location.clone()),
                black_box(&settings),
            )
        })
    });
}

fn bench_calc_positions_all_planets(c: &mut Criterion) {
    let mut adapter = SwissEphemerisAdapter::new(None).unwrap();
    
    let settings = EphemerisSettings {
        zodiac_type: "tropical".to_string(),
        ayanamsa: None,
        house_system: "placidus".to_string(),
        include_objects: vec![
            "sun".to_string(),
            "moon".to_string(),
            "mercury".to_string(),
            "venus".to_string(),
            "mars".to_string(),
            "jupiter".to_string(),
            "saturn".to_string(),
            "uranus".to_string(),
            "neptune".to_string(),
            "pluto".to_string(),
            "chiron".to_string(),
            "north_node".to_string(),
        ],
    };
    
    let location = Some(GeoLocation {
        lat: 51.48,
        lon: 0.0,
    });
    
    let dt = Utc.with_ymd_and_hms(2000, 1, 1, 12, 0, 0).unwrap();
    
    c.bench_function("calc_positions_all_planets", |b| {
        b.iter(|| {
            adapter.calc_positions(
                black_box(dt),
                black_box(location.clone()),
                black_box(&settings),
            )
        })
    });
}

fn bench_calc_positions_sidereal(c: &mut Criterion) {
    let mut adapter = SwissEphemerisAdapter::new(None).unwrap();
    
    let settings = EphemerisSettings {
        zodiac_type: "sidereal".to_string(),
        ayanamsa: Some("lahiri".to_string()),
        house_system: "whole_sign".to_string(),
        include_objects: vec![
            "sun".to_string(),
            "moon".to_string(),
            "mercury".to_string(),
            "venus".to_string(),
            "mars".to_string(),
        ],
    };
    
    let location = Some(GeoLocation {
        lat: 28.6139,
        lon: 77.2090,
    });
    
    let dt = Utc.with_ymd_and_hms(2000, 1, 1, 12, 0, 0).unwrap();
    
    c.bench_function("calc_positions_sidereal", |b| {
        b.iter(|| {
            adapter.calc_positions(
                black_box(dt),
                black_box(location.clone()),
                black_box(&settings),
            )
        })
    });
}

criterion_group!(benches, bench_calc_positions_basic, bench_calc_positions_all_planets, bench_calc_positions_sidereal);
criterion_main!(benches);

