use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;

use crate::feature::output::{Destination, Format, OutputRoutes, Sink};
use crate::feature::run::{RunRequest, execute};

#[derive(Args)]
pub struct RunArgs {
    /// Bundled agent name (claude, codex, ...) or the path to an agent TOML file.
    #[arg(long)]
    agent: String,
    /// Working directory for the agent.
    #[arg(long)]
    cwd: Option<PathBuf>,
    /// Extra {{variable}} for the command template. Repeatable.
    #[arg(long = "var", value_name = "KEY=VALUE", value_parser = parse_var)]
    vars: Vec<(String, String)>,
    /// Session id to use; a UUID v4 is generated when omitted.
    #[arg(long)]
    session_id: Option<String>,
    /// Continue the session given by --session-id.
    #[arg(long, requires = "session_id")]
    resume: bool,
    /// Where stdout goes: FORMAT[:DEST], FORMAT is raw or events, DEST is stdout, stderr, or a
    /// file path (default stdout). Repeatable. Defaults to events on stdout.
    #[arg(long = "out", value_name = "FORMAT[:DEST]", value_parser = parse_out)]
    out: Vec<Sink>,
    /// Where the agent's stderr goes: stdout, stderr, or a file path. Repeatable. Defaults to
    /// this process's stderr.
    #[arg(long = "err", value_name = "DEST", value_parser = parse_err)]
    err: Vec<Sink>,
    /// The prompt, or `-` to read it from stdin. Put `--` before a prompt starting with `-`.
    prompt: String,
}

pub async fn run(args: RunArgs) -> ExitCode {
    match run_agent(args).await {
        Ok(code) => ExitCode::from((code & 0xff) as u8),
        Err(message) => {
            eprintln!("agent-translator: {message}");
            ExitCode::FAILURE
        }
    }
}

async fn run_agent(args: RunArgs) -> Result<i32, String> {
    let prompt = read_prompt(&args.prompt)?;
    let request = RunRequest {
        agent: args.agent,
        cwd: args.cwd,
        vars: args.vars.into_iter().collect(),
        session_id: args.session_id,
        resume: args.resume,
        routes: build_routes(args.out, args.err),
        prompt,
    };
    let outcome = execute(request).await.map_err(|error| error.to_string())?;
    if let Some(session_id) = outcome.captured_session_id {
        eprintln!("session_id: {session_id}");
    }
    Ok(outcome.exit_code)
}

fn read_prompt(prompt: &str) -> Result<String, String> {
    if prompt != "-" {
        return Ok(prompt.to_string());
    }
    let mut text = String::new();
    std::io::stdin()
        .read_to_string(&mut text)
        .map_err(|error| format!("failed to read the prompt from stdin: {error}"))?;
    Ok(text)
}

fn build_routes(mut out: Vec<Sink>, mut err: Vec<Sink>) -> OutputRoutes {
    if out.is_empty() {
        out.push(sink(Destination::Stdout, Format::Events));
    }
    if err.is_empty() {
        err.push(sink(Destination::Stderr, Format::Raw));
    }
    OutputRoutes {
        stdout: out,
        stderr: err,
    }
}

fn sink(destination: Destination, format: Format) -> Sink {
    Sink {
        destination,
        format,
    }
}

fn parse_var(raw: &str) -> Result<(String, String), String> {
    match raw.split_once('=') {
        Some((key, value)) if !key.is_empty() => Ok((key.to_string(), value.to_string())),
        _ => Err(format!("invalid --var '{raw}': expected KEY=VALUE")),
    }
}

fn parse_destination(raw: &str) -> Destination {
    match raw {
        "stdout" => Destination::Stdout,
        "stderr" => Destination::Stderr,
        path => Destination::File(PathBuf::from(path)),
    }
}

fn parse_out(raw: &str) -> Result<Sink, String> {
    let (format, dest) = raw.split_once(':').unwrap_or((raw, "stdout"));
    let format = match format {
        "raw" => Format::Raw,
        "events" => Format::Events,
        other => {
            return Err(format!(
                "invalid --out format '{other}': expected raw or events (as FORMAT[:DEST])"
            ));
        }
    };
    Ok(sink(parse_destination(dest), format))
}

fn parse_err(raw: &str) -> Result<Sink, String> {
    let format = raw.split_once(':').map_or(raw, |(format, _)| format);
    if format == "events" {
        return Err("the events format is not supported on --err; stderr is always raw".into());
    }
    let dest = raw.strip_prefix("raw:").unwrap_or(raw);
    Ok(sink(parse_destination(dest), Format::Raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn describe(sink: &Sink) -> (String, Format) {
        let dest = match &sink.destination {
            Destination::Stdout => "stdout".to_string(),
            Destination::Stderr => "stderr".to_string(),
            Destination::File(path) => path.display().to_string(),
            Destination::Channel(_) => "channel".to_string(),
        };
        (dest, sink.format)
    }

    #[test]
    fn out_defaults_the_destination_to_stdout() {
        assert_eq!(
            describe(&parse_out("events").unwrap()),
            ("stdout".into(), Format::Events)
        );
        assert_eq!(
            describe(&parse_out("raw").unwrap()),
            ("stdout".into(), Format::Raw)
        );
    }

    #[test]
    fn out_accepts_stdio_and_file_destinations() {
        assert_eq!(
            describe(&parse_out("raw:stderr").unwrap()),
            ("stderr".into(), Format::Raw)
        );
        assert_eq!(
            describe(&parse_out("events:/tmp/events.ndjson").unwrap()),
            ("/tmp/events.ndjson".into(), Format::Events)
        );
    }

    #[test]
    fn out_rejects_unknown_formats_with_a_clear_message() {
        let error = parse_out("xml:/tmp/x").unwrap_err();
        assert!(error.contains("invalid --out format 'xml'"), "{error}");
        assert!(parse_out("/tmp/x").is_err());
    }

    #[test]
    fn err_destinations_are_raw() {
        assert_eq!(
            describe(&parse_err("stdout").unwrap()),
            ("stdout".into(), Format::Raw)
        );
        assert_eq!(
            describe(&parse_err("/tmp/e.log").unwrap()),
            ("/tmp/e.log".into(), Format::Raw)
        );
        assert_eq!(
            describe(&parse_err("raw:/tmp/e.log").unwrap()),
            ("/tmp/e.log".into(), Format::Raw)
        );
    }

    #[test]
    fn err_rejects_the_events_format() {
        for raw in ["events", "events:/tmp/e.ndjson"] {
            let error = parse_err(raw).unwrap_err();
            assert!(
                error.contains("events format is not supported on --err"),
                "{error}"
            );
        }
    }

    #[test]
    fn var_requires_a_key_and_equals_sign() {
        assert_eq!(parse_var("a=b=c").unwrap(), ("a".into(), "b=c".into()));
        assert_eq!(parse_var("a=").unwrap(), ("a".into(), String::new()));
        assert!(parse_var("novalue").is_err());
        assert!(parse_var("=x").is_err());
    }

    #[test]
    fn default_routes_are_events_on_stdout_and_raw_stderr() {
        let routes = build_routes(vec![], vec![]);
        assert_eq!(
            describe(&routes.stdout[0]),
            ("stdout".into(), Format::Events)
        );
        assert_eq!(describe(&routes.stderr[0]), ("stderr".into(), Format::Raw));
    }

    #[test]
    fn explicit_out_sinks_replace_the_stdout_default() {
        let routes = build_routes(vec![parse_out("raw:/tmp/n").unwrap()], vec![]);
        assert_eq!(routes.stdout.len(), 1);
        assert_eq!(describe(&routes.stdout[0]).1, Format::Raw);
    }
}
