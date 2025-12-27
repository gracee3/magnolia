#[cfg(test)]
mod tests {
    use aphrodite_core::western::decans::*;

    #[test]
    fn test_get_decan_index() {
        assert_eq!(get_decan_index(0.0), 1);
        assert_eq!(get_decan_index(5.0), 1);
        assert_eq!(get_decan_index(9.999), 1);
        assert_eq!(get_decan_index(10.0), 2);
        assert_eq!(get_decan_index(15.0), 2);
        assert_eq!(get_decan_index(19.999), 2);
        assert_eq!(get_decan_index(20.0), 3);
        assert_eq!(get_decan_index(25.0), 3);
        assert_eq!(get_decan_index(29.999), 3);
    }
    
    #[test]
    fn test_get_decan_info_for_sign_and_degree() {
        let info = get_decan_info_for_sign_and_degree("aries", 5.0).unwrap();
        assert_eq!(info.sign, "aries");
        assert_eq!(info.decan_index, 1);
        assert_eq!(info.sign_ruler, "mars");
        // First decan of Aries (fire) should be ruled by Mars (first in fire group)
        assert_eq!(info.decan_ruler, "mars");
    }
    
    #[test]
    fn test_get_decan_info_from_longitude() {
        let info = get_decan_info_from_longitude(5.0); // 5° Aries
        assert_eq!(info.sign, "aries");
        assert_eq!(info.decan_index, 1);
    }
}

