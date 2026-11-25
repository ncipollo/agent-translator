use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Child;
use tokio::sync::mpsc;

use crate::feature::agent_config::OutputParsing;
use crate::feature::event::AgentEvent;
use crate::feature::output::{
    OpenWriter, OutputError, OutputRoutes, RunSummary, StreamSinks, open_routes,
};
use crate::feature::rules::MapRule;
use crate::feature::translate::{EventMapper, LineFramer};

#[derive(Clone, Copy)]
enum Stream {
    Stdout,
    Stderr,
}

struct Chunk {
    stream: Stream,
    bytes: Vec<u8>,
}

/// Opens the sinks in `routes`, drains the child's stdout and stderr, and waits for it to exit.
pub async fn route(
    child: &mut Child,
    routes: &OutputRoutes,
    parsing: &OutputParsing,
    rules: Vec<MapRule>,
) -> Result<RunSummary, OutputError> {
    let sinks = open_routes(routes).await?;
    let stdout = child.stdout.take().ok_or_else(|| missing_pipe("stdout"))?;
    let stderr = child.stderr.take().ok_or_else(|| missing_pipe("stderr"))?;
    let session_id = route_streams(stdout, stderr, sinks, parsing, rules).await;
    let exit_status = child.wait().await?;
    Ok(RunSummary {
        exit_status,
        session_id,
    })
}

fn missing_pipe(name: &str) -> std::io::Error {
    std::io::Error::other(format!("child {name} is not piped"))
}

/// Drains both readers concurrently into the sinks and returns the first captured session id.
/// Taking readers rather than a process keeps this testable with in-memory streams.
pub async fn route_streams<O, E>(
    stdout: O,
    stderr: E,
    sinks: StreamSinks,
    parsing: &OutputParsing,
    rules: Vec<MapRule>,
) -> Option<String>
where
    O: AsyncRead + Unpin + Send,
    E: AsyncRead + Unpin + Send,
{
    let (tx, mut rx) = mpsc::channel::<Chunk>(64);
    let pump = async move {
        tokio::join!(
            pump_stream(stdout, Stream::Stdout, tx.clone()),
            pump_stream(stderr, Stream::Stderr, tx)
        );
    };
    let consume = async {
        let mut router =
            StreamRouter::new(sinks, matches!(parsing, OutputParsing::Typed { .. }), rules);
        while let Some(chunk) = rx.recv().await {
            router.handle(chunk).await;
        }
        router.finish().await
    };
    let ((), session_id) = tokio::join!(pump, consume);
    session_id
}

async fn pump_stream<R: AsyncRead + Unpin>(mut reader: R, stream: Stream, tx: mpsc::Sender<Chunk>) {
    let mut buffer = [0u8; 8192];
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(n) => {
                let chunk = Chunk {
                    stream,
                    bytes: buffer[..n].to_vec(),
                };
                if tx.send(chunk).await.is_err() {
                    return;
                }
            }
        }
    }
}

struct StreamRouter {
    stdout_raw: Vec<OpenWriter>,
    stderr_raw: Vec<OpenWriter>,
    events: crate::feature::output::EventFanout,
    typed: bool,
    mapper: EventMapper,
    framer: LineFramer,
    full_stdout: Vec<u8>,
    session_id: Option<String>,
}

impl StreamRouter {
    fn new(sinks: StreamSinks, typed: bool, rules: Vec<MapRule>) -> Self {
        Self {
            stdout_raw: sinks.stdout_raw,
            stderr_raw: sinks.stderr_raw,
            events: sinks.events,
            typed,
            mapper: EventMapper::new(rules),
            framer: LineFramer::new(),
            full_stdout: Vec::new(),
            session_id: None,
        }
    }

    async fn handle(&mut self, chunk: Chunk) {
        match chunk.stream {
            Stream::Stdout => self.handle_stdout(&chunk.bytes).await,
            Stream::Stderr => write_all(&mut self.stderr_raw, &chunk.bytes).await,
        }
    }

    async fn handle_stdout(&mut self, bytes: &[u8]) {
        write_all(&mut self.stdout_raw, bytes).await;
        if self.typed {
            for line in self.framer.push(bytes) {
                for event in self.mapper.map_line(&line) {
                    self.emit(event).await;
                }
            }
        } else if !self.events.is_empty() {
            self.full_stdout.extend_from_slice(bytes);
        }
    }

    async fn finish(mut self) -> Option<String> {
        if self.typed {
            if let Some(partial) = self.framer.flush() {
                self.emit(incomplete_line(&partial)).await;
            }
        } else if !self.full_stdout.is_empty() {
            let text = String::from_utf8_lossy(&self.full_stdout).into_owned();
            self.emit(AgentEvent::Text { text, delta: false }).await;
        }
        self.session_id
    }

    async fn emit(&mut self, event: AgentEvent) {
        if let AgentEvent::SystemInit { session_id, .. } = &event
            && self.session_id.is_none()
            && !session_id.is_empty()
        {
            self.session_id = Some(session_id.clone());
        }
        self.events.emit(&event).await;
    }
}

fn incomplete_line(partial: &[u8]) -> AgentEvent {
    AgentEvent::Unknown {
        raw: Value::String(String::from_utf8_lossy(partial).into_owned()),
        reason: Some("incomplete_line_at_eof".into()),
    }
}

async fn write_all(writers: &mut [OpenWriter], bytes: &[u8]) {
    for writer in writers {
        writer.write(bytes).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feature::event::EventEnvelope;
    use crate::feature::output::{ChannelSink, Destination, Format, Sink};
    use crate::feature::process::{SpawnOptions, spawn};
    use std::collections::HashMap;
    use tokio::io::{AsyncWriteExt, DuplexStream};

    const CAP: usize = 1 << 16;

    fn rules() -> Vec<MapRule> {
        serde_json::from_str(
            r#"[
              {"when":[{"op":"eq","path":"type","value":"init"}],
               "emit":{"event":"system_init","fields":{"session_id":{"path":"sid"}}}},
              {"when":[{"op":"eq","path":"type","value":"say"}],
               "emit":{"event":"text","fields":{"text":{"path":"t"}}}}
            ]"#,
        )
        .unwrap()
    }

    fn typed() -> OutputParsing {
        OutputParsing::Typed { rules_file: None }
    }

    async fn capture(mut reader: DuplexStream) -> String {
        let mut out = String::new();
        tokio::io::AsyncReadExt::read_to_string(&mut reader, &mut out)
            .await
            .unwrap();
        out
    }

    fn envelopes(raw: &str) -> Vec<EventEnvelope> {
        raw.lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    async fn run(stdout: &[u8], sinks: StreamSinks, parsing: OutputParsing) -> Option<String> {
        route_streams(stdout, &b""[..], sinks, &parsing, rules()).await
    }

    #[tokio::test]
    async fn raw_sinks_receive_each_stream_independently_and_in_order() {
        let (so, so_r) = tokio::io::duplex(CAP);
        let (se, se_r) = tokio::io::duplex(CAP);
        let mut sinks = StreamSinks::default();
        sinks.stdout_raw.push(OpenWriter::new("out", so));
        sinks.stderr_raw.push(OpenWriter::new("err", se));
        route_streams(
            &b"out-1\nout-2\n"[..],
            &b"warn\n"[..],
            sinks,
            &typed(),
            rules(),
        )
        .await;
        assert_eq!(capture(so_r).await, "out-1\nout-2\n");
        assert_eq!(capture(se_r).await, "warn\n");
    }

    #[tokio::test]
    async fn event_sinks_get_increasing_seq_and_matching_content() {
        let (a, a_r) = tokio::io::duplex(CAP);
        let (b, b_r) = tokio::io::duplex(CAP);
        let mut sinks = StreamSinks::default();
        sinks.events.add_writer(OpenWriter::new("a", a));
        sinks.events.add_writer(OpenWriter::new("b", b));
        let stdout = b"{\"type\":\"say\",\"t\":\"one\"}\n{\"type\":\"say\",\"t\":\"two\"}\n";
        run(stdout, sinks, typed()).await;
        let (ea, eb) = (
            envelopes(&capture(a_r).await),
            envelopes(&capture(b_r).await),
        );
        assert_eq!(ea.iter().map(|e| e.seq).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(
            ea.iter().map(|e| e.event.clone()).collect::<Vec<_>>(),
            eb.iter().map(|e| e.event.clone()).collect::<Vec<_>>()
        );
        assert_eq!(
            ea[1].event,
            AgentEvent::Text {
                text: "two".into(),
                delta: false
            }
        );
    }

    #[tokio::test]
    async fn lines_split_across_chunks_are_reassembled() {
        let (w, w_r) = tokio::io::duplex(CAP);
        let mut sinks = StreamSinks::default();
        sinks.events.add_writer(OpenWriter::new("w", w));
        let (mut feed, stdout) = tokio::io::duplex(CAP);
        let writer = tokio::spawn(async move {
            feed.write_all(b"{\"type\":\"say\",").await.unwrap();
            tokio::task::yield_now().await;
            feed.write_all(b"\"t\":\"joined\"}\n").await.unwrap();
        });
        route_streams(stdout, &b""[..], sinks, &typed(), rules()).await;
        writer.await.unwrap();
        let events = envelopes(&capture(w_r).await);
        assert_eq!(
            events[0].event,
            AgentEvent::Text {
                text: "joined".into(),
                delta: false
            }
        );
    }

    #[tokio::test]
    async fn trailing_partial_line_becomes_an_unknown_event() {
        let (w, w_r) = tokio::io::duplex(CAP);
        let mut sinks = StreamSinks::default();
        sinks.events.add_writer(OpenWriter::new("w", w));
        run(b"{\"type\":\"say\"", sinks, typed()).await;
        let events = envelopes(&capture(w_r).await);
        assert_eq!(
            events[0].event,
            AgentEvent::Unknown {
                raw: Value::String("{\"type\":\"say\"".into()),
                reason: Some("incomplete_line_at_eof".into())
            }
        );
    }

    #[tokio::test]
    async fn raw_parsing_with_an_event_sink_emits_the_full_stdout_as_text() {
        let (w, w_r) = tokio::io::duplex(CAP);
        let mut sinks = StreamSinks::default();
        sinks.events.add_writer(OpenWriter::new("w", w));
        run(b"plain line 1\nplain line 2\n", sinks, OutputParsing::Raw).await;
        let events = envelopes(&capture(w_r).await);
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].event,
            AgentEvent::Text {
                text: "plain line 1\nplain line 2\n".into(),
                delta: false
            }
        );
    }

    #[tokio::test]
    async fn session_id_comes_from_the_first_system_init_only() {
        let stdout = b"{\"type\":\"say\",\"t\":\"x\"}\n{\"type\":\"init\",\"sid\":\"first\"}\n{\"type\":\"init\",\"sid\":\"second\"}\n";
        assert_eq!(
            run(stdout, StreamSinks::default(), typed()).await,
            Some("first".into())
        );
        assert_eq!(
            run(b"nothing\n", StreamSinks::default(), typed()).await,
            None
        );
    }

    #[tokio::test]
    async fn channel_sink_observes_events_live() {
        let (tx, mut rx) = mpsc::channel(8);
        let routes = OutputRoutes {
            stdout: vec![Sink {
                destination: Destination::Channel(ChannelSink(tx)),
                format: Format::Events,
            }],
            stderr: vec![],
        };
        let sinks = open_routes(&routes).await.unwrap();
        run(b"{\"type\":\"say\",\"t\":\"live\"}\n", sinks, typed()).await;
        assert_eq!(
            rx.recv().await,
            Some(AgentEvent::Text {
                text: "live".into(),
                delta: false
            })
        );
    }

    #[tokio::test]
    async fn route_waits_for_the_child_and_reports_exit_status_and_session() {
        let cmd: Vec<String> = [
            "sh",
            "-c",
            r#"echo '{"type":"init","sid":"abc"}'; echo oops >&2; exit 3"#,
            "agent",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let none = HashMap::new();
        let mut child = spawn(&SpawnOptions {
            cmd: &cmd,
            prompt: "",
            working_dir: None,
            env_config: None,
            extra_env: &none,
            forced_env: &none,
        })
        .unwrap();
        let routes = OutputRoutes {
            stdout: vec![Sink {
                destination: Destination::Stdout,
                format: Format::Raw,
            }],
            stderr: vec![],
        };
        let summary = route(&mut child, &routes, &typed(), rules()).await.unwrap();
        assert_eq!(summary.exit_status.code(), Some(3));
        assert_eq!(summary.session_id.as_deref(), Some("abc"));
    }

    #[tokio::test]
    async fn route_rejects_events_on_stderr() {
        let cmd: Vec<String> = ["true"].iter().map(|s| s.to_string()).collect();
        let none = HashMap::new();
        let mut child = spawn(&SpawnOptions {
            cmd: &cmd,
            prompt: "",
            working_dir: None,
            env_config: None,
            extra_env: &none,
            forced_env: &none,
        })
        .unwrap();
        let routes = OutputRoutes {
            stdout: vec![],
            stderr: vec![Sink {
                destination: Destination::Stderr,
                format: Format::Events,
            }],
        };
        let result = route(&mut child, &routes, &typed(), rules()).await;
        assert!(matches!(result, Err(OutputError::InvalidRoutes(_))));
    }
}
