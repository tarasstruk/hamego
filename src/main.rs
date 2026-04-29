use anyhow::{Context, Result};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use vsvg::{DocumentTrait, LayerTrait};

use clap::Parser;
use hamego::{COLORS, Config, elaborate};

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
