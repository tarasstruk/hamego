#![cfg(feature = "async")]
// Integration tests for parse_hpgl_async.
//
// Uses a mock AsyncRead backed by &[u8] and a RecordingHandler that
// collects events into a Vec for assertion.

use embedded_io_async::Read;
use futures_lite::future::block_on;
use hamego_core::Config;

use hamego_core::async_parser::{
    AsyncCommandHandler, DEFAULT_CMD_BUF_SIZE, ParseError, parse_hpgl_async,
};

// --- Mock AsyncRead ---

struct SliceReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> SliceReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
}

impl embedded_io_async::ErrorType for SliceReader<'_> {
    type Error = core::convert::Infallible;
}

impl Read for SliceReader<'_> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if self.pos >= self.data.len() {
            return Ok(0);
        }
        let n = buf.len().min(self.data.len() - self.pos);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

// --- Recording handler ---

#[derive(Debug, PartialEq)]
enum Event {
    SelectPen(usize),
    PenUp(i64, i64), // scaled coords × 1000 to avoid f64 PartialEq issues
    PenDownBegin,
    PenDownPoint(i64, i64),
    PenDownEnd,
    Complete,
}

fn fp(v: f64) -> i64 {
    (v * 1000.0).round() as i64
}

struct RecordingHandler {
    events: Vec<Event>,
}

impl RecordingHandler {
    fn new() -> Self {
        Self { events: Vec::new() }
    }
}

impl AsyncCommandHandler for RecordingHandler {
    async fn select_pen(&mut self, pen: usize) {
        self.events.push(Event::SelectPen(pen));
    }
    async fn pen_up(&mut self, x: f64, y: f64) {
        self.events.push(Event::PenUp(fp(x), fp(y)));
    }
    async fn pen_down_begin(&mut self) {
        self.events.push(Event::PenDownBegin);
    }
    async fn pen_down_point(&mut self, x: f64, y: f64) {
        self.events.push(Event::PenDownPoint(fp(x), fp(y)));
    }
    async fn pen_down_end(&mut self) {
        self.events.push(Event::PenDownEnd);
    }
    async fn complete(&mut self) {
        self.events.push(Event::Complete);
    }
}

// --- Helpers ---

fn run(input: &[u8]) -> Vec<Event> {
    let config = Config::default();
    let reader = SliceReader::new(input);
    let mut handler = RecordingHandler::new();
    block_on(parse_hpgl_async::<DEFAULT_CMD_BUF_SIZE, _, _>(
        reader,
        &config,
        &mut handler,
    ))
    .expect("parse failed");
    handler.events
}

// --- Tests ---

#[test]
fn async_parses_single_command() {
    // SP1 followed by LF terminator
    let events = run(b"SP1;\x0A");
    assert_eq!(events, vec![Event::SelectPen(1), Event::Complete]);
}

#[test]
fn async_complete_fires_on_lf() {
    let events = run(b"SP1;\x0A");
    assert!(events.contains(&Event::Complete));
    assert_eq!(events.iter().filter(|e| **e == Event::Complete).count(), 1);
}

#[test]
fn async_no_complete_on_eof() {
    // No 0x0A — EOF only
    let events = run(b"SP1;");
    assert_eq!(events, vec![Event::SelectPen(1)]);
    assert!(!events.contains(&Event::Complete));
}

#[test]
fn async_continues_after_hpgl_block_ends() {
    // 0x0A signals end of one transmission but parsing continues for the next
    let events = run(b"SP1;\x0ASP2;\x0A");
    assert_eq!(
        events,
        vec![
            Event::SelectPen(1),
            Event::Complete,
            Event::SelectPen(2),
            Event::Complete,
        ]
    );
}

#[test]
fn async_empty_pu_is_skipped() {
    // PU without coordinates must not emit pen_up
    let events = run(b"PU;\x0A");
    assert_eq!(events, vec![Event::Complete]);
}

#[test]
fn async_parses_pen_up_pen_down_sequence() {
    // Default config: scale=0.25, height=4400
    // PU100,200 → x=25.0, y=(4400-200)*0.25=1050.0
    // PD300,400 → first point is PU carry: (25, 1050)
    //             then 300*0.25=75, (4400-400)*0.25=1000
    let events = run(b"PU100,200;PD300,400;\x0A");
    assert_eq!(
        events,
        vec![
            Event::PenUp(fp(25.0), fp(1050.0)),
            Event::PenDownBegin,
            Event::PenDownPoint(fp(25.0), fp(1050.0)), // carry from PU
            Event::PenDownPoint(fp(75.0), fp(1000.0)),
            Event::PenDownEnd,
            Event::Complete,
        ]
    );
}

#[test]
fn async_parity_with_sync_parser() {
    use hamego_core::{CommandHandler, parse_hpgl};
    use std::fs;

    // Count-only sync handler
    struct SyncCounter {
        path_count: usize,
        pen_counts: [usize; 5],
        current_pen: usize,
    }
    impl CommandHandler for SyncCounter {
        fn select_pen(&mut self, pen: usize) {
            self.current_pen = pen;
        }
        fn pen_up(&mut self, _: f64, _: f64) {}
        fn pen_down_begin(&mut self) {}
        fn pen_down_point(&mut self, _: f64, _: f64) {}
        fn pen_down_end(&mut self) {
            self.path_count += 1;
            if self.current_pen < 5 {
                self.pen_counts[self.current_pen] += 1;
            }
        }
    }

    // Count-only async handler
    struct AsyncCounter {
        path_count: usize,
        pen_counts: [usize; 5],
        current_pen: usize,
        in_path: bool,
    }
    impl AsyncCommandHandler for AsyncCounter {
        async fn select_pen(&mut self, pen: usize) {
            self.current_pen = pen;
        }
        async fn pen_up(&mut self, _: f64, _: f64) {}
        async fn pen_down_begin(&mut self) {
            self.in_path = true;
        }
        async fn pen_down_point(&mut self, _: f64, _: f64) {}
        async fn pen_down_end(&mut self) {
            self.path_count += 1;
            if self.current_pen < 5 {
                self.pen_counts[self.current_pen] += 1;
            }
            self.in_path = false;
        }
        async fn complete(&mut self) {}
    }

    let content = fs::read_to_string("../samples/test5.hpgl").expect("sample missing");
    let config = Config::default();

    // Sync
    let mut sc = SyncCounter {
        path_count: 0,
        pen_counts: [0; 5],
        current_pen: 0,
    };
    parse_hpgl(&content, &config, &mut sc);

    // Async — append LF to simulate terminator
    let mut bytes = content.as_bytes().to_vec();
    bytes.push(0x0A);
    let reader = SliceReader::new(&bytes);
    let mut ac = AsyncCounter {
        path_count: 0,
        pen_counts: [0; 5],
        current_pen: 0,
        in_path: false,
    };
    block_on(parse_hpgl_async::<DEFAULT_CMD_BUF_SIZE, _, _>(
        reader, &config, &mut ac,
    ))
    .expect("parity parse failed");

    assert_eq!(sc.path_count, ac.path_count, "path count mismatch");
    assert_eq!(sc.pen_counts, ac.pen_counts, "pen color counts mismatch");
}

#[test]
fn async_command_too_long_returns_err() {
    // Command body of 10 bytes with buffer size BS=4 → must return CommandTooLong
    let input = b"SP11111;\x0A"; // "SP11111" = 7 bytes > BS=4
    let config = Config::default();
    let reader = SliceReader::new(input);
    let mut handler = RecordingHandler::new();
    let result = block_on(parse_hpgl_async::<4, _, _>(reader, &config, &mut handler));
    assert_eq!(result, Err(ParseError::CommandTooLong));
}

#[test]
fn async_buffer_overflow_handled() {
    // Build a command longer than BS=4 bytes — parser must return Err, not panic
    let mut input: Vec<u8> = b"SP".to_vec();
    input.extend(std::iter::repeat_n(b'1', 300));
    input.push(b';');
    input.push(0x0A);

    let config = Config::default();
    let reader = SliceReader::new(&input);
    let mut handler = RecordingHandler::new();
    let result = block_on(parse_hpgl_async::<4, _, _>(reader, &config, &mut handler));
    assert_eq!(result, Err(ParseError::CommandTooLong));
}
