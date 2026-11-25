use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// Environment filtering for a spawned agent. Sorted collections keep serialization stable.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EnvConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow: Option<BTreeSet<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deny: Option<BTreeSet<String>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub overrides: BTreeMap<String, String>,
}

impl EnvConfig {
    /// A non-empty `allow`/`deny` on `over` wins, otherwise `base`'s is kept. `overrides` merge
    /// per key with `over` winning.
    pub fn merge(base: &EnvConfig, over: &EnvConfig) -> EnvConfig {
        let mut overrides = base.overrides.clone();
        overrides.extend(over.overrides.clone());
        EnvConfig {
            allow: pick_set(&base.allow, &over.allow),
            deny: pick_set(&base.deny, &over.deny),
            overrides,
        }
    }

    /// Folds a cascade, lowest precedence first.
    pub fn merge_all<'a>(configs: impl IntoIterator<Item = &'a EnvConfig>) -> EnvConfig {
        configs.into_iter().fold(EnvConfig::default(), |acc, next| {
            EnvConfig::merge(&acc, next)
        })
    }
}

fn pick_set(
    base: &Option<BTreeSet<String>>,
    over: &Option<BTreeSet<String>>,
) -> Option<BTreeSet<String>> {
    match over {
        Some(set) if !set.is_empty() => Some(set.clone()),
        _ => base.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> Option<BTreeSet<String>> {
        Some(items.iter().map(|s| s.to_string()).collect())
    }

    fn overrides(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn non_empty_override_allow_wins() {
        let base = EnvConfig {
            allow: set(&["A"]),
            ..Default::default()
        };
        let over = EnvConfig {
            allow: set(&["B"]),
            ..Default::default()
        };
        assert_eq!(EnvConfig::merge(&base, &over).allow, set(&["B"]));
    }

    #[test]
    fn empty_or_missing_override_allow_falls_back_to_base() {
        let base = EnvConfig {
            allow: set(&["A"]),
            ..Default::default()
        };
        let empty = EnvConfig {
            allow: set(&[]),
            ..Default::default()
        };
        assert_eq!(EnvConfig::merge(&base, &empty).allow, set(&["A"]));
        assert_eq!(
            EnvConfig::merge(&base, &EnvConfig::default()).allow,
            set(&["A"])
        );
    }

    #[test]
    fn deny_follows_the_same_precedence() {
        let base = EnvConfig {
            deny: set(&["X"]),
            ..Default::default()
        };
        let over = EnvConfig {
            deny: set(&["Y"]),
            ..Default::default()
        };
        assert_eq!(EnvConfig::merge(&base, &over).deny, set(&["Y"]));
        assert_eq!(
            EnvConfig::merge(&base, &EnvConfig::default()).deny,
            set(&["X"])
        );
    }

    #[test]
    fn overrides_merge_per_key_with_override_winning() {
        let base = EnvConfig {
            overrides: overrides(&[("A", "1"), ("B", "1")]),
            ..Default::default()
        };
        let over = EnvConfig {
            overrides: overrides(&[("B", "2"), ("C", "2")]),
            ..Default::default()
        };
        let merged = EnvConfig::merge(&base, &over);
        assert_eq!(
            merged.overrides,
            overrides(&[("A", "1"), ("B", "2"), ("C", "2")])
        );
    }

    #[test]
    fn merge_all_applies_lowest_precedence_first() {
        let low = EnvConfig {
            allow: set(&["A"]),
            overrides: overrides(&[("K", "low")]),
            ..Default::default()
        };
        let high = EnvConfig {
            overrides: overrides(&[("K", "high")]),
            ..Default::default()
        };
        let merged = EnvConfig::merge_all([&low, &high]);
        assert_eq!(merged.allow, set(&["A"]));
        assert_eq!(merged.overrides, overrides(&[("K", "high")]));
    }
}
