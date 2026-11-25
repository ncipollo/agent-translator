//! Golden tests: each bundled agent translates a recorded native stream into exactly the
//! expected events. Set `UPDATE_GOLDEN=1` to regenerate the expected files, then review the diff.

use std::fs;
use std::path::PathBuf;

use agent_translator::feature::defaults;
use agent_translator::feature::event::AgentEvent;
use agent_translator::feature::translate::{EventMapper, LineFramer};

fn fixture_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/agent_streams")
        .join(name)
}

fn translate(name: &str, stream: &[u8]) -> Vec<serde_json::Value> {
    let mapper = EventMapper::new(defaults::load(name).unwrap().rules);
    let mut framer = LineFramer::new();
    let mut lines = framer.push(stream);
    lines.extend(framer.flush());
    lines
        .iter()
        .flat_map(|line| mapper.map_line(line))
        .map(|event: AgentEvent| serde_json::to_value(event).unwrap())
        .collect()
}

#[test]
fn every_default_matches_its_golden_stream() {
    for name in defaults::names() {
        let dir = fixture_dir(name);
        let stream = fs::read(dir.join("run-01.ndjson")).unwrap();
        let actual = translate(name, &stream);
        let expected_path = dir.join("run-01.events.ndjson");
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            let body: String = actual.iter().map(|e| format!("{e}\n")).collect();
            fs::write(&expected_path, body).unwrap();
            continue;
        }
        let expected: Vec<serde_json::Value> = fs::read_to_string(&expected_path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(actual, expected, "{name}");
    }
}
