use std::collections::HashMap;

use crate::feature::agent_config::EnvConfig;

/// Builds the child's environment: the inherited variables, then `extra_env`, then the
/// config's `overrides`; then `allow` (when non-empty, only listed names survive) and `deny`
/// filtering; and finally `forced_env`, which always survives.
pub fn resolve_env(
    inherited: impl IntoIterator<Item = (String, String)>,
    extra_env: &HashMap<String, String>,
    env_config: Option<&EnvConfig>,
    forced_env: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut env: HashMap<String, String> = inherited.into_iter().collect();
    env.extend(extra_env.clone());
    if let Some(config) = env_config {
        env.extend(config.overrides.clone());
        if let Some(allow) = config.allow.as_ref().filter(|set| !set.is_empty()) {
            env.retain(|name, _| allow.contains(name));
        }
        for name in config.deny.iter().flatten() {
            env.remove(name);
        }
    }
    env.extend(forced_env.clone());
    env
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn set(items: &[&str]) -> Option<BTreeSet<String>> {
        Some(items.iter().map(|s| s.to_string()).collect())
    }

    fn inherited() -> Vec<(String, String)> {
        map(&[("A", "inherited"), ("B", "inherited")])
            .into_iter()
            .collect()
    }

    #[test]
    fn without_config_inherits_and_applies_extra_env() {
        let env = resolve_env(
            inherited(),
            &map(&[("B", "extra"), ("C", "extra")]),
            None,
            &map(&[]),
        );
        assert_eq!(
            env,
            map(&[("A", "inherited"), ("B", "extra"), ("C", "extra")])
        );
    }

    #[test]
    fn overrides_beat_extra_env() {
        let config = EnvConfig {
            overrides: map(&[("B", "over")]).into_iter().collect(),
            ..Default::default()
        };
        let env = resolve_env(
            inherited(),
            &map(&[("B", "extra")]),
            Some(&config),
            &map(&[]),
        );
        assert_eq!(env["B"], "over");
    }

    #[test]
    fn allow_list_drops_unlisted_variables() {
        let config = EnvConfig {
            allow: set(&["A"]),
            ..Default::default()
        };
        let env = resolve_env(inherited(), &map(&[]), Some(&config), &map(&[]));
        assert_eq!(env, map(&[("A", "inherited")]));
    }

    #[test]
    fn deny_list_removes_variables() {
        let config = EnvConfig {
            deny: set(&["B"]),
            ..Default::default()
        };
        let env = resolve_env(inherited(), &map(&[]), Some(&config), &map(&[]));
        assert_eq!(env, map(&[("A", "inherited")]));
    }

    #[test]
    fn forced_env_survives_allow_and_deny() {
        let config = EnvConfig {
            allow: set(&["A"]),
            deny: set(&["F"]),
            ..Default::default()
        };
        let env = resolve_env(
            inherited(),
            &map(&[]),
            Some(&config),
            &map(&[("F", "forced")]),
        );
        assert_eq!(env, map(&[("A", "inherited"), ("F", "forced")]));
    }

    #[test]
    fn empty_allow_list_is_ignored() {
        let config = EnvConfig {
            allow: set(&[]),
            ..Default::default()
        };
        let env = resolve_env(inherited(), &map(&[]), Some(&config), &map(&[]));
        assert_eq!(env.len(), 2);
    }
}
