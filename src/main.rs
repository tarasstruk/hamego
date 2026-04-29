use anyhow::{Context, Result};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::str::FromStr;
use vsvg::{Color, DocumentTrait, LayerTrait, PathTrait};

use clap::Parser;
use itertools::Itertools;

#[derive(Parser)]
#[command(about = "Hameg HM1507 Oscilloscope HPGL to SVG converter")]
struct Args {
    /// Input HPGL file
    input: PathBuf,

    /// Output SVG file [default: <input>.svg]
    #[arg(short, long)]
    output: Option<PathBuf>,

    #[command(flatten)]
    config: Config,
}

#[derive(Parser)]
struct Config {
    /// Scale factor for converting plotter units to SVG pixels
    #[arg(long, default_value_t = 0.25)]
    scale: f64,

    /// HPGL canvas width in plotter units
    #[arg(long, default_value_t = 6540.0)]
    width: f64,

    /// HPGL canvas height in plotter units
    #[arg(long, default_value_t = 4400.0)]
    height: f64,

    /// Stroke width in SVG pixels
    #[arg(long, default_value_t = 1.0)]
    stroke_width: f64,
}

const COLORS: [Color; 5] = [
    Color::KHAKI,
    Color::DARK_RED,
    Color::GREEN,
    Color::BLUE,
    Color::GRAY,
];

fn extract_points<'a>(input: &'a str) -> impl 'a + Iterator<Item = (f64, f64)> {
    input.split(',').map(|x| f64::from_str(x).unwrap()).tuples()
}

fn read_points<'a>(input: &'a str, config: &'a Config) -> impl 'a + Iterator<Item = (f64, f64)> {
    extract_points(input).map(|(x, y)| (x * config.scale, (config.height - y) * config.scale))
}

const PEN_UP: &str = "PU";
const PEN_DOWN: &str = "PD";
const SELECT_PEN: &str = "SP";

fn elaborate(
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
            // Handle the case when the PU; command comes without args
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

fn main() -> Result<()> {
    let args = Args::parse();
    let config = &args.config;

    let output = args
        .output
        .unwrap_or_else(|| args.input.with_extension("svg"));

    let svg_width = config.width * config.scale;
    let svg_height = config.height * config.scale;

    let mut doc = vsvg::Document::new_with_page_size(vsvg::PageSize::Custom(
        svg_width,
        svg_height,
        vsvg::Unit::Px,
    ));

    let mut layer = vsvg::Layer::default();
    layer.metadata_mut().name = Some("Layer 2".to_string());

    let reader = BufReader::with_capacity(4096 * 16, File::open(&args.input)?);

    let mut chunks = reader.split(b';');

    let mut current_point: Option<(f64, f64)> = None;
    let mut color = COLORS[0];

    while let Some(Ok(chunk)) = chunks.next() {
        if chunk.starts_with(b"\r") {
            break;
        }
        let buf = String::from_utf8(chunk)?;
        elaborate(&buf, &mut layer, &mut current_point, &mut color, config);
    }

    doc.layers_mut().insert(2, layer);
    doc.to_svg_file(&output).context("Failed to write SVG file")
}
