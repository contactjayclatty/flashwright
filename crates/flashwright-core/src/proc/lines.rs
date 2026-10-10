// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

use std::time::Duration;

use crate::proc::StdStream;

/// One decoded line from a child stream. Separators are `\n` and `\r`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamLine {
    pub stream: StdStream,
    pub text: String,
    pub at: Duration,
}

#[derive(Debug, Default)]
pub(crate) struct LineAssembler {
    lines: Vec<StreamLine>,
    stdout_carry: String,
    stderr_carry: String,
}

impl LineAssembler {
    pub(crate) fn push(&mut self, stream: StdStream, chunk: &[u8], at: Duration) {
        let text = String::from_utf8_lossy(chunk);
        let carry = match stream {
            StdStream::Stdout => &mut self.stdout_carry,
            StdStream::Stderr => &mut self.stderr_carry,
        };
        push_text(carry, &mut self.lines, stream, &text, at);
    }

    pub(crate) fn finish(&mut self, at: Duration) {
        flush_carry(
            &mut self.stdout_carry,
            &mut self.lines,
            StdStream::Stdout,
            at,
        );
        flush_carry(
            &mut self.stderr_carry,
            &mut self.lines,
            StdStream::Stderr,
            at,
        );
    }

    pub(crate) fn into_lines(mut self, at: Duration) -> Vec<StreamLine> {
        self.finish(at);
        self.lines
    }
}

fn push_text(
    carry: &mut String,
    lines: &mut Vec<StreamLine>,
    stream: StdStream,
    text: &str,
    at: Duration,
) {
    let mut start = 0;
    for (index, ch) in text.char_indices() {
        if ch == '\n' || ch == '\r' {
            carry.push_str(&text[start..index]);
            emit(carry, lines, stream, at);
            start = index + ch.len_utf8();
        }
    }
    carry.push_str(&text[start..]);
}

fn flush_carry(carry: &mut String, lines: &mut Vec<StreamLine>, stream: StdStream, at: Duration) {
    emit(carry, lines, stream, at);
}

fn emit(carry: &mut String, lines: &mut Vec<StreamLine>, stream: StdStream, at: Duration) {
    if carry.is_empty() {
        return;
    }
    lines.push(StreamLine {
        stream,
        text: std::mem::take(carry),
        at,
    });
}

pub(crate) const TAIL_LIMIT: usize = 64 * 1024;
pub(crate) const OUTPUT_CAP: usize = 8 * 1024 * 1024;

pub(crate) fn push_capped(buf: &mut Vec<u8>, chunk: &[u8], truncated: &mut bool) {
    if buf.len().saturating_add(chunk.len()) <= OUTPUT_CAP {
        buf.extend_from_slice(chunk);
        return;
    }
    *truncated = true;
    if chunk.len() >= OUTPUT_CAP {
        buf.clear();
        buf.extend_from_slice(&chunk[chunk.len() - OUTPUT_CAP..]);
        return;
    }
    let overflow = buf.len() + chunk.len() - OUTPUT_CAP;
    buf.drain(..overflow);
    buf.extend_from_slice(chunk);
}

pub(crate) fn tail_of(buf: &[u8]) -> Vec<u8> {
    if buf.len() <= TAIL_LIMIT {
        buf.to_vec()
    } else {
        buf[buf.len() - TAIL_LIMIT..].to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preset_truncation_flag_stays_set_when_the_chunk_fits() {
        let mut buf = Vec::new();
        let mut truncated = true;
        push_capped(&mut buf, b"ok", &mut truncated);
        assert!(truncated);
        assert_eq!(buf, b"ok");

        let mut full = vec![1u8; OUTPUT_CAP];
        let mut overflow = false;
        push_capped(&mut full, b"x", &mut overflow);
        assert!(overflow);
        assert_eq!(full.len(), OUTPUT_CAP);
    }

    #[test]
    fn splits_newlines_and_carriage_returns() {
        let mut asm = LineAssembler::default();
        let raw = b"serving: 'a' (~10%)\rserving: 'b' (~100%)\nTotal xfer: 1.00x\r\n";
        asm.push(StdStream::Stdout, raw, Duration::from_millis(5));
        let lines = asm.into_lines(Duration::from_millis(6));
        let texts: Vec<&str> = lines.iter().map(|line| line.text.as_str()).collect();
        assert_eq!(
            texts,
            vec![
                "serving: 'a' (~10%)",
                "serving: 'b' (~100%)",
                "Total xfer: 1.00x"
            ]
        );
    }

    #[test]
    fn lossy_utf8_and_partial_line() {
        let mut asm = LineAssembler::default();
        asm.push(StdStream::Stderr, b"half", Duration::ZERO);
        asm.push(StdStream::Stderr, b" line\n", Duration::from_millis(1));
        let lines = asm.into_lines(Duration::from_millis(2));
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "half line");
        assert_eq!(lines[0].stream, StdStream::Stderr);
    }

    #[test]
    fn tail_keeps_the_last_64_kib() {
        let mut buf = vec![b'a'; TAIL_LIMIT + 10];
        buf.extend(std::iter::repeat_n(b'z', 10));
        let tail = tail_of(&buf);
        assert_eq!(tail.len(), TAIL_LIMIT);
        assert!(tail.ends_with(&[b'z'; 10]));
    }
}
