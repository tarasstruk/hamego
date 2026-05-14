#![no_std]

use core::fmt::Write;
use core::str::FromStr;

// --- Config ---

#[derive(Debug, Clone)]
pub struct Config {
    pub scale: f64,
    pub width: f64,
    pub height: f64,
    pub stroke_width: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            scale: 0.25,
            width: 6540.0,
            height: 4400.0,
            stroke_width: 1.0,
        }
    }
}

// --- Color mapping ---

// Hex color strings for pens 0–4: KHAKI, DARK_RED, GREEN, BLUE, GRAY
pub const COLOR_HEX: [&str; 5] = ["#f0e68c", "#8b0000", "#008000", "#0000ff", "#808080"];

// --- Coordinate helpers ---

/// Iterator over (x, y) pairs parsed from a comma-separated HPGL coordinate string.
pub fn pair_iter(input: &str) -> impl Iterator<Item = (f64, f64)> + '_ {
    let mut iter = input
        .split(',')
        .map(|s| f64::from_str(s.trim()).unwrap_or(0.0));
    core::iter::from_fn(move || {
        let x = iter.next()?;
        let y = iter.next()?;
        Some((x, y))
    })
}

/// Convert raw HPGL plotter coordinates to SVG coordinates.
pub fn read_points<'a>(
    input: &'a str,
    config: &'a Config,
) -> impl Iterator<Item = (f64, f64)> + 'a {
    pair_iter(input).map(|(x, y)| (x * config.scale, (config.height - y) * config.scale))
}

// --- CommandHandler trait ---

/// Streaming event handler for HPGL commands.
/// Implement this trait to react to parsed commands without any heap allocation.
pub trait CommandHandler {
    fn select_pen(&mut self, pen: usize);
    fn pen_up(&mut self, x: f64, y: f64);
    /// Called before the first point of a PD segment.
    fn pen_down_begin(&mut self);
    /// Called for each point in the current PD segment.
    fn pen_down_point(&mut self, x: f64, y: f64);
    /// Called after the last point of a PD segment.
    fn pen_down_end(&mut self);
}

// --- HPGL parser ---

const PEN_UP: &str = "PU";
const PEN_DOWN: &str = "PD";
const SELECT_PEN: &str = "SP";

/// Stream HPGL commands to a `CommandHandler`. Zero heap allocation.
pub fn parse_hpgl(hpgl: &str, config: &Config, handler: &mut impl CommandHandler) {
    let mut current: Option<(f64, f64)> = None;

    for cmd in hpgl.split(';') {
        let cmd = cmd.trim();

        if let Some(body) = cmd.strip_prefix(SELECT_PEN) {
            if let Ok(num) = usize::from_str(body.trim()) {
                handler.select_pen(num);
            }
            continue;
        }

        if let Some(body) = cmd.strip_prefix(PEN_UP) {
            let body = body.trim();
            if body.is_empty() {
                continue;
            }
            if let Some(point) = read_points(body, config).last() {
                current = Some(point);
                handler.pen_up(point.0, point.1);
            }
            continue;
        }

        if let Some(body) = cmd.strip_prefix(PEN_DOWN) {
            handler.pen_down_begin();
            // Include the last PU position as the first point of the polyline
            if let Some((x, y)) = current.take() {
                handler.pen_down_point(x, y);
            }
            for (x, y) in read_points(body.trim(), config) {
                handler.pen_down_point(x, y);
            }
            handler.pen_down_end();
        }
    }
}

// --- SvgWriter: CommandHandler that writes SVG markup ---

/// Writes SVG `<polyline>` elements to any `core::fmt::Write` sink.
pub struct SvgWriter<'a, W: Write> {
    writer: &'a mut W,
    current_color: &'static str,
    stroke_width: f64,
    first_point: bool,
}

impl<'a, W: Write> SvgWriter<'a, W> {
    pub fn new(writer: &'a mut W, stroke_width: f64) -> Self {
        Self {
            writer,
            current_color: COLOR_HEX[0],
            stroke_width,
            first_point: true,
        }
    }
}

impl<W: Write> CommandHandler for SvgWriter<'_, W> {
    fn select_pen(&mut self, pen: usize) {
        if pen < COLOR_HEX.len() {
            self.current_color = COLOR_HEX[pen];
        }
    }

    fn pen_up(&mut self, _x: f64, _y: f64) {}

    fn pen_down_begin(&mut self) {
        let _ = write!(
            self.writer,
            r#"<polyline fill="none" stroke="{}" stroke-width="{}" points=""#,
            self.current_color, self.stroke_width
        );
        self.first_point = true;
    }

    fn pen_down_point(&mut self, x: f64, y: f64) {
        if !self.first_point {
            let _ = self.writer.write_char(' ');
        }
        let _ = write!(self.writer, "{},{}", x, y);
        self.first_point = false;
    }

    fn pen_down_end(&mut self) {
        let _ = self.writer.write_str(r#""/>"#);
    }
}

// --- High-level SVG generator ---

/// Write a complete SVG document to the given `core::fmt::Write` sink.
/// The caller supplies the buffer (e.g. `String`, `heapless::String<N>`).
pub fn generate_svg(hpgl: &str, config: &Config, out: &mut impl Write) {
    let svg_width = config.width * config.scale;
    let svg_height = config.height * config.scale;

    let _ = write!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}">"#,
        svg_width, svg_height, svg_width, svg_height
    );

    let mut svg_writer = SvgWriter::new(out, config.stroke_width);
    parse_hpgl(hpgl, config, &mut svg_writer);

    let _ = svg_writer.writer.write_str("</svg>");
}
