use serde_json::Value;

use crate::feature::rules::{Expr, ExprBody, MapPath};
use crate::feature::translate::path::resolve;

/// Evaluates a field expression against `scope`. `None` means the field has no value and
/// should be omitted from the event.
pub fn evaluate(expr: &Expr, scope: &Value) -> Option<Value> {
    match expr {
        Expr::Literal(value) => Some(value.clone()),
        Expr::Structured(body) => evaluate_body(body, scope),
    }
}

fn evaluate_body(body: &ExprBody, scope: &Value) -> Option<Value> {
    let value = if let Some(constant) = &body.value {
        Some(constant.clone())
    } else if let Some(path) = &body.path {
        resolve(scope, path).filter(|v| !v.is_null()).cloned()
    } else if let Some(parts) = &body.concat {
        concat(parts, body.sep.as_deref().unwrap_or(""), scope)
    } else if let Some(map_path) = &body.map_path {
        pluck(map_path, scope)
    } else {
        None
    };
    value.or_else(|| body.default.clone())
}

fn concat(parts: &[Expr], sep: &str, scope: &Value) -> Option<Value> {
    let pieces: Vec<String> = parts
        .iter()
        .filter_map(|part| evaluate(part, scope))
        .filter_map(|value| stringify(&value))
        .collect();
    (!pieces.is_empty()).then(|| Value::String(pieces.join(sep)))
}

fn stringify(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        Value::Number(_) | Value::Bool(_) => Some(value.to_string()),
        Value::Array(_) | Value::Object(_) => serde_json::to_string(value).ok(),
    }
}

fn pluck(map_path: &MapPath, scope: &Value) -> Option<Value> {
    let items = resolve(scope, &map_path.from)?.as_array()?;
    let plucked = items
        .iter()
        .filter_map(|item| resolve(item, &map_path.pluck))
        .cloned()
        .collect();
    Some(Value::Array(plucked))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn expr(raw: Value) -> Expr {
        serde_json::from_value(raw).unwrap()
    }

    #[test]
    fn literal_and_value_return_constants() {
        let scope = json!({});
        assert_eq!(evaluate(&expr(json!("hi")), &scope), Some(json!("hi")));
        assert_eq!(
            evaluate(&expr(json!({"value": true})), &scope),
            Some(json!(true))
        );
    }

    #[test]
    fn path_uses_default_when_missing_or_null() {
        let scope = json!({"a": null, "b": "x"});
        let with_default = |p: &str| expr(json!({"path": p, "default": "fallback"}));
        assert_eq!(
            evaluate(&with_default("a"), &scope),
            Some(json!("fallback"))
        );
        assert_eq!(
            evaluate(&with_default("zzz"), &scope),
            Some(json!("fallback"))
        );
        assert_eq!(evaluate(&with_default("b"), &scope), Some(json!("x")));
        assert_eq!(evaluate(&expr(json!({"path": "zzz"})), &scope), None);
    }

    #[test]
    fn concat_joins_stringified_parts_with_separator() {
        let scope = json!({"n": 3, "ok": true, "gone": null, "obj": {"k": 1}, "s": "txt"});
        let joined = expr(json!({
            "sep": "|",
            "concat": [{"path": "s"}, {"path": "n"}, {"path": "ok"}, {"path": "gone"}, {"path": "obj"}, "lit"]
        }));
        assert_eq!(
            evaluate(&joined, &scope),
            Some(json!("txt|3|true|{\"k\":1}|lit"))
        );
    }

    #[test]
    fn concat_without_separator_joins_directly() {
        let scope = json!({"a": "x", "b": "y"});
        let joined = expr(json!({"concat": [{"path": "a"}, {"path": "b"}]}));
        assert_eq!(evaluate(&joined, &scope), Some(json!("xy")));
    }

    #[test]
    fn empty_concat_falls_back_to_default() {
        let scope = json!({});
        let joined = expr(json!({"concat": [{"path": "a"}], "default": "none"}));
        assert_eq!(evaluate(&joined, &scope), Some(json!("none")));
    }

    #[test]
    fn map_path_plucks_from_each_array_element() {
        let scope = json!({"tools": [{"name": "a"}, {"other": 1}, {"name": "b"}]});
        let plucked = expr(json!({"map_path": {"from": "tools", "pluck": "name"}}));
        assert_eq!(evaluate(&plucked, &scope), Some(json!(["a", "b"])));
    }

    #[test]
    fn map_path_over_a_non_array_is_none() {
        let scope = json!({"tools": "nope"});
        let plucked = expr(json!({"map_path": {"from": "tools", "pluck": "name"}}));
        assert_eq!(evaluate(&plucked, &scope), None);
    }
}
