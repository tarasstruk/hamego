use std::str::FromStr;
use vsvg::{Color, PathTrait};


pub use hamego_core::{Config, extract_points, read_points};

pub const COLORS: [Color; 5] = [
    Color::KHAKI,
    Color::DARK_RED,
    Color::GREEN,
    Color::BLUE,
    Color::GRAY,
];

const PEN_UP: &str = "PU";
const PEN_DOWN: &str = "PD";
const SELECT_PEN: &str = "SP";

pub fn elaborate(
    buf: &str,
    layer: &mut vsvg::Layer,
    current: &mut Option<(f64, f64)>,
    color: &mut Color,
    config: &Config,
) {
    for cmd in buf.split(';') {
        let cmd = cmd.trim();

        if let Some(body) = cmd.strip_prefix(SELECT_PEN) {
            if let Ok(num) = usize::from_str(body.trim()) {
                *color = COLORS[num];
            }
            continue;
        }

        if let Some(body) = cmd.strip_prefix(PEN_UP) {
            let body = body.trim();
            if body.is_empty() {
                continue;
            }
            if let Some(point) = read_points(body, config).last() {
                current.replace(point);
            }
            continue;
        }

        if let Some(body) = cmd.strip_prefix(PEN_DOWN) {
            let all_points = current.take().into_iter().chain(read_points(body.trim(), config));
            let mut poly = vsvg::Path::from_points(all_points);
            poly.metadata_mut().color = *color;
            poly.metadata_mut().stroke_width = config.stroke_width;
            layer.paths.push(poly);
        }
    }
}
