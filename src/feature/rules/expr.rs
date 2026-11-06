use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Describes how to build one field's value when emitting an event.
///
/// `#[serde(untagged)]` tries `Structured` first; `ExprBody::deny_unknown_fields` makes any
/// object with keys outside the recognized set fall through to `Literal` instead of silently
/// deserializing into an all-`None` `ExprBody`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Expr {
    Structured(ExprBody),
    Literal(Value),
}

/// Scalar/dynamic-valued fields are declared before array/table fields so the `toml` crate can
/// serialize this struct (it requires values to precede tables within a struct).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ExprBody {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sep: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concat: Option<Vec<Expr>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map_path: Option<MapPath>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapPath {
    pub from: String,
    pub pluck: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn round_trip_json(expr: &Expr) {
        let serialized = serde_json::to_string(expr).unwrap();
        let deserialized: Expr = serde_json::from_str(&serialized).unwrap();
        assert_eq!(expr, &deserialized);
    }

    fn round_trip_toml(expr: &Expr) {
        let serialized = toml::to_string(expr).unwrap();
        let deserialized: Expr = toml::from_str(&serialized).unwrap();
        assert_eq!(expr, &deserialized);
    }

    #[test]
    fn round_trips_path_expr() {
        let expr = Expr::Structured(ExprBody {
            path: Some("message.content".into()),
            ..Default::default()
        });
        round_trip_json(&expr);
        round_trip_toml(&expr);
    }

    #[test]
    fn round_trips_concat_expr() {
        let expr = Expr::Structured(ExprBody {
            sep: Some(" ".into()),
            concat: Some(vec![
                Expr::Structured(ExprBody {
                    path: Some("a".into()),
                    ..Default::default()
                }),
                Expr::Literal(Value::String("b".into())),
            ]),
            ..Default::default()
        });
        round_trip_json(&expr);
        round_trip_toml(&expr);
    }

    #[test]
    fn round_trips_map_path_expr() {
        let expr = Expr::Structured(ExprBody {
            map_path: Some(MapPath {
                from: "tools".into(),
                pluck: "name".into(),
            }),
            ..Default::default()
        });
        round_trip_json(&expr);
        round_trip_toml(&expr);
    }

    #[test]
    fn round_trips_default_value_expr() {
        let expr = Expr::Structured(ExprBody {
            path: Some("x".into()),
            default: Some(json!(false)),
            ..Default::default()
        });
        round_trip_json(&expr);
        round_trip_toml(&expr);
    }

    #[test]
    fn round_trips_scalar_literal() {
        let expr = Expr::Literal(json!("hello"));
        round_trip_json(&expr);
    }

    #[test]
    fn object_literal_falls_through_to_literal_not_empty_expr_body() {
        let raw = r#"{"foo": "bar"}"#;
        let expr: Expr = serde_json::from_str(raw).unwrap();
        assert_eq!(expr, Expr::Literal(json!({"foo": "bar"})));
    }

    #[test]
    fn recognized_keys_parse_as_structured() {
        let raw = r#"{"path": "a"}"#;
        let expr: Expr = serde_json::from_str(raw).unwrap();
        assert_eq!(
            expr,
            Expr::Structured(ExprBody {
                path: Some("a".into()),
                ..Default::default()
            })
        );
    }
}
