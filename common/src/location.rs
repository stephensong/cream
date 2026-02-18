use serde::{Deserialize, Serialize};

/// Geographic coordinates in decimal degrees.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeoLocation {
    pub latitude: f64,
    pub longitude: f64,
}

impl GeoLocation {
    pub fn new(latitude: f64, longitude: f64) -> Self {
        Self {
            latitude,
            longitude,
        }
    }

    /// Haversine distance in kilometers between two points.
    pub fn distance_km(&self, other: &GeoLocation) -> f64 {
        const EARTH_RADIUS_KM: f64 = 6371.0;

        let lat1 = self.latitude.to_radians();
        let lat2 = other.latitude.to_radians();
        let dlat = (other.latitude - self.latitude).to_radians();
        let dlon = (other.longitude - self.longitude).to_radians();

        let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
        let c = 2.0 * a.sqrt().asin();

        EARTH_RADIUS_KM * c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_distance_same_point() {
        let p = GeoLocation::new(40.7128, -74.0060);
        assert!((p.distance_km(&p) - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_distance_nyc_to_la() {
        let nyc = GeoLocation::new(40.7128, -74.0060);
        let la = GeoLocation::new(34.0522, -118.2437);
        let dist = nyc.distance_km(&la);
        // NYC to LA is ~3944 km
        assert!((dist - 3944.0).abs() < 50.0);
    }
}
