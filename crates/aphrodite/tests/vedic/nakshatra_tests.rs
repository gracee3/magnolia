#[cfg(test)]
mod tests {
    use aphrodite_core::vedic::nakshatra::*;

    #[test]
    fn test_get_nakshatra_for_longitude() {
        let meta = get_nakshatra_for_longitude(0.0);
        assert_eq!(meta.base.id, "ashwini");
        assert_eq!(meta.base.lord, "ketu");
        assert_eq!(meta.pada, 1);
        
        let meta2 = get_nakshatra_for_longitude(13.33);
        assert_eq!(meta2.base.id, "ashwini");
        assert!(meta2.pada >= 1 && meta2.pada <= 4);
        
        // Test boundary between nakshatras
        let meta3 = get_nakshatra_for_longitude(13.33);
        assert_eq!(meta3.base.id, "ashwini");
    }
}

