use serde_json::Value;

/// Resolves a dot-separated path. Numeric segments index arrays; `"."` is the whole scope.
pub fn resolve<'a>(scope: &'a Value, path: &str) -> Option<&'a Value> {
    if path == "." {
        return Some(scope);
    }
    path.split('.')
        .try_fold(scope, |current, segment| match current {
            Value::Object(map) => map.get(segment),
            Value::Array(items) => segment.parse::<usize>().ok().and_then(|i| items.get(i)),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resolves_nested_objects_and_array_indexes() {
        let value = json!({"a": {"b": [10, {"c": "deep"}]}});
        assert_eq!(resolve(&value, "a.b.0"), Some(&json!(10)));
        assert_eq!(resolve(&value, "a.b.1.c"), Some(&json!("deep")));
    }

    #[test]
    fn dot_is_the_whole_scope() {
        let value = json!([1, 2]);
        assert_eq!(resolve(&value, "."), Some(&value));
    }

    #[test]
    fn missing_segments_and_bad_indexes_are_none() {
        let value = json!({"a": [1], "s": "str"});
        assert_eq!(resolve(&value, "x"), None);
        assert_eq!(resolve(&value, "a.5"), None);
        assert_eq!(resolve(&value, "a.k"), None);
        assert_eq!(resolve(&value, "s.len"), None);
    }
}
