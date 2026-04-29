use std::str::FromStr;
use vsvg::{Color, PathTrait};

use clap::Parser;
use itertools::Itertools;

#[derive(Parser, Debug)]
pub struct Config {
    /// Scale factor for converting plotter units to SVG pixels
    #[arg(long, default_value_t = 0.25)]
    pub scale: f64,

    /// HPGL canvas width in plotter units
    #[arg(long, default_value_t = 6540.0)]
    pub width: f64,

    /// HPGL canvas height in plotter units
    #[arg(long, default_value_t = 4400.0)]
    pub height: f64,

    /// Stroke width in SVG pixels
    #[arg(long, default_value_t = 1.0)]
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

pub const COLORS: [Color; 5] = [
    Color::KHAKI,
    Color::DARK_RED,
    Color::GREEN,
    Color::BLUE,
    Color::GRAY,
];

pub fn extract_points(input: &str) -> impl '_ + Iterator<Item = (f64, f64)> {
    input.split(',').map(|x| f64::from_str(x).unwrap()).tuples()
}

pub fn read_points<'a>(
    input: &'a str,
    config: &'a Config,
) -> impl 'a + Iterator<Item = (f64, f64)> {
    extract_points(input).map(|(x, y)| (x * config.scale, (config.height - y) * config.scale))
}

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
        if let Some(body) = cmd.strip_prefix(SELECT_PEN) {
            let num = usize::from_str(body).unwrap();
            *color = COLORS[num];
            continue;
        }

        if let Some(body) = cmd.strip_prefix(PEN_UP) {
            if body.is_empty() {
                continue;
            }

            if let Some(point) = read_points(body, config).last() {
                current.replace(point);
            }
            continue;
        }

        if let Some(body) = cmd.strip_prefix(PEN_DOWN) {
            let all_points = current.take().into_iter().chain(read_points(body, config));
            let mut poly = vsvg::Path::from_points(all_points);
            poly.metadata_mut().color = *color;
            poly.metadata_mut().stroke_width = config.stroke_width;
            layer.paths.push(poly);
        }
    }
}
