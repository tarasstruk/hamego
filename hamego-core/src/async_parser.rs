use core::str::FromStr;

use embedded_io_async::Read;

use crate::{Config, read_points};

// Default buffer size for a single HPGL command.
// Must be large enough for the longest possible PD command in the input.
// test5.hpgl contains PD commands up to ~10 KB.
pub const DEFAULT_CMD_BUF_SIZE: usize = 16384;

const PEN_UP: &str = "PU";
const PEN_DOWN: &str = "PD";
const SELECT_PEN: &str = "SP";

/// Errors returned by [`parse_hpgl_async`].
#[derive(Debug, PartialEq)]
pub enum ParseError<E> {
    /// A single HPGL command exceeded the command buffer size `BS`.
    CommandTooLong,
    /// The underlying reader returned an I/O error.
    Io(E),
}

/// Async streaming event handler for HPGL commands.
#[allow(async_fn_in_trait)]
pub trait AsyncCommandHandler {
    async fn select_pen(&mut self, pen: usize);
    async fn pen_up(&mut self, x: f64, y: f64);
    /// Called before the first point of a PD segment.
    async fn pen_down_begin(&mut self);
    /// Called for each point in the current PD segment.
    async fn pen_down_point(&mut self, x: f64, y: f64);
    /// Called after the last point of a PD segment.
    async fn pen_down_end(&mut self);
    /// Called when byte `0x0A` (LF) is received — signals end of HPGL transmission.
    async fn complete(&mut self);
}

/// Dispatch a single parsed HPGL command string to the handler.
async fn dispatch<H: AsyncCommandHandler>(
    cmd: &str,
    config: &Config,
    current: &mut Option<(f64, f64)>,
    handler: &mut H,
) {
    let cmd = cmd.trim();
    if cmd.is_empty() {
        return;
    }

    if let Some(body) = cmd.strip_prefix(SELECT_PEN) {
        if let Ok(num) = usize::from_str(body.trim()) {
            handler.select_pen(num).await;
        }
        return;
    }

    if let Some(body) = cmd.strip_prefix(PEN_UP) {
        let body = body.trim();
        if body.is_empty() {
            return;
        }
        if let Some(point) = read_points(body, config).last() {
            *current = Some(point);
            handler.pen_up(point.0, point.1).await;
        }
        return;
    }

    if let Some(body) = cmd.strip_prefix(PEN_DOWN) {
        handler.pen_down_begin().await;
        if let Some((x, y)) = current.take() {
            handler.pen_down_point(x, y).await;
        }
        for (x, y) in read_points(body.trim(), config) {
            handler.pen_down_point(x, y).await;
        }
        handler.pen_down_end().await;
    }
}

// Default delimiter byte — LF signals end of one HPGL transmission.
pub const DEFAULT_DELIM: u8 = 0x0A;

/// Parse HPGL from an async byte reader, streaming events to `handler`.
///
/// - Commands are delimited by `;`.
/// - `DELIM` byte signals end of a transmission: `handler.complete()` is called
///   and parsing continues — more transmissions may follow.
///   Use [`DEFAULT_DELIM`] (`0x0A`, LF) for standard HPGL streams.
/// - EOF without `DELIM` exits silently without calling `complete()`.
/// - `BS` is both the I/O read buffer size and the max command length.
///   Returns [`ParseError::CommandTooLong`] if a command exceeds `BS` bytes.
///   Use [`DEFAULT_CMD_BUF_SIZE`] as a sensible default.
pub async fn parse_hpgl_async<const BS: usize, const DELIM: u8, R, H>(
    mut reader: R,
    config: &Config,
    handler: &mut H,
) -> Result<(), ParseError<R::Error>>
where
    R: Read,
    H: AsyncCommandHandler,
{
    // I/O read buffer — filled in chunks from the reader
    let mut io_buf = [0u8; BS];
    // Accumulates bytes of the current command between `;` delimiters
    let mut cmd_buf = [0u8; BS];
    let mut cmd_len = 0usize;
    let mut current: Option<(f64, f64)> = None;

    loop {
        let n = match reader.read(&mut io_buf).await {
            Ok(0) => break, // EOF
            Ok(n) => n,
            Err(e) => return Err(ParseError::Io(e)),
        };

        for &b in &io_buf[..n] {
            if b == DELIM {
                // Flush pending command, fire complete(), reset state
                if cmd_len > 0 {
                    if let Ok(s) = core::str::from_utf8(&cmd_buf[..cmd_len]) {
                        dispatch(s, config, &mut current, handler).await;
                    }
                    cmd_len = 0;
                }
                handler.complete().await;
                current = None;
                continue;
            }

            if b == b';' {
                // End of one HPGL command — dispatch and reset
                if cmd_len > 0 {
                    if let Ok(s) = core::str::from_utf8(&cmd_buf[..cmd_len]) {
                        dispatch(s, config, &mut current, handler).await;
                    }
                    cmd_len = 0;
                }
                continue;
            }

            // Accumulate into command buffer — error if full
            if cmd_len >= BS {
                return Err(ParseError::CommandTooLong);
            }
            cmd_buf[cmd_len] = b;
            cmd_len += 1;
        }
    }

    // EOF: flush any trailing command without complete()
    if cmd_len > 0
        && let Ok(s) = core::str::from_utf8(&cmd_buf[..cmd_len])
    {
        dispatch(s, config, &mut current, handler).await;
    }

    Ok(())
}
