use anyhow::{Context, Result, anyhow};
use std::fs;
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
}

const SCALE: f64 = 0.25;
const WIDTH: f64 = 6540. * SCALE;
const HEIGHT: f64 = 4400. * SCALE;

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

fn read_points<'a>(input: &'a str) -> impl 'a + Iterator<Item = (f64, f64)> {
    extract_points(input).map(|(x, y)| (x * SCALE, (4400. - y) * SCALE))
}

fn elaborate(
    buf: &str,
    layer: &mut vsvg::Layer,
    current: &mut Option<(f64, f64)>,
    color: &mut Color,
) {
    for cmd in buf.split(';') {
        if cmd.starts_with("SP") {
            let num = usize::from_str(&cmd[2..]).unwrap();
            *color = COLORS[num];
        }

        if cmd.starts_with("PU") {
            if cmd.len() == 2 {
                continue;
            }
            let sub = &cmd[2..];

            if let Some(point) = read_points(sub).last() {
                current.replace(point);
            }
        }

        if cmd.starts_with("PD") {
            let sub = &cmd[2..];

            let all_points = current.take().into_iter().chain(read_points(sub));
            let mut poly = vsvg::Path::from_points(all_points);
            poly.metadata_mut().color = *color;
            poly.metadata_mut().stroke_width = 1.0;
            layer.paths.push(poly);
        }
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    let output = args
        .output
        .unwrap_or_else(|| args.input.with_extension("svg"));

    let mut doc =
        vsvg::Document::new_with_page_size(vsvg::PageSize::Custom(WIDTH, HEIGHT, vsvg::Unit::Px));

    let mut layer = vsvg::Layer::default();
    layer.metadata_mut().name = Some("Layer 2".to_string());

    let mut reader = BufReader::with_capacity(4096 * 16, File::open(&args.input)?);

    let mut chunks = reader.split(b';');

    let mut current_point: Option<(f64, f64)> = None;
    let mut color = COLORS[0];

    while let Some(Ok(chunk)) = chunks.next() {
        if chunk.starts_with(b"\r") {
            break;
        }
        let buf = String::from_utf8(chunk)?;
        elaborate(&buf, &mut layer, &mut current_point, &mut color);
    }

    doc.layers_mut().insert(2, layer);
    doc.to_svg_file(&output).context("Failed to write SVG file")
}
