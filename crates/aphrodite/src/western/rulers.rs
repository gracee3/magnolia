//! Sign rulers for Western astrology.
//! 
//! Maps zodiac signs to their planetary rulers (traditional and modern).

/// Get sign index (0-11) from longitude
pub fn get_sign_index(longitude: f64) -> u8 {
    let normalized = longitude % 360.0;
    (normalized / 30.0) as u8
}

/// Get sign ruler (traditional rulership)
pub fn get_sign_ruler(sign_index: u8, modern: bool) -> String {
    let sign_idx = (sign_index % 12) as usize;
    
    if modern {
        // Modern rulerships include outer planets
        const MODERN_RULERS: &[&str] = &[
            "mars",      // Aries
            "venus",     // Taurus
            "mercury",   // Gemini
            "moon",      // Cancer
            "sun",       // Leo
            "mercury",   // Virgo
            "venus",     // Libra
            "pluto",     // Scorpio (modern)
            "jupiter",   // Sagittarius
            "saturn",    // Capricorn
            "uranus",    // Aquarius (modern)
            "neptune",   // Pisces (modern)
        ];
        MODERN_RULERS[sign_idx].to_string()
    } else {
        // Traditional rulerships
        const TRADITIONAL_RULERS: &[&str] = &[
            "mars",      // Aries
            "venus",     // Taurus
            "mercury",   // Gemini
            "moon",      // Cancer
            "sun",       // Leo
            "mercury",   // Virgo
            "venus",     // Libra
            "mars",      // Scorpio (traditional)
            "jupiter",   // Sagittarius
            "saturn",    // Capricorn
            "saturn",    // Aquarius (traditional)
            "jupiter",   // Pisces (traditional)
        ];
        TRADITIONAL_RULERS[sign_idx].to_string()
    }
}

/// Get sign ruler from longitude
pub fn get_sign_ruler_from_longitude(longitude: f64, modern: bool) -> String {
    let sign_index = get_sign_index(longitude);
    get_sign_ruler(sign_index, modern)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_get_sign_ruler_traditional() {
        assert_eq!(get_sign_ruler(0, false), "mars");   // Aries
        assert_eq!(get_sign_ruler(3, false), "moon");   // Cancer
        assert_eq!(get_sign_ruler(4, false), "sun");    // Leo
        assert_eq!(get_sign_ruler(7, false), "mars");    // Scorpio (traditional)
    }
    
    #[test]
    fn test_get_sign_ruler_modern() {
        assert_eq!(get_sign_ruler(7, true), "pluto");    // Scorpio (modern)
        assert_eq!(get_sign_ruler(10, true), "uranus");  // Aquarius (modern)
        assert_eq!(get_sign_ruler(11, true), "neptune"); // Pisces (modern)
    }
}

