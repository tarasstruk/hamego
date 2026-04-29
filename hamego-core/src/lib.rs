use std::str::FromStr;
use itertools::Itertools;

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

// Hex color strings for pens 0–4 matching the original COLORS array:
// KHAKI, DARK_RED, GREEN, BLUE, GRAY
pub const COLOR_HEX: [&str; 5] = ["#f0e68c", "#8b0000", "#008000", "#0000ff", "#808080"];

// --- Draw commands ---

pub enum DrawCommand {
    SelectPen(usize),
    PenUp(f64, f64),
    PenDown(Vec<(f64, f64)>),
}

// --- Coordinate helpers ---

pub fn extract_points(input: &str) -> impl '_ + Iterator<Item = (f64, f64)> {
    input.split(',').map(|x| f64::from_str(x.trim()).unwrap()).tuples()
}

pub fn read_points<'a>(input: &'a str, config: &'a Config) -> impl 'a + Iterator<Item = (f64, f64)> {
    extract_points(input).map(|(x, y)| (x * config.scale, (config.height - y) * config.scale))
}

// --- Command parser ---

const PEN_UP: &str = "PU";
const PEN_DOWN: &str = "PD";
const SELECT_PEN: &str = "SP";

/// Parse a full HPGL string into a list of DrawCommands.
pub fn parse_commands(hpgl: &str, config: &Config) -> Vec<DrawCommand> {
    let mut commands = Vec::new();
    let mut current: Option<(f64, f64)> = None;

    for cmd in hpgl.split(';') {
        let cmd = cmd.trim();

        if let Some(body) = cmd.strip_prefix(SELECT_PEN) {
            if let Ok(num) = usize::from_str(body.trim()) {
                commands.push(DrawCommand::SelectPen(num));
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
                commands.push(DrawCommand::PenUp(point.0, point.1));
            }
            continue;
        }

        if let Some(body) = cmd.strip_prefix(PEN_DOWN) {
            let pts: Vec<(f64, f64)> = current.take().into_iter()
                .chain(read_points(body.trim(), config))
                .collect();
            if !pts.is_empty() {
                commands.push(DrawCommand::PenDown(pts));
            }
        }
    }

    commands
}

// --- SVG string generator ---

/// Generate a complete SVG string from HPGL input and config.
pub fn generate_svg_string(hpgl: &str, config: &Config) -> String {
    let svg_width = config.width * config.scale;
    let svg_height = config.height * config.scale;

    let commands = parse_commands(hpgl, config);

    let mut out = String::new();
    out.push_str(&format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}">"#,
        svg_width, svg_height, svg_width, svg_height
    ));

    let mut current_color = COLOR_HEX[0];
    let stroke_width = config.stroke_width;

    for cmd in &commands {
        match cmd {
            DrawCommand::SelectPen(n) => {
                current_color = COLOR_HEX[*n];
            }
            DrawCommand::PenDown(pts) => {
                let points_str: String = pts
                    .iter()
                    .map(|(x, y)| format!("{},{}", x, y))
                    .collect::<Vec<_>>()
                    .join(" ");
                out.push_str(&format!(
                    r#"<polyline points="{}" fill="none" stroke="{}" stroke-width="{}"/>"#,
                    points_str, current_color, stroke_width
                ));
            }
            DrawCommand::PenUp(_, _) => {}
        }
    }

    out.push_str("</svg>");
    out
}

