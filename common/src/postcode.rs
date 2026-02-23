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

fn entry_to_info((pc, lat, lon, name, state): &(&str, f64, f64, &str, &str)) -> PostcodeInfo {
    PostcodeInfo {
        postcode: pc.to_string(),
        location: GeoLocation::new(*lat, *lon),
        place_name: name.to_string(),
        state: state.to_string(),
    }
}

/// Find the index of the first entry for a given postcode using binary search.
fn find_first_index(postcode: &str) -> Option<usize> {
    let idx = AU_POSTCODE_CENTROIDS
        .binary_search_by_key(&postcode, |(pc, _, _, _, _)| pc)
        .ok()?;
    // Walk backwards to find the first entry for this postcode
    let mut first = idx;
    while first > 0 && AU_POSTCODE_CENTROIDS[first - 1].0 == postcode {
        first -= 1;
    }
    Some(first)
}

/// Look up all localities for a given postcode.
pub fn lookup_all_localities(postcode: &str) -> Vec<PostcodeInfo> {
    let Some(first) = find_first_index(postcode) else {
        return Vec::new();
    };
    let mut results = Vec::new();
    for entry in &AU_POSTCODE_CENTROIDS[first..] {
        if entry.0 != postcode {
            break;
        }
        results.push(entry_to_info(entry));
    }
    results
}

/// Look up a specific postcode + locality pair.
pub fn lookup_locality(postcode: &str, locality: &str) -> Option<PostcodeInfo> {
    let first = find_first_index(postcode)?;
    let locality_lower = locality.to_lowercase();
    for entry in &AU_POSTCODE_CENTROIDS[first..] {
        if entry.0 != postcode {
            break;
        }
        if entry.3.to_lowercase() == locality_lower {
            return Some(entry_to_info(entry));
        }
    }
    None
}

/// Find the nearest postcode entry to a given location (brute-force).
pub fn nearest_postcode(location: &GeoLocation) -> Option<PostcodeInfo> {
    AU_POSTCODE_CENTROIDS
        .iter()
        .min_by(|a, b| {
            let da = location.distance_km(&GeoLocation::new(a.1, a.2));
            let db = location.distance_km(&GeoLocation::new(b.1, b.2));
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|e| entry_to_info(e))
}

/// Look up full info for an Australian postcode (first locality match).
pub fn lookup_au_postcode_info(postcode: &str) -> Option<PostcodeInfo> {
    find_first_index(postcode).map(|idx| entry_to_info(&AU_POSTCODE_CENTROIDS[idx]))
}

/// Look up the geographic centroid for an Australian postcode (first match).
pub fn lookup_au_postcode(postcode: &str) -> Option<GeoLocation> {
    find_first_index(postcode).map(|idx| {
        let (_, lat, lon, _, _) = AU_POSTCODE_CENTROIDS[idx];
        GeoLocation::new(lat, lon)
    })
}

/// Check if an Australian postcode is valid (exists in our dataset).
pub fn is_valid_au_postcode(postcode: &str) -> bool {
    find_first_index(postcode).is_some()
}

/// Calculate distance in km between two Australian postcodes.
/// Returns None if either postcode is not found.
pub fn distance_between_postcodes(a: &str, b: &str) -> Option<f64> {
    let loc_a = lookup_au_postcode(a)?;
    let loc_b = lookup_au_postcode(b)?;
    Some(loc_a.distance_km(&loc_b))
}

/// Return the IANA timezone for an Australian state abbreviation.
pub fn timezone_for_state(state: &str) -> Option<&'static str> {
    match state {
        "NSW" | "ACT" => Some("Australia/Sydney"),
        "VIC" => Some("Australia/Melbourne"),
        "QLD" => Some("Australia/Brisbane"),
        "TAS" => Some("Australia/Hobart"),
        "SA" => Some("Australia/Adelaide"),
        "NT" => Some("Australia/Darwin"),
        "WA" => Some("Australia/Perth"),
        _ => None,
    }
}

/// Derive IANA timezone from an Australian postcode using the state of the first locality match.
pub fn timezone_for_postcode(postcode: &str) -> Option<&'static str> {
    let info = lookup_au_postcode_info(postcode)?;
    timezone_for_state(&info.state)
}

/// Format a postcode for display, showing the locality name.
/// If locality is provided, shows "Locality (Postcode)".
/// Otherwise shows the first locality match, or just the postcode if not found.
pub fn format_postcode(postcode: &str, locality: Option<&str>) -> String {
    if let Some(loc) = locality {
        if let Some(info) = lookup_locality(postcode, loc) {
            return info.short_name();
        }
        // Locality didn't match dataset — show it anyway
        return format!("{loc} ({postcode})");
    }
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
        assert_eq!(info.state, "NSW");
        assert!((info.location.latitude - (-33.86)).abs() < 0.1);
        assert!((info.location.longitude - 151.21).abs() < 0.1);
    }

    #[test]
    fn test_lookup_melbourne_cbd() {
        let info = lookup_au_postcode_info("3000").expect("3000 should exist");
        assert_eq!(info.state, "VIC");
    }

    #[test]
    fn test_multiple_localities_for_2000() {
        let localities = lookup_all_localities("2000");
        assert!(
            localities.len() > 1,
            "postcode 2000 should have multiple localities, got {}",
            localities.len()
        );
        let names: Vec<_> = localities.iter().map(|l| l.place_name.as_str()).collect();
        assert!(names.contains(&"Sydney"), "should contain Sydney");
        assert!(names.contains(&"Haymarket"), "should contain Haymarket");
    }

    #[test]
    fn test_lookup_locality() {
        let info = lookup_locality("2000", "Haymarket").expect("Haymarket should exist");
        assert_eq!(info.postcode, "2000");
        assert_eq!(info.place_name, "Haymarket");
        assert_eq!(info.state, "NSW");
    }

    #[test]
    fn test_lookup_locality_case_insensitive() {
        assert!(lookup_locality("2000", "haymarket").is_some());
        assert!(lookup_locality("2000", "HAYMARKET").is_some());
    }

    #[test]
    fn test_nearest_postcode() {
        // Use exact Sydney 2000 coordinates from the dataset
        let loc = GeoLocation::new(-33.86, 151.2566);
        let info = nearest_postcode(&loc).expect("should find something");
        assert_eq!(info.postcode, "2000");
    }

    #[test]
    fn test_format_postcode_with_locality() {
        assert_eq!(format_postcode("2000", Some("Sydney")), "Sydney (2000)");
        assert_eq!(
            format_postcode("2000", Some("Haymarket")),
            "Haymarket (2000)"
        );
    }

    #[test]
    fn test_format_postcode_without_locality() {
        // First match for 2000 alphabetically
        let result = format_postcode("2000", None);
        assert!(result.contains("2000"), "should contain postcode");
        assert_eq!(format_postcode("0000", None), "0000");
    }

    #[test]
    fn test_timezone_for_state() {
        assert_eq!(timezone_for_state("NSW"), Some("Australia/Sydney"));
        assert_eq!(timezone_for_state("ACT"), Some("Australia/Sydney"));
        assert_eq!(timezone_for_state("VIC"), Some("Australia/Melbourne"));
        assert_eq!(timezone_for_state("QLD"), Some("Australia/Brisbane"));
        assert_eq!(timezone_for_state("WA"), Some("Australia/Perth"));
        assert_eq!(timezone_for_state("XX"), None);
    }

    #[test]
    fn test_timezone_for_postcode() {
        assert_eq!(timezone_for_postcode("2000"), Some("Australia/Sydney"));
        assert_eq!(timezone_for_postcode("3000"), Some("Australia/Melbourne"));
        assert_eq!(timezone_for_postcode("4000"), Some("Australia/Brisbane"));
        assert_eq!(timezone_for_postcode("0000"), None);
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
        assert!(
            (dist - 714.0).abs() < 50.0,
            "Sydney to Melbourne should be ~714km, got {dist}"
        );
    }

    #[test]
    fn test_distance_same_postcode() {
        let dist = distance_between_postcodes("2000", "2000").expect("should exist");
        assert!(dist < 0.001);
    }

    #[test]
    fn test_nearby_postcodes() {
        let dist = distance_between_postcodes("2000", "2010").expect("both should exist");
        assert!(
            dist < 5.0,
            "Sydney CBD to Surry Hills should be < 5km, got {dist}"
        );
    }
}
