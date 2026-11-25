use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;

use tokio::process::{Child, Command};

use crate::feature::agent_config::EnvConfig;
use crate::feature::process::{ProcessError, resolve_env};

/// Everything needed to launch one agent process.
pub struct SpawnOptions<'a> {
    /// The rendered command; the prompt is appended as the final argument.
    pub cmd: &'a [String],
    pub prompt: &'a str,
    pub working_dir: Option<&'a Path>,
    pub env_config: Option<&'a EnvConfig>,
    pub extra_env: &'a HashMap<String, String>,
    /// Set after filtering so it always reaches the child.
    pub forced_env: &'a HashMap<String, String>,
}

/// Starts the agent with stdin on `/dev/null`, stdout and stderr piped, its own process group
/// (so terminal signals do not reach it), and `kill_on_drop`.
pub fn spawn(options: &SpawnOptions) -> Result<Child, ProcessError> {
    let (program, args) = options.cmd.split_first().ok_or(ProcessError::EmptyCmd)?;
    let env = resolve_env(
        std::env::vars(),
        options.extra_env,
        options.env_config,
        options.forced_env,
    );
    let mut command = Command::new(program);
    command
        .args(args)
        .arg(options.prompt)
        .env_clear()
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(dir) = options.working_dir {
        command.current_dir(dir);
    }
    #[cfg(unix)]
    command.process_group(0);
    command.spawn().map_err(ProcessError::Spawn)
}

/// True if a process with `pid` exists. `EPERM` means it exists but belongs to someone else.
#[cfg(unix)]
pub fn process_exists(pid: u32) -> bool {
    use nix::errno::Errno;
    use nix::sys::signal::kill;
    use nix::unistd::Pid;

    match kill(Pid::from_raw(pid as i32), None) {
        Ok(()) => true,
        Err(error) => error == Errno::EPERM,
    }
}

/// Asks the process to stop with `SIGTERM`.
#[cfg(unix)]
pub fn terminate(pid: u32) -> std::io::Result<()> {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    kill(Pid::from_raw(pid as i32), Signal::SIGTERM).map_err(std::io::Error::from)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use tempfile::TempDir;

    fn sh(script: &str) -> Vec<String> {
        ["sh", "-c", script, "agent"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn set(items: &[&str]) -> Option<BTreeSet<String>> {
        Some(items.iter().map(|s| s.to_string()).collect())
    }

    async fn run(cmd: &[String], prompt: &str, configure: impl FnOnce(&mut Config)) -> String {
        let mut config = Config::default();
        configure(&mut config);
        let options = SpawnOptions {
            cmd,
            prompt,
            working_dir: config.working_dir.as_deref(),
            env_config: config.env_config.as_ref(),
            extra_env: &config.extra_env,
            forced_env: &config.forced_env,
        };
        let child = spawn(&options).unwrap();
        let output = child.wait_with_output().await.unwrap();
        String::from_utf8(output.stdout).unwrap()
    }

    #[derive(Default)]
    struct Config {
        working_dir: Option<std::path::PathBuf>,
        env_config: Option<EnvConfig>,
        extra_env: HashMap<String, String>,
        forced_env: HashMap<String, String>,
    }

    const SHOW_ENV: &str = r#"echo "A=${A-unset} B=${B-unset} F=${F-unset}""#;

    #[tokio::test]
    async fn appends_the_prompt_as_the_final_argument() {
        let out = run(&sh(r#"printf '%s|%s' "$1" "$2""#), "hello world", |_| {}).await;
        assert_eq!(out, "hello world|");
        let cmd = [sh(r#"printf '%s|%s' "$1" "$2""#), vec!["first".into()]].concat();
        assert_eq!(run(&cmd, "last", |_| {}).await, "first|last");
    }

    #[tokio::test]
    async fn runs_in_the_working_directory() {
        let dir = TempDir::new().unwrap();
        let expected = dir.path().canonicalize().unwrap();
        let out = run(&sh("pwd -P"), "", |c| {
            c.working_dir = Some(dir.path().to_path_buf())
        })
        .await;
        assert_eq!(out.trim(), expected.to_str().unwrap());
    }

    #[tokio::test]
    async fn allow_list_drops_unlisted_variables() {
        let out = run(&sh(SHOW_ENV), "", |c| {
            c.extra_env = map(&[("A", "1"), ("B", "2")]);
            c.env_config = Some(EnvConfig {
                allow: set(&["A"]),
                ..Default::default()
            });
        })
        .await;
        assert_eq!(out.trim(), "A=1 B=unset F=unset");
    }

    #[tokio::test]
    async fn deny_list_removes_variables() {
        let out = run(&sh(SHOW_ENV), "", |c| {
            c.extra_env = map(&[("A", "1"), ("B", "2")]);
            c.env_config = Some(EnvConfig {
                deny: set(&["B"]),
                ..Default::default()
            });
        })
        .await;
        assert_eq!(out.trim(), "A=1 B=unset F=unset");
    }

    #[tokio::test]
    async fn overrides_replace_extra_env() {
        let out = run(&sh(SHOW_ENV), "", |c| {
            c.extra_env = map(&[("A", "extra")]);
            c.env_config = Some(EnvConfig {
                overrides: map(&[("A", "over")]).into_iter().collect(),
                ..Default::default()
            });
        })
        .await;
        assert!(out.starts_with("A=over"), "{out}");
    }

    #[tokio::test]
    async fn forced_env_survives_an_allow_list() {
        let out = run(&sh(SHOW_ENV), "", |c| {
            c.forced_env = map(&[("F", "forced")]);
            c.env_config = Some(EnvConfig {
                allow: set(&["A"]),
                ..Default::default()
            });
        })
        .await;
        assert_eq!(out.trim(), "A=unset B=unset F=forced");
    }

    #[tokio::test]
    async fn stdin_is_dev_null() {
        let out = run(&sh("cat; echo done"), "", |_| {}).await;
        assert_eq!(out.trim(), "done");
    }

    #[tokio::test]
    async fn child_runs_in_its_own_process_group() {
        let out = run(&sh(r#"ps -o pgid= -p $$; ps -o pid= -p $$"#), "", |_| {}).await;
        let numbers: Vec<&str> = out.split_whitespace().collect();
        assert_eq!(
            numbers[0], numbers[1],
            "child should lead its own group: {out}"
        );
    }

    #[tokio::test]
    async fn empty_cmd_is_an_error() {
        let options = SpawnOptions {
            cmd: &[],
            prompt: "x",
            working_dir: None,
            env_config: None,
            extra_env: &HashMap::new(),
            forced_env: &HashMap::new(),
        };
        assert!(matches!(spawn(&options), Err(ProcessError::EmptyCmd)));
    }

    #[tokio::test]
    async fn terminate_stops_a_running_process() {
        let cmd = sh("sleep 30");
        let options = SpawnOptions {
            cmd: &cmd,
            prompt: "",
            working_dir: None,
            env_config: None,
            extra_env: &HashMap::new(),
            forced_env: &HashMap::new(),
        };
        let mut child = spawn(&options).unwrap();
        let pid = child.id().unwrap();
        assert!(process_exists(pid));
        terminate(pid).unwrap();
        let status = child.wait().await.unwrap();
        assert!(!status.success());
        assert!(!process_exists(pid));
    }
}
