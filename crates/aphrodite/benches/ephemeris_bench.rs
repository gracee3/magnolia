use criterion::{black_box, criterion_group, criterion_main, Criterion};
use aphrodite_core::ephemeris::{EphemerisSettings, GeoLocation, SwissEphemerisAdapter};
use chrono::Utc;

fn bench_calc_positions(c: &mut Criterion) {
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
    
    c.bench_function("calc_positions", |b| {
        b.iter(|| {
            adapter.calc_positions(
                black_box(dt),
                black_box(location.clone()),
                black_box(&settings),
            )
        })
    });
}

criterion_group!(benches, bench_calc_positions);
criterion_main!(benches);

