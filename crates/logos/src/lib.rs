use enigo::{Enigo, Settings, Key as EnigoKey, Keyboard, Direction};
use sha2::{Digest, Sha256};
use kamea::PlanetarySphere;

pub fn engage_hardware() {
    // Hardware control to engage the machine.
    // We simply press the key once to toggle it, as state detection is tricky.
    // Try to init Enigo
    if let Ok(mut enigo) = Enigo::new(&Settings::default()) {
        // Enigo 0.3 uses key(Key, Direction)
        let _ = enigo.key(EnigoKey::CapsLock, Direction::Click);
    }
}

pub fn disengage_hardware() {
     if let Ok(mut enigo) = Enigo::new(&Settings::default()) {
        let _ = enigo.key(EnigoKey::CapsLock, Direction::Click);
    }
}

pub fn determine_sphere(intent: &str) -> PlanetarySphere {
    let i = intent.to_uppercase();
    
    // Heuristic Matching (Order matters for priority)
    if i.contains("WAR") || i.contains("WIN") || i.contains("FIGHT") || i.contains("DEFEND") { return PlanetarySphere::Mars; }
    if i.contains("LOVE") || i.contains("FRIEND") || i.contains("PEACE") { return PlanetarySphere::Venus; }
    if i.contains("MONEY") || i.contains("JOB") || i.contains("WEALTH") || i.contains("FUND") { return PlanetarySphere::Jupiter; }
    if i.contains("HEALTH") || i.contains("HEAL") || i.contains("SHINE") { return PlanetarySphere::Sun; }
    if i.contains("CODE") || i.contains("WRITE") || i.contains("TRAVEL") { return PlanetarySphere::Mercury; }
    if i.contains("HOME") || i.contains("DREAM") || i.contains("SAFE") { return PlanetarySphere::Moon; }
    if i.contains("STOP") || i.contains("BIND") || i.contains("END") || i.contains("TIME") { return PlanetarySphere::Saturn; }

    // Fallback: Jupiter (The Great Benefic) is the safest default
    PlanetarySphere::Jupiter
}

pub fn semantic_hash(intent: &str, salt: &str) -> (String, [u8; 32]) {
    let raw = format!("{}{}", intent, salt);
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    let result = hasher.finalize();
    let hash_hex = hex::encode(result);
    
    let mut seed_bytes = [0u8; 32];
    seed_bytes.copy_from_slice(&result[..32]);
    
    (hash_hex, seed_bytes)
}
