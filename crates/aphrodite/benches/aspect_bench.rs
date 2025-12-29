use criterion::{black_box, criterion_group, criterion_main, Criterion};
use aphrodite::aspects::{AspectCalculator, AspectSettings};
use aphrodite::ephemeris::{LayerPositions, PlanetPosition};
use std::collections::HashMap;

fn bench_calculate_aspect_conjunction(c: &mut Criterion) {
    let calculator = AspectCalculator::new();
    let mut orb_settings = HashMap::new();
    orb_settings.insert("conjunction".to_string(), 8.0);
    orb_settings.insert("opposition".to_string(), 8.0);
    orb_settings.insert("trine".to_string(), 7.0);
    orb_settings.insert("square".to_string(), 6.0);
    orb_settings.insert("sextile".to_string(), 4.0);
    
    c.bench_function("calculate_aspect_conjunction", |b| {
        b.iter(|| {
            calculator.calculate_aspect(
                black_box(100.0),
                black_box(102.0), // Within conjunction orb
                black_box(1.0),
                black_box(1.0),
                black_box(&orb_settings),
            )
        })
    });
}

fn bench_calculate_aspect_opposition(c: &mut Criterion) {
    let calculator = AspectCalculator::new();
    let mut orb_settings = HashMap::new();
    orb_settings.insert("conjunction".to_string(), 8.0);
    orb_settings.insert("opposition".to_string(), 8.0);
    orb_settings.insert("trine".to_string(), 7.0);
    orb_settings.insert("square".to_string(), 6.0);
    orb_settings.insert("sextile".to_string(), 4.0);
    
    c.bench_function("calculate_aspect_opposition", |b| {
        b.iter(|| {
            calculator.calculate_aspect(
                black_box(100.0),
                black_box(278.0), // ~180 degrees (opposition)
                black_box(1.0),
                black_box(1.0),
                black_box(&orb_settings),
            )
        })
    });
}

fn bench_compute_intra_layer_aspects_small(c: &mut Criterion) {
    let calculator = AspectCalculator::new();
    
    let mut planets = HashMap::new();
    for i in 0..5 {
        planets.insert(
            format!("planet_{}", i),
            PlanetPosition {
                lon: (i as f64) * 30.0,
                lat: 0.0,
                speed_lon: 1.0,
                retrograde: false,
            },
        );
    }
    
    let positions = LayerPositions {
        planets,
        houses: None,
    };
    
    let mut orb_settings = HashMap::new();
    orb_settings.insert("conjunction".to_string(), 8.0);
    orb_settings.insert("opposition".to_string(), 8.0);
    orb_settings.insert("trine".to_string(), 7.0);
    orb_settings.insert("square".to_string(), 6.0);
    orb_settings.insert("sextile".to_string(), 4.0);
    
    let settings = AspectSettings {
        orb_settings,
        include_objects: vec![],
        only_major: None,
    };
    
    c.bench_function("compute_intra_layer_aspects_small", |b| {
        b.iter(|| {
            calculator.compute_intra_layer_aspects(
                black_box("natal"),
                black_box(&positions),
                black_box(&settings),
            )
        })
    });
}

fn bench_compute_intra_layer_aspects_large(c: &mut Criterion) {
    let calculator = AspectCalculator::new();
    
    let mut planets = HashMap::new();
    // Include all major planets plus some asteroids
    let planet_names = vec![
        "sun", "moon", "mercury", "venus", "mars",
        "jupiter", "saturn", "uranus", "neptune", "pluto",
        "chiron", "north_node", "south_node",
    ];
    
    for (i, name) in planet_names.iter().enumerate() {
        planets.insert(
            name.to_string(),
            PlanetPosition {
                lon: (i as f64) * 27.69, // Spread across zodiac
                lat: 0.0,
                speed_lon: 1.0,
                retrograde: i % 3 == 0, // Some retrograde
            },
        );
    }
    
    let positions = LayerPositions {
        planets,
        houses: None,
    };
    
    let mut orb_settings = HashMap::new();
    orb_settings.insert("conjunction".to_string(), 8.0);
    orb_settings.insert("opposition".to_string(), 8.0);
    orb_settings.insert("trine".to_string(), 7.0);
    orb_settings.insert("square".to_string(), 6.0);
    orb_settings.insert("sextile".to_string(), 4.0);
    
    let settings = AspectSettings {
        orb_settings,
        include_objects: vec![],
        only_major: None,
    };
    
    c.bench_function("compute_intra_layer_aspects_large", |b| {
        b.iter(|| {
            calculator.compute_intra_layer_aspects(
                black_box("natal"),
                black_box(&positions),
                black_box(&settings),
            )
        })
    });
}

criterion_group!(
    benches,
    bench_calculate_aspect_conjunction,
    bench_calculate_aspect_opposition,
    bench_compute_intra_layer_aspects_small,
    bench_compute_intra_layer_aspects_large
);
criterion_main!(benches);

