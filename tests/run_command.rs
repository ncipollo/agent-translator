//! End-to-end tests of `agent-translator run` against a fake agent (a `sh` one-liner).

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use tempfile::TempDir;

const RULES: &str = r#"[
  {"when":[{"op":"eq","path":"type","value":"init"}],
   "emit":{"event":"system_init","fields":{"session_id":{"path":"sid"}}}},
  {"when":[{"op":"eq","path":"type","value":"say"}],
   "emit":{"event":"text","fields":{"text":{"path":"t"}}}}
]"#;

const NDJSON_AGENT: &str =
    r#"printf '{"type":"init","sid":"s1"}\n{"type":"say","t":"hi"}\n'; echo diag >&2; exit 7"#;

fn write_agent(dir: &Path, script: &str, extra: &str) -> String {
    fs::write(dir.join("rules.json"), RULES).unwrap();
    let config = format!(
        "cmd = [\"sh\", \"-c\", {script:?}, \"agent\"]\n{extra}\n[output_parsing]\ntype = \"typed\"\nrules_file = \"rules.json\"\n"
    );
    let path = dir.join("agent.toml");
    fs::write(&path, config).unwrap();
    path.to_str().unwrap().to_string()
}

fn write_raw_agent(dir: &Path, script: &str, extra: &str) -> String {
    let config = format!("cmd = [\"sh\", \"-c\", {script:?}, \"agent\"]\n{extra}\n");
    let path = dir.join("raw.toml");
    fs::write(&path, config).unwrap();
    path.to_str().unwrap().to_string()
}

fn run(args: &[&str], stdin: Option<&str>) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_agent-translator"))
        .arg("run")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(text) = stdin {
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }
    drop(child.stdin.take());
    child.wait_with_output().unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

fn event_types(raw: &str) -> Vec<String> {
    raw.lines()
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line).unwrap()["type"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect()
}

#[test]
fn defaults_to_translated_events_on_stdout_and_mirrors_the_exit_code() {
    let dir = TempDir::new().unwrap();
    let agent = write_agent(dir.path(), NDJSON_AGENT, "");
    let output = run(&["--agent", &agent, "--", "go"], None);
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(event_types(&stdout(&output)), ["system_init", "text"]);
    assert!(stderr(&output).contains("diag"));
}

#[test]
fn writes_translated_events_and_the_raw_stream_to_separate_sinks() {
    let dir = TempDir::new().unwrap();
    let agent = write_agent(dir.path(), NDJSON_AGENT, "");
    let native = dir.path().join("native.ndjson");
    let events = dir.path().join("events.ndjson");
    let output = run(
        &[
            "--agent",
            &agent,
            "--out",
            "events",
            "--out",
            &format!("raw:{}", native.display()),
            "--out",
            &format!("events:{}", events.display()),
            "--",
            "go",
        ],
        None,
    );
    assert_eq!(event_types(&stdout(&output)), ["system_init", "text"]);
    assert_eq!(
        fs::read_to_string(&native).unwrap(),
        "{\"type\":\"init\",\"sid\":\"s1\"}\n{\"type\":\"say\",\"t\":\"hi\"}\n"
    );
    assert_eq!(fs::read_to_string(&events).unwrap(), stdout(&output));
}

#[test]
fn err_can_be_redirected_to_a_file() {
    let dir = TempDir::new().unwrap();
    let agent = write_agent(dir.path(), NDJSON_AGENT, "");
    let log = dir.path().join("err.log");
    let output = run(
        &[
            "--agent",
            &agent,
            "--err",
            log.to_str().unwrap(),
            "--",
            "go",
        ],
        None,
    );
    assert_eq!(fs::read_to_string(&log).unwrap(), "diag\n");
    assert!(!stderr(&output).contains("diag"));
}

#[test]
fn prints_the_captured_session_id_when_the_agent_mints_its_own() {
    let dir = TempDir::new().unwrap();
    let agent = write_agent(dir.path(), NDJSON_AGENT, "parse_session_id = true");
    let output = run(&["--agent", &agent, "--", "go"], None);
    assert!(
        stderr(&output).contains("session_id: s1"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn the_prompt_is_one_argv_entry_never_interpolated_into_a_shell() {
    let dir = TempDir::new().unwrap();
    let agent = write_raw_agent(dir.path(), r#"printf '%s' "$1""#, "");
    let prompt = "a b; echo pwned $(id)";
    let output = run(&["--agent", &agent, "--out", "raw", "--", prompt], None);
    assert_eq!(stdout(&output), prompt);
}

#[test]
fn a_dash_prompt_is_read_from_stdin() {
    let dir = TempDir::new().unwrap();
    let agent = write_raw_agent(dir.path(), r#"printf '%s' "$1""#, "");
    let output = run(
        &["--agent", &agent, "--out", "raw", "--", "-"],
        Some("from stdin"),
    );
    assert_eq!(stdout(&output), "from stdin");
}

#[test]
fn resume_uses_the_headless_resume_template_with_the_given_session() {
    let dir = TempDir::new().unwrap();
    let extra = r#"headless_resume = ["sh", "-c", "printf '%s' \"$*\"", "agent", "--resume", "{{session_id}}"]"#;
    let agent = write_raw_agent(dir.path(), "exit 1", extra);
    let output = run(
        &[
            "--agent",
            &agent,
            "--session-id",
            "abc",
            "--resume",
            "--out",
            "raw",
            "--",
            "hello",
        ],
        None,
    );
    assert_eq!(stdout(&output), "--resume abc hello");
}

#[test]
fn vars_and_session_id_fill_the_command_template() {
    let dir = TempDir::new().unwrap();
    let config = "cmd = [\"sh\", \"-c\", \"printf '%s|%s' \\\"$1\\\" \\\"$2\\\"\", \"agent\", \"{{who}}\", \"{{session_id}}\"]\n";
    let path = dir.path().join("vars.toml");
    fs::write(&path, config).unwrap();
    let output = run(
        &[
            "--agent",
            path.to_str().unwrap(),
            "--var",
            "who=me",
            "--session-id",
            "S9",
            "--out",
            "raw",
            "--",
            "p",
        ],
        None,
    );
    assert_eq!(stdout(&output), "me|S9");
}

#[test]
fn invalid_out_format_is_a_clear_argument_error() {
    let output = run(&["--agent", "claude", "--out", "xml", "--", "go"], None);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("invalid --out format 'xml'"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn an_events_sink_on_err_is_a_clear_argument_error() {
    let output = run(&["--agent", "claude", "--err", "events", "--", "go"], None);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("events format is not supported on --err"));
}

#[test]
fn resume_requires_a_session_id() {
    let output = run(&["--agent", "claude", "--resume", "--", "go"], None);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn unknown_agents_and_missing_commands_fail_with_a_one_line_message() {
    let output = run(&["--agent", "nope", "--", "go"], None);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).starts_with("agent-translator: unknown default agent 'nope'"));
    assert_eq!(stderr(&output).trim().lines().count(), 1);

    let dir = TempDir::new().unwrap();
    let missing = dir.path().join("agent.toml");
    fs::write(&missing, "cmd = [\"definitely-not-a-real-binary-xyz\"]\n").unwrap();
    let output = run(&["--agent", missing.to_str().unwrap(), "--", "go"], None);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("failed to spawn agent process"));
}
