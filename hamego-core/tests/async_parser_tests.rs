#![cfg(feature = "async")]
// Integration tests for parse_hpgl_async — state-machine streaming parser.

use embedded_io_async::Read;
use futures_lite::future::block_on;
use hamego_core::Config;
use hamego_core::async_parser::{
    AsyncCommandHandler, DEFAULT_DELIM, DEFAULT_IO_BUF_SIZE, DEFAULT_MAX_PTS, ParseError,
    parse_hpgl_async,
};

// ---------------------------------------------------------------------------
// ChunkedReader — only needed for async_small_io_buf test (limits bytes per read)
// ---------------------------------------------------------------------------

struct ChunkedReader<'a> {
    data: &'a [u8],
    pos: usize,
    chunk: usize,
}

impl<'a> ChunkedReader<'a> {
    fn new(data: &'a [u8], chunk: usize) -> Self {
        Self {
            data,
            pos: 0,
            chunk,
        }
    }
}

impl embedded_io_async::ErrorType for ChunkedReader<'_> {
    type Error = core::convert::Infallible;
}

impl Read for ChunkedReader<'_> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if self.pos >= self.data.len() {
            return Ok(0);
        }
        let n = buf.len().min(self.chunk).min(self.data.len() - self.pos);
        buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

// ---------------------------------------------------------------------------
// Recording handler
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
enum Event {
    SelectPen(usize),
    PenUp(i64, i64),
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn run(input: &[u8]) -> Vec<Event> {
    let config = Config::default();
    let mut handler = RecordingHandler::new();
    let mut io_buf = [0u8; DEFAULT_IO_BUF_SIZE];
    block_on(parse_hpgl_async::<DEFAULT_MAX_PTS, DEFAULT_DELIM, _, _>(
        input,
        &config,
        &mut handler,
        &mut io_buf,
    ))
    .expect("parse failed");
    handler.events
}

fn handler_pd_points(events: &[Event]) -> Vec<(i64, i64)> {
    events
        .iter()
        .filter_map(|e| {
            if let Event::PenDownPoint(x, y) = e {
                Some((*x, *y))
            } else {
                None
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn async_parses_single_command() {
    let events = run(b"SP1;\x0A");
    assert_eq!(events, vec![Event::SelectPen(1), Event::Complete]);
}

#[test]
fn async_complete_fires_on_eof() {
    let events = run(b"SP1;");
    assert_eq!(events, vec![Event::SelectPen(1), Event::Complete]);
}

#[test]
fn async_continues_after_hpgl_block_ends() {
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
    let events = run(b"PU;\x0A");
    assert_eq!(events, vec![Event::Complete]);
}

#[test]
fn async_parses_pen_up_pen_down_sequence() {
    let events = run(b"PU100,200;PD300,400;\x0A");
    assert_eq!(
        events,
        vec![
            Event::PenUp(fp(25.0), fp(1050.0)),
            Event::PenDownBegin,
            Event::PenDownPoint(fp(25.0), fp(1050.0)),
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

    struct AsyncCounter {
        path_count: usize,
        pen_counts: [usize; 5],
        current_pen: usize,
    }
    impl AsyncCommandHandler for AsyncCounter {
        async fn select_pen(&mut self, pen: usize) {
            self.current_pen = pen;
        }
        async fn pen_up(&mut self, _: f64, _: f64) {}
        async fn pen_down_begin(&mut self) {}
        async fn pen_down_point(&mut self, _: f64, _: f64) {}
        async fn pen_down_end(&mut self) {
            self.path_count += 1;
            if self.current_pen < 5 {
                self.pen_counts[self.current_pen] += 1;
            }
        }
        async fn complete(&mut self) {}
    }

    let content = fs::read_to_string("../samples/test5.hpgl").expect("sample missing");
    let config = Config::default();

    let mut sc = SyncCounter {
        path_count: 0,
        pen_counts: [0; 5],
        current_pen: 0,
    };
    parse_hpgl(&content, &config, &mut sc);

    let mut bytes = content.as_bytes().to_vec();
    bytes.push(0x0A);
    let mut ac = AsyncCounter {
        path_count: 0,
        pen_counts: [0; 5],
        current_pen: 0,
    };
    let mut io_buf = [0u8; DEFAULT_IO_BUF_SIZE];
    block_on(parse_hpgl_async::<DEFAULT_MAX_PTS, DEFAULT_DELIM, _, _>(
        bytes.as_slice(),
        &config,
        &mut ac,
        &mut io_buf,
    ))
    .expect("parity parse failed");

    assert_eq!(sc.path_count, ac.path_count, "path count mismatch");
    assert_eq!(sc.pen_counts, ac.pen_counts, "pen color counts mismatch");
}

#[test]
fn async_token_too_long_returns_err() {
    let input: &[u8] = b"PD99999999999999999,100;\x0A";
    let config = Config::default();
    let mut handler = RecordingHandler::new();
    let mut io_buf = [0u8; DEFAULT_IO_BUF_SIZE];
    let result = block_on(parse_hpgl_async::<DEFAULT_MAX_PTS, DEFAULT_DELIM, _, _>(
        input,
        &config,
        &mut handler,
        &mut io_buf,
    ));
    assert_eq!(result, Err(ParseError::TokenTooLong));
}

#[test]
fn async_too_many_points_returns_err() {
    let input: &[u8] = b"PD100,200,100,200,100,200,100,200,100,200;\x0A";
    let config = Config::default();
    let mut handler = RecordingHandler::new();
    let mut io_buf = [0u8; DEFAULT_IO_BUF_SIZE];
    let result = block_on(parse_hpgl_async::<4, DEFAULT_DELIM, _, _>(
        input,
        &config,
        &mut handler,
        &mut io_buf,
    ));
    assert_eq!(result, Err(ParseError::TooManyPoints));
}

#[test]
fn async_pd_large_streams_correctly() {
    let mut input = b"PD".to_vec();
    for i in 0..500u32 {
        if i > 0 {
            input.push(b',');
        }
        input.extend_from_slice(b"100,200");
        if i < 499 {
            input.push(b',');
        }
    }
    input.push(b';');
    input.push(0x0A);

    let config = Config::default();
    let mut handler = RecordingHandler::new();
    let mut io_buf = [0u8; DEFAULT_IO_BUF_SIZE];
    block_on(parse_hpgl_async::<DEFAULT_MAX_PTS, DEFAULT_DELIM, _, _>(
        input.as_slice(),
        &config,
        &mut handler,
        &mut io_buf,
    ))
    .expect("parse failed");

    let pd_points: usize = handler
        .events
        .iter()
        .filter(|e| matches!(e, Event::PenDownPoint(_, _)))
        .count();
    assert_eq!(pd_points, 500);
    assert!(handler.events.contains(&Event::PenDownBegin));
    assert!(handler.events.contains(&Event::PenDownEnd));
}

#[test]
fn async_pd_empty_body() {
    let events = run(b"PD;\x0A");
    assert_eq!(
        events,
        vec![Event::PenDownBegin, Event::PenDownEnd, Event::Complete]
    );
}

#[test]
fn async_pd_with_carry_and_empty_body() {
    let events = run(b"PU100,200;PD;\x0A");
    assert_eq!(
        events,
        vec![
            Event::PenUp(fp(25.0), fp(1050.0)),
            Event::PenDownBegin,
            Event::PenDownPoint(fp(25.0), fp(1050.0)),
            Event::PenDownEnd,
            Event::Complete,
        ]
    );
}

#[test]
fn async_unknown_command_skipped() {
    let events = run(b"IN;LA1,2;PD100,200;\x0A");
    assert!(events.contains(&Event::PenDownBegin));
    assert!(events.contains(&Event::PenDownEnd));
    assert!(events.contains(&Event::Complete));
    let pd_points = handler_pd_points(&events);
    assert_eq!(pd_points, vec![(fp(25.0), fp(1050.0))]);
}

#[test]
fn async_delim_inside_pd_coords() {
    let events = run(b"PD100,200,300\x0A");
    assert_eq!(
        events,
        vec![
            Event::PenDownBegin,
            Event::PenDownPoint(fp(25.0), fp(1050.0)),
            Event::PenDownEnd,
            Event::Complete,
        ]
    );
}

#[test]
fn async_pu_multiple_pairs_keeps_last() {
    let events = run(b"PU100,200,300,400;\x0A");
    let pu_events: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::PenUp(_, _)))
        .collect();
    assert!(pu_events.last() == Some(&&Event::PenUp(fp(75.0), fp(1000.0))));
}

#[test]
fn async_sp_multidigit() {
    let events = run(b"SP12;\x0A");
    assert_eq!(events, vec![Event::SelectPen(12), Event::Complete]);
}

#[test]
fn async_small_io_buf() {
    use hamego_core::{CommandHandler, parse_hpgl};
    use std::fs;

    struct Counter {
        paths: usize,
        pens: [usize; 5],
        cur: usize,
    }
    impl CommandHandler for Counter {
        fn select_pen(&mut self, p: usize) {
            self.cur = p;
        }
        fn pen_up(&mut self, _: f64, _: f64) {}
        fn pen_down_begin(&mut self) {}
        fn pen_down_point(&mut self, _: f64, _: f64) {}
        fn pen_down_end(&mut self) {
            self.paths += 1;
            if self.cur < 5 {
                self.pens[self.cur] += 1;
            }
        }
    }
    impl AsyncCommandHandler for Counter {
        async fn select_pen(&mut self, p: usize) {
            self.cur = p;
        }
        async fn pen_up(&mut self, _: f64, _: f64) {}
        async fn pen_down_begin(&mut self) {}
        async fn pen_down_point(&mut self, _: f64, _: f64) {}
        async fn pen_down_end(&mut self) {
            self.paths += 1;
            if self.cur < 5 {
                self.pens[self.cur] += 1;
            }
        }
        async fn complete(&mut self) {}
    }

    let content = fs::read_to_string("../samples/test5.hpgl").expect("sample missing");
    let config = Config::default();

    let mut sync = Counter {
        paths: 0,
        pens: [0; 5],
        cur: 0,
    };
    parse_hpgl(&content, &config, &mut sync);

    let mut bytes = content.as_bytes().to_vec();
    bytes.push(0x0A);
    // ChunkedReader limits to 16 bytes per read — tests small IO buf behavior
    let reader = ChunkedReader::new(&bytes, 16);
    let mut async_c = Counter {
        paths: 0,
        pens: [0; 5],
        cur: 0,
    };
    let mut io_buf = [0u8; 64];
    block_on(parse_hpgl_async::<DEFAULT_MAX_PTS, DEFAULT_DELIM, _, _>(
        reader,
        &config,
        &mut async_c,
        &mut io_buf,
    ))
    .expect("small-buf parse failed");

    assert_eq!(
        sync.paths, async_c.paths,
        "path count mismatch with small IO buf"
    );
    assert_eq!(
        sync.pens, async_c.pens,
        "pen counts mismatch with small IO buf"
    );
}
