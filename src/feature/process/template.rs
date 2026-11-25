use std::collections::HashMap;

/// Replaces `{{ key }}` tokens (whitespace inside the braces is trimmed). Unknown keys and an
/// unterminated `{{` are left as-is.
pub fn render(template: &str, vars: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after_open = &rest[start + 2..];
        let Some(end) = after_open.find("}}") else {
            out.push_str(&rest[start..]);
            return out;
        };
        let token_len = start + 2 + end + 2;
        match vars.get(after_open[..end].trim()) {
            Some(value) => out.push_str(value),
            None => out.push_str(&rest[start..token_len]),
        }
        rest = &rest[token_len..];
    }
    out.push_str(rest);
    out
}

/// Renders every element of a command array.
pub fn render_cmd(cmd: &[String], vars: &HashMap<String, String>) -> Vec<String> {
    cmd.iter().map(|part| render(part, vars)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn substitutes_tokens_and_trims_whitespace() {
        let v = vars(&[("session_id", "abc")]);
        assert_eq!(render("--id={{session_id}}", &v), "--id=abc");
        assert_eq!(render("--id={{  session_id }}", &v), "--id=abc");
    }

    #[test]
    fn substitutes_repeated_tokens() {
        let v = vars(&[("a", "1")]);
        assert_eq!(render("{{a}}-{{a}}-{{a}}", &v), "1-1-1");
    }

    #[test]
    fn unknown_tokens_are_preserved() {
        let v = vars(&[("a", "1")]);
        assert_eq!(render("{{a}} {{ missing }}", &v), "1 {{ missing }}");
    }

    #[test]
    fn unterminated_braces_are_preserved() {
        let v = vars(&[("a", "1")]);
        assert_eq!(render("x {{a}} y {{a", &v), "x 1 y {{a");
        assert_eq!(render("{{", &v), "{{");
    }

    #[test]
    fn renders_each_command_element() {
        let v = vars(&[("session_id", "s1")]);
        let cmd = vec![
            "tool".to_string(),
            "--resume".into(),
            "{{session_id}}".into(),
        ];
        assert_eq!(render_cmd(&cmd, &v), ["tool", "--resume", "s1"]);
    }
}
