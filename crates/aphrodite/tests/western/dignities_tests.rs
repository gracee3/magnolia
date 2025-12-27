#[cfg(test)]
mod tests {
    use aphrodite_core::western::dignities::*;

    #[test]
    fn test_get_dignities_sun() {
        let service = DignitiesService;
        // Sun in Leo (120-150 degrees)
        let dignities = service.get_dignities("sun", 135.0, None);
        assert!(dignities.iter().any(|d| d.dignity_type == DignityType::Rulership));
    }
    
    #[test]
    fn test_get_dignities_moon() {
        let service = DignitiesService;
        // Moon in Cancer (90-120 degrees)
        let dignities = service.get_dignities("moon", 105.0, None);
        assert!(dignities.iter().any(|d| d.dignity_type == DignityType::Rulership));
    }
    
    #[test]
    fn test_get_dignities_exact_exaltation() {
        let service = DignitiesService;
        let exact_exaltations = service.get_default_exact_exaltations();
        // Sun at 19° Aries (exact exaltation)
        let dignities = service.get_dignities("sun", 19.0, Some(&exact_exaltations));
        assert!(dignities.iter().any(|d| d.dignity_type == DignityType::ExactExaltation));
    }
}

