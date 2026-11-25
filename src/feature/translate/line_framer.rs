/// Accumulates byte chunks into complete lines.
#[derive(Debug, Default)]
pub struct LineFramer {
    buffer: Vec<u8>,
}

impl LineFramer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns each newline-terminated line with the newline removed and trailing ASCII
    /// whitespace trimmed. Whitespace-only lines are dropped.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        self.buffer.extend_from_slice(bytes);
        let mut lines = Vec::new();
        while let Some(end) = self.buffer.iter().position(|b| *b == b'\n') {
            let rest = self.buffer.split_off(end + 1);
            let line = std::mem::replace(&mut self.buffer, rest);
            lines.extend(trimmed(&line));
        }
        lines
    }

    /// Returns the buffered partial line at end of stream, if any.
    pub fn flush(&mut self) -> Option<Vec<u8>> {
        trimmed(&std::mem::take(&mut self.buffer))
    }
}

fn trimmed(line: &[u8]) -> Option<Vec<u8>> {
    let line = line.trim_ascii_end();
    (!line.is_empty()).then(|| line.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_complete_lines_and_trims_trailing_whitespace() {
        let mut framer = LineFramer::new();
        let lines = framer.push(b"one  \r\ntwo\n");
        assert_eq!(lines, vec![b"one".to_vec(), b"two".to_vec()]);
        assert_eq!(framer.flush(), None);
    }

    #[test]
    fn joins_lines_split_across_chunks() {
        let mut framer = LineFramer::new();
        assert!(framer.push(b"{\"a\":").is_empty());
        assert_eq!(framer.push(b"1}\n{\"b"), vec![b"{\"a\":1}".to_vec()]);
        assert_eq!(framer.push(b"\":2}\n"), vec![b"{\"b\":2}".to_vec()]);
    }

    #[test]
    fn drops_whitespace_only_lines() {
        let mut framer = LineFramer::new();
        assert_eq!(framer.push(b"\n  \t\r\nx\n"), vec![b"x".to_vec()]);
    }

    #[test]
    fn flush_reports_trailing_partial_line_once() {
        let mut framer = LineFramer::new();
        assert!(framer.push(b"partial  ").is_empty());
        assert_eq!(framer.flush(), Some(b"partial".to_vec()));
        assert_eq!(framer.flush(), None);
    }

    #[test]
    fn preserves_non_utf8_bytes() {
        let mut framer = LineFramer::new();
        assert_eq!(framer.push(b"\xff\xfe\n"), vec![vec![0xff, 0xfe]]);
    }
}
