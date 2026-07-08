use serde::Serialize;

use crate::domain::location::Location;

#[derive(Debug, Clone, Serialize)]
pub struct OutdoorTour {
    pub id: String,
    pub title: String,
    pub category: String,
    pub location: Location,
    pub description: String,
    pub duration_minutes: u32,
    pub length_meters: u32,
    pub ascent_meters: u32,
    pub descent_meters: u32,
    pub difficulty: u8,
    pub stamina: u8,
    pub landscape: u8,
    pub experience: u8,
    pub is_loop: bool,
    pub season_bitmask: u16,
    #[serde(skip)]
    pub raw_json: String,
}

impl OutdoorTour {
    pub fn in_season(&self, month: u32) -> bool {
        self.season_bitmask & (1 << (month - 1)) != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tour(bitmask: u16) -> OutdoorTour {
        OutdoorTour {
            id: "1".into(),
            title: "T".into(),
            category: "Wanderung".into(),
            location: Location::new(50.0, 13.0, "T".into(), "".into()),
            description: String::new(),
            duration_minutes: 60,
            length_meters: 5000,
            ascent_meters: 200,
            descent_meters: 200,
            difficulty: 0,
            stamina: 0,
            landscape: 0,
            experience: 0,
            is_loop: false,
            season_bitmask: bitmask,
            raw_json: String::new(),
        }
    }

    #[test]
    fn in_season_checks_correct_bit() {
        let t = tour(0b0000_0111_1100); // mar–jul
        assert!(!t.in_season(1));
        assert!(!t.in_season(2));
        assert!(t.in_season(3));
        assert!(t.in_season(7));
        assert!(!t.in_season(8));
    }

    #[test]
    fn all_season_bits_set() {
        let t = tour(0b1111_1111_1111);
        for m in 1..=12 {
            assert!(t.in_season(m), "month {m} should be in season");
        }
    }

    #[test]
    fn no_season_bits_set() {
        let t = tour(0);
        for m in 1..=12 {
            assert!(!t.in_season(m), "month {m} should not be in season");
        }
    }
}
