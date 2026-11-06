use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::feature::rules::Expr;

/// Describes which event to emit and how to build each of its fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Extractor {
    pub event: String,
    pub fields: BTreeMap<String, Expr>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feature::rules::ExprBody;

    #[test]
    fn round_trips_json() {
        let mut fields = BTreeMap::new();
        fields.insert(
            "text".to_string(),
            Expr::Structured(ExprBody {
                path: Some("message.content".into()),
                ..Default::default()
            }),
        );
        let extractor = Extractor {
            event: "text".into(),
            fields,
        };
        let serialized = serde_json::to_string(&extractor).unwrap();
        let deserialized: Extractor = serde_json::from_str(&serialized).unwrap();
        assert_eq!(extractor, deserialized);
    }

    #[test]
    fn round_trips_toml() {
        let mut fields = BTreeMap::new();
        fields.insert(
            "text".to_string(),
            Expr::Structured(ExprBody {
                path: Some("message.content".into()),
                ..Default::default()
            }),
        );
        let extractor = Extractor {
            event: "text".into(),
            fields,
        };
        let serialized = toml::to_string(&extractor).unwrap();
        let deserialized: Extractor = toml::from_str(&serialized).unwrap();
        assert_eq!(extractor, deserialized);
    }
}
