use embedded_io_async::Read;

use crate::Config;

// ---------------------------------------------------------------------------
// Public constants
// ---------------------------------------------------------------------------

/// Default I/O read chunk size for [`parse_hpgl_async`].
pub const DEFAULT_IO_BUF_SIZE: usize = 256;

/// Default delimiter byte — LF signals end of one HPGL transmission.
pub const DEFAULT_DELIM: u8 = 0x0A;

/// Internal carry buffer capacity — fits any HPGL number (max ~15 digits).
const CARRY_BUF_SIZE: usize = 16;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors returned by [`parse_hpgl_async`].
#[derive(Debug, PartialEq)]
pub enum ParseError<E> {
    /// A number token exceeded [`CARRY_BUF_SIZE`] bytes.
    TokenTooLong,
    /// The underlying reader returned an I/O error.
    Io(E),
}

// ---------------------------------------------------------------------------
// AsyncCommandHandler trait
// ---------------------------------------------------------------------------

/// Async streaming event handler for HPGL commands.
#[allow(async_fn_in_trait)]
pub trait AsyncCommandHandler {
    async fn select_pen(&mut self, pen: usize);
    async fn pen_up(&mut self, x: f64, y: f64);
    async fn pen_down_begin(&mut self);
    /// Called for each point in the current PD segment (scaled SVG coordinates).
    async fn pen_down_point(&mut self, x: f64, y: f64);
    async fn pen_down_end(&mut self);
    /// Called when `DELIM` byte is received — signals end of HPGL transmission.
    async fn complete(&mut self);
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Prefix {
    buf: [u8; 2],
    len: usize,
}

impl Prefix {
    fn push(&mut self, b: u8) {
        if self.len < 2 {
            self.buf[self.len] = b;
            self.len += 1;
        }
    }
    fn is_full(&self) -> bool {
        self.len == 2
    }
    fn matches(&self, s: &[u8; 2]) -> bool {
        self.len == 2 && &self.buf == s
    }
    fn reset(&mut self) {
        self.len = 0;
    }
}

// ---------------------------------------------------------------------------
// Cursor — lightweight view over an immutable byte slice
// ---------------------------------------------------------------------------

struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn at_end(&self) -> bool {
        self.pos >= self.buf.len()
    }

    fn peek(&self) -> Option<u8> {
        self.buf.get(self.pos).copied()
    }

    fn advance(&mut self) {
        self.pos += 1;
    }

    /// Try to parse a decimal number (integer or float) from the current
    /// position up to (but not including) the next `,`, `;`, DELIM, or end.
    ///
    /// Returns `None` when:
    /// - the buffer ends before a delimiter is found (incomplete token — carry needed), or
    /// - parsing fails (malformed input — skip silently).
    ///
    /// On success the cursor is left **after** the number (but before the delimiter).
    fn try_parse_number<const DELIM: u8>(&mut self) -> Option<NumberResult> {
        let start = self.pos;
        loop {
            match self.buf.get(self.pos) {
                None => {
                    if self.pos - start > CARRY_BUF_SIZE {
                        return Some(NumberResult::TooLong);
                    }
                    return Some(NumberResult::Incomplete);
                }
                Some(&b) if b == b',' || b == b';' || b == DELIM => {
                    let slice = &self.buf[start..self.pos];
                    if slice.is_empty() {
                        return None;
                    }
                    if slice.len() > CARRY_BUF_SIZE {
                        return Some(NumberResult::TooLong);
                    }
                    let s = core::str::from_utf8(slice).ok()?;
                    let v = s.trim().parse::<f64>().ok()?;
                    return Some(NumberResult::Value(v));
                }
                Some(_) => {
                    self.pos += 1;
                }
            }
        }
    }

    /// Try to parse an `x,y` coordinate pair.
    ///
    /// Input format: `x,y` optionally followed by `,` (next pair) or `;`/DELIM (end).
    ///
    /// Returns:
    /// - `PairResult::Pair(x, y)` — both numbers parsed; cursor is past `y` and past
    ///   the trailing `,` separator (if any), ready for next pair or `;`.
    /// - `PairResult::Incomplete` — chunk ended mid-number; cursor rewound to checkpoint.
    /// - `PairResult::None` — hit `;`/DELIM immediately (empty body); cursor NOT advanced.
    fn try_parse_pair<const DELIM: u8>(&mut self) -> PairResult {
        let checkpoint = self.pos;

        // --- parse x ---
        let x = match self.try_parse_number::<DELIM>() {
            None => return PairResult::None,
            Some(NumberResult::Incomplete) => {
                self.pos = checkpoint;
                return PairResult::Incomplete;
            }
            Some(NumberResult::TooLong) => {
                self.pos = checkpoint;
                return PairResult::TooLong;
            }
            Some(NumberResult::Value(v)) => v,
        };

        // Expect comma separator between x and y.
        if self.peek() != Some(b',') {
            self.pos = checkpoint;
            return PairResult::None;
        }
        self.advance();

        // --- parse y ---
        let y = match self.try_parse_number::<DELIM>() {
            None => {
                self.pos = checkpoint;
                return PairResult::None;
            }
            Some(NumberResult::Incomplete) => {
                // Rewind to start of x so carry includes the full "x,y_partial"
                self.pos = checkpoint;
                return PairResult::Incomplete;
            }
            Some(NumberResult::TooLong) => {
                self.pos = checkpoint;
                return PairResult::TooLong;
            }
            Some(NumberResult::Value(v)) => v,
        };

        // NOTE: inter-pair comma is NOT consumed here.
        // The loop in handle_pd/handle_pu skips it at the top of each iteration.

        PairResult::Pair(x, y)
    }

    /// Try to parse a `usize` (e.g. pen number).
    fn try_parse_usize<const DELIM: u8>(&mut self) -> Option<NumberResult> {
        let checkpoint = self.pos;
        match self.try_parse_number::<DELIM>() {
            Some(NumberResult::Value(v)) => Some(NumberResult::Value(v)),
            Some(NumberResult::Incomplete) => {
                self.pos = checkpoint;
                Some(NumberResult::Incomplete)
            }
            Some(NumberResult::TooLong) => {
                self.pos = checkpoint;
                Some(NumberResult::TooLong)
            }
            None => None,
        }
    }

    fn remaining(&self) -> &'a [u8] {
        &self.buf[self.pos..]
    }
}

enum NumberResult {
    Value(f64),
    Incomplete,
    TooLong,
}

enum PairResult {
    Pair(f64, f64),
    /// Chunk ended mid-number — carry unconsumed bytes to next read.
    Incomplete,
    /// No valid pair at this position (e.g. hit `;` immediately).
    None,
    TooLong,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

enum State {
    Command,
    SpBody,
    PuCoords,
    PdCoords,
    Skip,
}

#[inline]
fn scale_xy(raw_x: f64, raw_y: f64, config: &Config) -> (f64, f64) {
    (raw_x * config.scale, (config.height - raw_y) * config.scale)
}

// ---------------------------------------------------------------------------
// Internal parse error
// ---------------------------------------------------------------------------

enum InnerError {
    TokenTooLong,
}

impl<E> From<InnerError> for ParseError<E> {
    fn from(e: InnerError) -> Self {
        match e {
            InnerError::TokenTooLong => ParseError::TokenTooLong,
        }
    }
}

// ---------------------------------------------------------------------------
// StateMachine
// ---------------------------------------------------------------------------

struct StateMachine {
    state: State,
    prefix: Prefix,
    /// Last PU position (scaled SVG coords) carried into the next PD as its first point.
    pu_carry: Option<(f64, f64)>,
    /// `true` if any byte has been received since the last `complete()` call.
    has_pending_data: bool,
    /// Bytes left over from the previous IO read that could not form a complete token.
    carry_buf: [u8; CARRY_BUF_SIZE],
    carry_len: usize,
    config: Config,
}

impl StateMachine {
    fn new(config: &Config) -> Self {
        Self {
            state: State::Command,
            prefix: Prefix::default(),
            pu_carry: None,
            has_pending_data: false,
            carry_buf: [0u8; CARRY_BUF_SIZE],
            carry_len: 0,
            config: config.clone(),
        }
    }

    // -----------------------------------------------------------------------
    // Carry buffer helpers
    // -----------------------------------------------------------------------

    /// Append bytes to the carry buffer. Returns `Err` if overflow.
    fn carry_push(&mut self, bytes: &[u8]) -> Result<(), InnerError> {
        if self.carry_len + bytes.len() > CARRY_BUF_SIZE {
            return Err(InnerError::TokenTooLong);
        }
        self.carry_buf[self.carry_len..self.carry_len + bytes.len()].copy_from_slice(bytes);
        self.carry_len += bytes.len();
        Ok(())
    }

    fn carry_clear(&mut self) {
        self.carry_len = 0;
    }

    // -----------------------------------------------------------------------
    // Per-state handlers (operate on Cursor)
    // -----------------------------------------------------------------------

    async fn handle_command<const DELIM: u8, H: AsyncCommandHandler>(
        &mut self,
        cur: &mut Cursor<'_>,
        handler: &mut H,
    ) {
        while let Some(b) = cur.peek() {
            if b == b';' {
                cur.advance();
                self.prefix.reset();
                return;
            }
            if b == DELIM {
                return; // handled by caller
            }
            cur.advance();
            self.prefix.push(b);
            if self.prefix.is_full() {
                if self.prefix.matches(b"PD") {
                    handler.pen_down_begin().await;
                    if let Some((cx, cy)) = self.pu_carry.take() {
                        handler.pen_down_point(cx, cy).await;
                    }
                    self.carry_clear();
                    self.state = State::PdCoords;
                } else if self.prefix.matches(b"PU") {
                    self.carry_clear();
                    self.state = State::PuCoords;
                } else if self.prefix.matches(b"SP") {
                    self.carry_clear();
                    self.state = State::SpBody;
                } else {
                    self.state = State::Skip;
                }
                self.prefix.reset();
                return;
            }
        }
    }

    async fn handle_pd<const DELIM: u8, H: AsyncCommandHandler>(
        &mut self,
        cur: &mut Cursor<'_>,
        handler: &mut H,
    ) -> Result<(), InnerError> {
        loop {
            // Skip any inter-pair commas (input may have multiple consecutive commas).
            while cur.peek() == Some(b',') {
                cur.advance();
            }
            match cur.peek() {
                Some(b';') => {
                    cur.advance();
                    handler.pen_down_end().await;
                    self.state = State::Command;
                    return Ok(());
                }
                Some(b) if b == DELIM => return Ok(()),
                None => return Ok(()),
                _ => {}
            }

            match cur.try_parse_pair::<DELIM>() {
                PairResult::Pair(rx, ry) => {
                    let (sx, sy) = scale_xy(rx, ry, &self.config);
                    handler.pen_down_point(sx, sy).await;
                }
                PairResult::Incomplete => {
                    self.carry_push(cur.remaining())?;
                    cur.pos = cur.buf.len();
                    return Ok(());
                }
                PairResult::TooLong => return Err(InnerError::TokenTooLong),
                PairResult::None => {
                    // Orphan number — skip to next delimiter.
                    while let Some(b) = cur.peek() {
                        if b == b';' || b == DELIM {
                            break;
                        }
                        cur.advance();
                    }
                }
            }
        }
    }

    async fn handle_pu<const DELIM: u8, H: AsyncCommandHandler>(
        &mut self,
        cur: &mut Cursor<'_>,
        handler: &mut H,
    ) -> Result<(), InnerError> {
        loop {
            while cur.peek() == Some(b',') {
                cur.advance();
            }
            match cur.peek() {
                Some(b';') => {
                    cur.advance();
                    self.state = State::Command;
                    return Ok(());
                }
                Some(b) if b == DELIM => return Ok(()),
                None => return Ok(()),
                _ => {}
            }

            match cur.try_parse_pair::<DELIM>() {
                PairResult::Pair(rx, ry) => {
                    let (sx, sy) = scale_xy(rx, ry, &self.config);
                    self.pu_carry = Some((sx, sy));
                    handler.pen_up(sx, sy).await;
                }
                PairResult::Incomplete => {
                    self.carry_push(cur.remaining())?;
                    cur.pos = cur.buf.len();
                    return Ok(());
                }
                PairResult::TooLong => return Err(InnerError::TokenTooLong),
                PairResult::None => {
                    return Ok(());
                }
            }
        }
    }

    async fn handle_sp<const DELIM: u8, H: AsyncCommandHandler>(
        &mut self,
        cur: &mut Cursor<'_>,
        handler: &mut H,
    ) -> Result<(), InnerError> {
        let pen: Option<usize> = match cur.try_parse_usize::<DELIM>() {
            Some(NumberResult::Value(v)) => Some(v as usize),
            Some(NumberResult::Incomplete) => {
                self.carry_push(cur.remaining())?;
                cur.pos = cur.buf.len();
                return Ok(());
            }
            Some(NumberResult::TooLong) => return Err(InnerError::TokenTooLong),
            None => None,
        };

        if let Some(pen) = pen {
            handler.select_pen(pen).await;
        }

        if cur.peek() == Some(b';') {
            cur.advance();
        }
        self.state = State::Command;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // DELIM flush
    // -----------------------------------------------------------------------

    async fn flush_delim<H: AsyncCommandHandler>(&mut self, handler: &mut H) {
        match self.state {
            State::PdCoords => {
                handler.pen_down_end().await;
            }
            State::PuCoords | State::SpBody | State::Command | State::Skip => {}
        }
        self.carry_clear();
        self.prefix.reset();
        self.pu_carry = None;
        self.has_pending_data = false;
        self.state = State::Command;
        handler.complete().await;
    }

    // -----------------------------------------------------------------------
    // EOF flush
    // -----------------------------------------------------------------------

    async fn flush_eof<H: AsyncCommandHandler>(&mut self, handler: &mut H) {
        if !self.has_pending_data {
            return;
        }
        if let State::PdCoords = self.state {
            handler.pen_down_end().await;
        }
        handler.complete().await;
    }

    // -----------------------------------------------------------------------
    // Chunk-level dispatch
    // -----------------------------------------------------------------------

    async fn feed_bytes<const DELIM: u8, H: AsyncCommandHandler>(
        &mut self,
        data: &[u8],
        handler: &mut H,
    ) -> Result<(), InnerError> {
        self.has_pending_data = true;

        // Prepend any carry bytes to this chunk so handlers see a contiguous slice.
        let effective_data: &[u8] = if self.carry_len > 0 {
            // Stack-allocate a combined buffer.
            let total = self.carry_len + data.len();
            if total > CARRY_BUF_SIZE + DEFAULT_IO_BUF_SIZE + 64 {
                return Err(InnerError::TokenTooLong);
            }
            // We cannot return a reference to a stack-local without unsafe.
            // Instead, use a heap-free trick: extend carry_buf temporarily.
            // carry_buf is CARRY_BUF_SIZE; data can be up to DEFAULT_IO_BUF_SIZE.
            // Use a fixed-size stack array big enough.
            // We handle this in the loop below via `effective_buf`.
            data // placeholder — actual logic below
        } else {
            data
        };
        let _ = effective_data; // suppress warning

        // Use a stack buffer for carry + data concatenation.
        const EFF_BUF: usize = CARRY_BUF_SIZE + DEFAULT_IO_BUF_SIZE + 256;
        let mut eff_buf = [0u8; EFF_BUF];
        let eff_slice: &[u8] = if self.carry_len > 0 {
            let total = self.carry_len + data.len();
            if total > EFF_BUF {
                return Err(InnerError::TokenTooLong);
            }
            eff_buf[..self.carry_len].copy_from_slice(&self.carry_buf[..self.carry_len]);
            eff_buf[self.carry_len..total].copy_from_slice(data);
            self.carry_len = 0;
            &eff_buf[..total]
        } else {
            data
        };

        let mut cur = Cursor::new(eff_slice);

        while !cur.at_end() {
            let b = cur.peek().unwrap();

            if b == DELIM {
                cur.advance();
                self.flush_delim(handler).await;
                if !cur.at_end() {
                    self.has_pending_data = true;
                }
                continue;
            }

            match self.state {
                State::Command => {
                    self.handle_command::<DELIM, H>(&mut cur, handler).await;
                }
                State::PdCoords => {
                    self.handle_pd::<DELIM, H>(&mut cur, handler).await?;
                }
                State::PuCoords => {
                    self.handle_pu::<DELIM, H>(&mut cur, handler).await?;
                }
                State::SpBody => {
                    self.handle_sp::<DELIM, H>(&mut cur, handler).await?;
                }
                State::Skip => {
                    // Discard until ';' or DELIM.
                    while let Some(b) = cur.peek() {
                        if b == b';' || b == DELIM {
                            break;
                        }
                        cur.advance();
                    }
                    if cur.peek() == Some(b';') {
                        cur.advance();
                        self.state = State::Command;
                    }
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// parse_hpgl_async — public entry point
// ---------------------------------------------------------------------------

/// Parse HPGL from an async byte reader, streaming events to `handler`.
///
/// # Generic parameters
/// - `DELIM` — byte that signals end of one HPGL transmission.
///
/// # Parameters
/// - `io_buf` — externally-owned I/O read buffer.
pub async fn parse_hpgl_async<const DELIM: u8, R, H>(
    mut reader: R,
    config: &Config,
    handler: &mut H,
    io_buf: &mut [u8],
) -> Result<(), ParseError<R::Error>>
where
    R: Read,
    H: AsyncCommandHandler,
{
    let mut sm = StateMachine::new(config);

    loop {
        let n = match reader.read(io_buf).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => return Err(ParseError::Io(e)),
        };
        sm.feed_bytes::<DELIM, H>(&io_buf[..n], handler)
            .await
            .map_err(ParseError::from)?;
    }

    sm.flush_eof(handler).await;
    Ok(())
}
