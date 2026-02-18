use crate::au_postcodes::AU_POSTCODE_CENTROIDS;
use crate::location::GeoLocation;

/// Result of a postcode lookup: location + place name + state.
#[derive(Debug, Clone)]
pub struct PostcodeInfo {
    pub postcode: String,
    pub location: GeoLocation,
    pub place_name: String,
    pub state: String,
}

impl PostcodeInfo {
    /// Format as "Place Name, STATE Postcode" e.g. "Sydney, NSW 2000"
    pub fn display_name(&self) -> String {
        format!("{}, {} {}", self.place_name, self.state, self.postcode)
    }

    /// Format as "Place Name (Postcode)" e.g. "Sydney (2000)"
    pub fn short_name(&self) -> String {
        format!("{} ({})", self.place_name, self.postcode)
    }
}

fn find_index(postcode: &str) -> Option<usize> {
    AU_POSTCODE_CENTROIDS
        .binary_search_by_key(&postcode, |(pc, _, _, _, _)| pc)
        .ok()
}

/// Look up full info for an Australian postcode.
pub fn lookup_au_postcode_info(postcode: &str) -> Option<PostcodeInfo> {
    find_index(postcode).map(|idx| {
        let (pc, lat, lon, name, state) = AU_POSTCODE_CENTROIDS[idx];
        PostcodeInfo {
            postcode: pc.to_string(),
            location: GeoLocation::new(lat, lon),
            place_name: name.to_string(),
            state: state.to_string(),
        }
    })
}

/// Look up the geographic centroid for an Australian postcode.
pub fn lookup_au_postcode(postcode: &str) -> Option<GeoLocation> {
    find_index(postcode).map(|idx| {
        let (_, lat, lon, _, _) = AU_POSTCODE_CENTROIDS[idx];
        GeoLocation::new(lat, lon)
    })
}

/// Check if an Australian postcode is valid (exists in our dataset).
pub fn is_valid_au_postcode(postcode: &str) -> bool {
    find_index(postcode).is_some()
}

/// Calculate distance in km between two Australian postcodes.
/// Returns None if either postcode is not found.
pub fn distance_between_postcodes(a: &str, b: &str) -> Option<f64> {
    let loc_a = lookup_au_postcode(a)?;
    let loc_b = lookup_au_postcode(b)?;
    Some(loc_a.distance_km(&loc_b))
}

/// Format a postcode for display, showing the place name.
/// Returns "Place Name (postcode)" or just the postcode if not found.
pub fn format_postcode(postcode: &str) -> String {
    lookup_au_postcode_info(postcode)
        .map(|info| info.short_name())
        .unwrap_or_else(|| postcode.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lookup_sydney_cbd() {
        let info = lookup_au_postcode_info("2000").expect("2000 should exist");
        assert_eq!(info.place_name, "Sydney");
        assert_eq!(info.state, "NSW");
        assert!((info.location.latitude - (-33.87)).abs() < 0.1);
        assert!((info.location.longitude - 151.21).abs() < 0.1);
    }

    #[test]
    fn test_lookup_melbourne_cbd() {
        let info = lookup_au_postcode_info("3000").expect("3000 should exist");
        assert_eq!(info.place_name, "Melbourne");
        assert_eq!(info.state, "VIC");
    }

    #[test]
    fn test_display_name() {
        let info = lookup_au_postcode_info("2000").unwrap();
        assert_eq!(info.display_name(), "Sydney, NSW 2000");
        assert_eq!(info.short_name(), "Sydney (2000)");
    }

    #[test]
    fn test_format_postcode() {
        assert_eq!(format_postcode("2000"), "Sydney (2000)");
        assert_eq!(format_postcode("3000"), "Melbourne (3000)");
        assert_eq!(format_postcode("0000"), "0000");
    }

    #[test]
    fn test_invalid_postcode() {
        assert!(lookup_au_postcode("0000").is_none());
        assert!(lookup_au_postcode("99999").is_none());
        assert!(lookup_au_postcode("abc").is_none());
    }

    #[test]
    fn test_distance_sydney_to_melbourne() {
        let dist = distance_between_postcodes("2000", "3000").expect("both should exist");
        assert!((dist - 714.0).abs() < 50.0);
    }

    #[test]
    fn test_distance_same_postcode() {
        let dist = distance_between_postcodes("2000", "2000").expect("should exist");
        assert!(dist < 0.001);
    }

    #[test]
    fn test_nearby_postcodes() {
        let dist = distance_between_postcodes("2000", "2010").expect("both should exist");
        assert!(dist < 5.0, "Sydney CBD to Surry Hills should be < 5km, got {dist}");
    }
}
