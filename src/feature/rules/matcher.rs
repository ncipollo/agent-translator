use serde::{Deserialize, Serialize};

/// Describes when a native JSON line (or an element reached via `MapRule::iterate`) matches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Matcher {
    Eq { path: String, value: String },
    Exists { path: String },
    In { path: String, any_of: Vec<String> },
    Not { matcher: Box<Matcher> },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip_json(matcher: &Matcher) {
        let serialized = serde_json::to_string(matcher).unwrap();
        let deserialized: Matcher = serde_json::from_str(&serialized).unwrap();
        assert_eq!(matcher, &deserialized);
    }

    fn round_trip_toml(matcher: &Matcher) {
        let serialized = toml::to_string(matcher).unwrap();
        let deserialized: Matcher = toml::from_str(&serialized).unwrap();
        assert_eq!(matcher, &deserialized);
    }

    #[test]
    fn round_trips_eq() {
        let matcher = Matcher::Eq {
            path: "type".into(),
            value: "text".into(),
        };
        round_trip_json(&matcher);
        round_trip_toml(&matcher);
    }

    #[test]
    fn round_trips_exists() {
        let matcher = Matcher::Exists {
            path: "delta".into(),
        };
        round_trip_json(&matcher);
        round_trip_toml(&matcher);
    }

    #[test]
    fn round_trips_in() {
        let matcher = Matcher::In {
            path: "type".into(),
            any_of: vec!["text".into(), "thinking".into()],
        };
        round_trip_json(&matcher);
        round_trip_toml(&matcher);
    }

    #[test]
    fn round_trips_not() {
        let matcher = Matcher::Not {
            matcher: Box::new(Matcher::Exists {
                path: "delta".into(),
            }),
        };
        round_trip_json(&matcher);
        round_trip_toml(&matcher);
    }

    #[test]
    fn op_tag_is_snake_case() {
        let json = serde_json::to_value(Matcher::Exists { path: "x".into() }).unwrap();
        assert_eq!(json["op"], "exists");
    }
}
