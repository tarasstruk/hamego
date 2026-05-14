use core::str::FromStr;

use embedded_io_async::Read;

use crate::{Config, read_points};

// Max length of a single HPGL command (e.g. "PD0,0,100,200,...")
const CMD_BUF_SIZE: usize = 256;

const PEN_UP: &str = "PU";
const PEN_DOWN: &str = "PD";
const SELECT_PEN: &str = "SP";

/// Async streaming event handler for HPGL commands.
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

/// Parse HPGL from an async byte reader, streaming events to `handler`.
///
/// - Commands are delimited by `;`.
/// - `0x0A` (LF) terminates the transmission: `handler.complete()` is called and the
///   function returns immediately — any bytes after LF are ignored.
/// - EOF without LF exits silently without calling `complete()`.
/// - Commands longer than `CMD_BUF_SIZE` bytes are silently truncated.
pub async fn parse_hpgl_async<R, H>(mut reader: R, config: &Config, handler: &mut H)
where
    R: Read,
    H: AsyncCommandHandler,
{
    let mut cmd_buf = [0u8; CMD_BUF_SIZE];
    let mut cmd_len = 0usize;
    let mut current: Option<(f64, f64)> = None;
    let mut byte = [0u8; 1];

    loop {
        match reader.read(&mut byte).await {
            Ok(0) => break, // EOF
            Err(_) => break,
            Ok(_) => {}
        }

        let b = byte[0];

        if b == 0x0A {
            // Flush any remaining command before signalling complete
            if cmd_len > 0 {
                if let Ok(s) = core::str::from_utf8(&cmd_buf[..cmd_len]) {
                    dispatch(s, config, &mut current, handler).await;
                }
                cmd_len = 0;
            }
            handler.complete().await;
            // Reset state and continue — more data may follow
            current = None;
            continue;
        }

        if b == b';' {
            if cmd_len > 0 {
                if let Ok(s) = core::str::from_utf8(&cmd_buf[..cmd_len]) {
                    dispatch(s, config, &mut current, handler).await;
                }
                cmd_len = 0;
            }
            continue;
        }

        // Accumulate byte — silently drop if buffer is full
        if cmd_len < CMD_BUF_SIZE {
            cmd_buf[cmd_len] = b;
            cmd_len += 1;
        }
    }

    // EOF: flush any trailing command (no complete() call)
    if cmd_len > 0 {
        if let Ok(s) = core::str::from_utf8(&cmd_buf[..cmd_len]) {
            dispatch(s, config, &mut current, handler).await;
        }
    }
}
