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
// Mock AsyncRead
// ---------------------------------------------------------------------------

struct SliceReader<'a> {
    data: &'a [u8],
    pos: usize,
    chunk: usize, // max bytes per read() call — lets us test small IO buf
}

impl<'a> SliceReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            chunk: data.len().max(1),
        }
    }
    fn with_chunk(data: &'a [u8], chunk: usize) -> Self {
        Self {
            data,
            pos: 0,
            chunk,
        }
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
    let reader = SliceReader::new(input);
    let mut handler = RecordingHandler::new();
    let mut buf = [0u8; DEFAULT_IO_BUF_SIZE];
    block_on(parse_hpgl_async::<DEFAULT_MAX_PTS, DEFAULT_DELIM, _, _>(
        reader,
        &config,
        &mut handler,
        &mut buf,
    ))
    .expect("parse failed");
    handler.events
}

// ---------------------------------------------------------------------------
// Preserved tests
// ---------------------------------------------------------------------------

#[test]
fn async_parses_single_command() {
    let events = run(b"SP1;\x0A");
    assert_eq!(events, vec![Event::SelectPen(1), Event::Complete]);
}

#[test]
fn async_complete_fires_on_eof() {
    // No explicit DELIM — EOF must still call complete()
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
    // PU without coordinates must not emit pen_up
    let events = run(b"PU;\x0A");
    assert_eq!(events, vec![Event::Complete]);
}

#[test]
fn async_parses_pen_up_pen_down_sequence() {
    // Default config: scale=0.25, height=4400
    // PU100,200 → x=25.0, y=(4400-200)*0.25=1050.0
    // PD300,400 → carry (25,1050) + 300*0.25=75, (4400-400)*0.25=1000
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
    let reader = SliceReader::new(&bytes);
    let mut ac = AsyncCounter {
        path_count: 0,
        pen_counts: [0; 5],
        current_pen: 0,
    };
    let mut buf = [0u8; DEFAULT_IO_BUF_SIZE];
    block_on(parse_hpgl_async::<DEFAULT_MAX_PTS, DEFAULT_DELIM, _, _>(
        reader, &config, &mut ac, &mut buf,
    ))
    .expect("parity parse failed");

    assert_eq!(sc.path_count, ac.path_count, "path count mismatch");
    assert_eq!(sc.pen_counts, ac.pen_counts, "pen color counts mismatch");
}

// ---------------------------------------------------------------------------
// New tests
// ---------------------------------------------------------------------------

#[test]
fn async_token_too_long_returns_err() {
    // A coordinate longer than TOKEN_BUF_SIZE (16) — malformed input
    // "99999999999999999" = 17 chars > 16
    let input = b"PD99999999999999999,100;\x0A";
    let config = Config::default();
    let reader = SliceReader::new(input);
    let mut handler = RecordingHandler::new();
    let mut buf = [0u8; DEFAULT_IO_BUF_SIZE];
    let result = block_on(parse_hpgl_async::<DEFAULT_MAX_PTS, DEFAULT_DELIM, _, _>(
        reader,
        &config,
        &mut handler,
        &mut buf,
    ));
    assert_eq!(result, Err(ParseError::TokenTooLong));
}

#[test]
fn async_too_many_points_returns_err() {
    // PD with MAX_PTS+1 pairs using MAX_PTS=4
    // 5 pairs: "100,200,100,200,100,200,100,200,100,200"
    let input = b"PD100,200,100,200,100,200,100,200,100,200;\x0A";
    let config = Config::default();
    let reader = SliceReader::new(input);
    let mut handler = RecordingHandler::new();
    let mut buf = [0u8; DEFAULT_IO_BUF_SIZE];
    let result = block_on(parse_hpgl_async::<4, DEFAULT_DELIM, _, _>(
        reader,
        &config,
        &mut handler,
        &mut buf,
    ));
    assert_eq!(result, Err(ParseError::TooManyPoints));
}

#[test]
fn async_pd_large_streams_correctly() {
    // Build a PD with 500 coordinate pairs — must stream all 500 points
    let mut input = b"PD".to_vec();
    for i in 0..500u32 {
        if i > 0 {
            input.push(b',');
        }
        // x,y — simple values
        input.extend_from_slice(b"100,200");
        if i < 499 {
            input.push(b',');
        }
    }
    input.push(b';');
    input.push(0x0A);

    let config = Config::default();
    let reader = SliceReader::new(&input);
    let mut handler = RecordingHandler::new();
    let mut buf = [0u8; DEFAULT_IO_BUF_SIZE];
    block_on(parse_hpgl_async::<DEFAULT_MAX_PTS, DEFAULT_DELIM, _, _>(
        reader,
        &config,
        &mut handler,
        &mut buf,
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
    // PD; with no coordinates — begin + end, no points
    let events = run(b"PD;\x0A");
    assert_eq!(
        events,
        vec![Event::PenDownBegin, Event::PenDownEnd, Event::Complete]
    );
}

#[test]
fn async_pd_with_carry_and_empty_body() {
    // PU100,200 sets carry → PD; emits carry as first point
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
    // IN and LA are unknown — must be silently skipped
    let events = run(b"IN;LA1,2;PD100,200;\x0A");
    // Only PD events + Complete expected
    assert!(events.contains(&Event::PenDownBegin));
    assert!(events.contains(&Event::PenDownEnd));
    assert!(events.contains(&Event::Complete));
    assert!(!events.contains(&Event::SelectPen(0)));
    // Exactly: Begin, Point(100*0.25, (4400-200)*0.25) = (25, 1050), End, Complete
    let pd_points: Vec<_> = handler_pd_points(&events);
    assert_eq!(pd_points, vec![(fp(25.0), fp(1050.0))]);
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

#[test]
fn async_delim_inside_pd_coords() {
    // PD100,200,300 + DELIM — first pair emitted, unpaired 300 dropped, PenDownEnd + Complete
    let input = b"PD100,200,300\x0A";
    let events = run(input);
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
    // PU100,200,300,400 — pen_up must be called with (300,400) only (last pair)
    // scale: 300*0.25=75, (4400-400)*0.25=1000
    let events = run(b"PU100,200,300,400;\x0A");
    let pu_events: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, Event::PenUp(_, _)))
        .collect();
    // pen_up is called for each pair — intermediate (100,200) and final (300,400)
    // but carry is overwritten each time; the last pen_up is (300,400)
    assert!(pu_events.last() == Some(&&Event::PenUp(fp(75.0), fp(1000.0))));
}

#[test]
fn async_sp_multidigit() {
    // SP12 — multi-digit pen number
    let events = run(b"SP12;\x0A");
    assert_eq!(events, vec![Event::SelectPen(12), Event::Complete]);
}

#[test]
fn async_small_io_buf() {
    // Full test5.hpgl with tiny IO chunk size of 16 bytes — must produce same
    // path/pen counts as with the default buffer size.
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
    // chunk=16 forces many partial reads
    let reader = SliceReader::with_chunk(&bytes, 16);
    let mut async_c = Counter {
        paths: 0,
        pens: [0; 5],
        cur: 0,
    };
    let mut buf = [0u8; 64];
    block_on(parse_hpgl_async::<DEFAULT_MAX_PTS, DEFAULT_DELIM, _, _>(
        reader,
        &config,
        &mut async_c,
        &mut buf,
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
