use core::str::FromStr;

use embedded_io_async::Read;

use crate::Config;

// ---------------------------------------------------------------------------
// Public constants
// ---------------------------------------------------------------------------

/// Default I/O read chunk size for [`parse_hpgl_async`].
pub const DEFAULT_IO_BUF_SIZE: usize = 256;

/// Default maximum coordinate pairs allowed in a single PD path.
/// `test5.hpgl` largest PD has ~640 pairs — 4096 is comfortable headroom.
pub const DEFAULT_MAX_PTS: usize = 4096;

/// Default delimiter byte — LF signals end of one HPGL transmission.
pub const DEFAULT_DELIM: u8 = 0x0A;

/// Internal token buffer capacity — fits any HPGL number (max 4 digits + sign/dot).
const TOKEN_BUF_SIZE: usize = 16;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors returned by [`parse_hpgl_async`].
#[derive(Debug, PartialEq)]
pub enum ParseError<E> {
    /// A single PD path contained more coordinate pairs than `MAX_PTS`.
    TooManyPoints,
    /// A number token exceeded [`TOKEN_BUF_SIZE`] bytes.
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

struct Token {
    buf: [u8; TOKEN_BUF_SIZE],
    len: usize,
}

impl Token {
    fn new() -> Self {
        Self {
            buf: [0; TOKEN_BUF_SIZE],
            len: 0,
        }
    }
    fn push(&mut self, b: u8) -> bool {
        if self.len >= TOKEN_BUF_SIZE {
            return false;
        }
        self.buf[self.len] = b;
        self.len += 1;
        true
    }
    fn parse_f64(&self) -> Option<f64> {
        let s = core::str::from_utf8(&self.buf[..self.len]).ok()?;
        f64::from_str(s.trim()).ok()
    }
    fn parse_usize(&self) -> Option<usize> {
        let s = core::str::from_utf8(&self.buf[..self.len]).ok()?;
        usize::from_str(s.trim()).ok()
    }
    fn reset(&mut self) {
        self.len = 0;
    }
}

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
// Internal parse error (no IO variant — used inside StateMachine)
// ---------------------------------------------------------------------------

enum InnerError {
    TooManyPoints,
    TokenTooLong,
}

impl<E> From<InnerError> for ParseError<E> {
    fn from(e: InnerError) -> Self {
        match e {
            InnerError::TooManyPoints => ParseError::TooManyPoints,
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
    token: Token,
    /// Last PU position (scaled SVG coords) carried into the next PD as its first point.
    pu_carry: Option<(f64, f64)>,
    /// Unscaled X of the current PU coordinate pair; `None` until the first comma is seen.
    pu_pending_x: Option<f64>,
    /// Unscaled X of the current PD coordinate pair; `None` until the first comma is seen.
    pd_pending_x: Option<f64>,
    /// Number of points emitted so far in the active PD path (used to enforce `MAX_PTS`).
    pd_point_count: usize,
    /// `true` if any byte has been received since the last `complete()` call.
    /// Prevents a spurious `complete()` when DELIM was the very last byte.
    has_pending_data: bool,
    config: Config,
}

impl StateMachine {
    fn new(config: &Config) -> Self {
        Self {
            state: State::Command,
            prefix: Prefix::default(),
            token: Token::new(),
            pu_carry: None,
            pu_pending_x: None,
            pd_pending_x: None,
            pd_point_count: 0,
            has_pending_data: false,
            config: config.clone(),
        }
    }

    // -----------------------------------------------------------------------
    // Per-state byte handlers
    // -----------------------------------------------------------------------

    async fn handle_command_byte<H: AsyncCommandHandler>(&mut self, b: u8, handler: &mut H) {
        if b == b';' {
            self.prefix.reset();
            return;
        }
        self.prefix.push(b);
        if self.prefix.is_full() {
            if self.prefix.matches(b"PD") {
                handler.pen_down_begin().await;
                if let Some((cx, cy)) = self.pu_carry.take() {
                    handler.pen_down_point(cx, cy).await;
                    self.pd_point_count = 1;
                } else {
                    self.pd_point_count = 0;
                }
                self.pd_pending_x = None;
                self.token.reset();
                self.state = State::PdCoords;
            } else if self.prefix.matches(b"PU") {
                self.pu_pending_x = None;
                self.pd_pending_x = None;
                self.token.reset();
                self.state = State::PuCoords;
            } else if self.prefix.matches(b"SP") {
                self.token.reset();
                self.state = State::SpBody;
            } else {
                self.state = State::Skip;
            }
            self.prefix.reset();
        }
    }

    async fn handle_pd_byte<const MAX_PTS: usize, H: AsyncCommandHandler>(
        &mut self,
        b: u8,
        handler: &mut H,
    ) -> Result<(), InnerError> {
        if b == b';' {
            if let (Some(rx), Some(ry)) = (self.pd_pending_x.take(), self.token.parse_f64()) {
                let (sx, sy) = scale_xy(rx, ry, &self.config);
                if self.pd_point_count >= MAX_PTS {
                    return Err(InnerError::TooManyPoints);
                }
                handler.pen_down_point(sx, sy).await;
                self.pd_point_count += 1;
            }
            self.pd_pending_x = None;
            self.token.reset();
            self.pd_point_count = 0;
            handler.pen_down_end().await;
            self.state = State::Command;
        } else if b == b',' {
            if self.pd_pending_x.is_none() {
                self.pd_pending_x = self.token.parse_f64();
                self.token.reset();
            } else {
                if let (Some(rx), Some(ry)) = (self.pd_pending_x.take(), self.token.parse_f64()) {
                    let (sx, sy) = scale_xy(rx, ry, &self.config);
                    if self.pd_point_count >= MAX_PTS {
                        return Err(InnerError::TooManyPoints);
                    }
                    handler.pen_down_point(sx, sy).await;
                    self.pd_point_count += 1;
                }
                self.token.reset();
            }
        } else if !self.token.push(b) {
            return Err(InnerError::TokenTooLong);
        }
        Ok(())
    }

    async fn handle_pu_byte<H: AsyncCommandHandler>(
        &mut self,
        b: u8,
        handler: &mut H,
    ) -> Result<(), InnerError> {
        if b == b';' {
            if let (Some(rx), Some(ry)) = (self.pu_pending_x.take(), self.token.parse_f64()) {
                let (sx, sy) = scale_xy(rx, ry, &self.config);
                self.pu_carry = Some((sx, sy));
                handler.pen_up(sx, sy).await;
            }
            self.token.reset();
            self.state = State::Command;
        } else if b == b',' {
            if self.pu_pending_x.is_none() {
                self.pu_pending_x = self.token.parse_f64();
                self.token.reset();
            } else {
                if let (Some(rx), Some(ry)) = (self.pu_pending_x.take(), self.token.parse_f64()) {
                    let (sx, sy) = scale_xy(rx, ry, &self.config);
                    self.pu_carry = Some((sx, sy));
                    handler.pen_up(sx, sy).await;
                }
                self.token.reset();
            }
        } else if !self.token.push(b) {
            return Err(InnerError::TokenTooLong);
        }
        Ok(())
    }

    async fn handle_sp_byte<H: AsyncCommandHandler>(
        &mut self,
        b: u8,
        handler: &mut H,
    ) -> Result<(), InnerError> {
        if b == b';' {
            if let Some(pen) = self.token.parse_usize() {
                handler.select_pen(pen).await;
            }
            self.token.reset();
            self.state = State::Command;
        } else if !self.token.push(b) {
            return Err(InnerError::TokenTooLong);
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // DELIM flush: flush current state, reset, call complete()
    // -----------------------------------------------------------------------

    async fn flush_delim<H: AsyncCommandHandler>(&mut self, handler: &mut H) {
        match self.state {
            State::PdCoords => {
                if let (Some(rx), Some(ry)) = (self.pd_pending_x.take(), self.token.parse_f64()) {
                    let (sx, sy) = scale_xy(rx, ry, &self.config);
                    handler.pen_down_point(sx, sy).await;
                }
                handler.pen_down_end().await;
            }
            State::PuCoords => {
                if let (Some(rx), Some(ry)) = (self.pu_pending_x.take(), self.token.parse_f64()) {
                    let (sx, sy) = scale_xy(rx, ry, &self.config);
                    self.pu_carry = Some((sx, sy));
                    handler.pen_up(sx, sy).await;
                }
            }
            State::SpBody => {
                if let Some(pen) = self.token.parse_usize() {
                    handler.select_pen(pen).await;
                }
            }
            _ => {}
        }
        // reset all state
        self.pu_pending_x = None;
        self.pd_pending_x = None;
        self.pd_point_count = 0;
        self.token.reset();
        self.prefix.reset();
        self.pu_carry = None;
        self.has_pending_data = false;
        self.state = State::Command;
        handler.complete().await;
    }

    // -----------------------------------------------------------------------
    // EOF flush: flush trailing state, call complete() if pending
    // -----------------------------------------------------------------------

    async fn flush_eof<H: AsyncCommandHandler>(&mut self, handler: &mut H) {
        if !self.has_pending_data {
            return;
        }
        match self.state {
            State::PdCoords => {
                handler.pen_down_end().await;
            }
            State::PuCoords => {
                if let (Some(rx), Some(ry)) = (self.pu_pending_x, self.token.parse_f64()) {
                    let (sx, sy) = scale_xy(rx, ry, &self.config);
                    handler.pen_up(sx, sy).await;
                }
            }
            State::SpBody => {
                if let Some(pen) = self.token.parse_usize() {
                    handler.select_pen(pen).await;
                }
            }
            _ => {}
        }
        handler.complete().await;
    }

    // -----------------------------------------------------------------------
    // Main byte dispatch
    // -----------------------------------------------------------------------

    async fn feed_byte<const MAX_PTS: usize, const DELIM: u8, H: AsyncCommandHandler>(
        &mut self,
        b: u8,
        handler: &mut H,
    ) -> Result<(), InnerError> {
        self.has_pending_data = true;

        if b == DELIM {
            self.flush_delim(handler).await;
            return Ok(());
        }

        match self.state {
            State::Command => {
                self.handle_command_byte(b, handler).await;
            }
            State::PdCoords => {
                self.handle_pd_byte::<MAX_PTS, H>(b, handler).await?;
            }
            State::PuCoords => {
                self.handle_pu_byte(b, handler).await?;
            }
            State::SpBody => {
                self.handle_sp_byte(b, handler).await?;
            }
            State::Skip => {
                if b == b';' {
                    self.state = State::Command;
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// parse_hpgl_async — public entry point (IO loop only)
// ---------------------------------------------------------------------------

/// Parse HPGL from an async byte reader, streaming events to `handler`.
///
/// # Generic parameters
/// - `MAX_PTS` — maximum coordinate pairs per PD path.
/// - `DELIM` — byte that signals end of one HPGL transmission.
///
/// # Parameters
/// - `io_buf` — externally-owned I/O read buffer.
pub async fn parse_hpgl_async<const MAX_PTS: usize, const DELIM: u8, R, H>(
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
        for &b in &io_buf[..n] {
            sm.feed_byte::<MAX_PTS, DELIM, H>(b, handler)
                .await
                .map_err(ParseError::from)?;
        }
    }

    sm.flush_eof(handler).await;
    Ok(())
}
