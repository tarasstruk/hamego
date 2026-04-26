use std::fs;
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

#[derive(Clone)]
pub struct StartingPoint(Option<(f64, f64)>);

impl Iterator for StartingPoint {
    type Item = (f64, f64);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.take()
    }
}

impl StartingPoint {
    fn set(&mut self, value: (f64, f64)) {
        self.0 = Some(value);
    }

    fn release(&mut self) -> Self {
        StartingPoint(self.0.take())
    }
}

fn main() {
    let args = Args::parse();

    let output = args
        .output
        .unwrap_or_else(|| args.input.with_extension("svg"));

    let mut doc =
        vsvg::Document::new_with_page_size(vsvg::PageSize::Custom(WIDTH, HEIGHT, vsvg::Unit::Px));

    let mut layer = vsvg::Layer::default();
    layer.metadata_mut().name = Some("Layer 2".to_string());

    let mut hpgl = fs::read_to_string(&args.input).expect("Should have been able to read the file");

    hpgl.retain(|c| c != '\n');

    let mut start = StartingPoint(None);

    let mut color = COLORS[0];

    for cmd in hpgl.split(';') {
        if cmd.starts_with("SP") {
            let num = usize::from_str(&cmd[2..]).unwrap();
            color = COLORS[num];
        }

        if cmd.starts_with("PU") {
            if cmd.len() == 2 {
                continue;
            }
            let sub = &cmd[2..];

            if let Some(point) = read_points(sub).last() {
                start.set(point)
            }
        }

        if cmd.starts_with("PD") {
            let sub = &cmd[2..];

            let all_points = start.release().chain(read_points(sub));
            let mut poly = vsvg::Path::from_points(all_points);
            poly.metadata_mut().color = color;
            poly.metadata_mut().stroke_width = 1.0;
            layer.paths.push(poly);
        }
    }

    // save to SVG
    doc.layers_mut().insert(2, layer);
    doc.to_svg_file(&output).unwrap();
}
