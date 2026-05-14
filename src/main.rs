use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

use clap::Parser;
use hamego_core::{Config, generate_svg};

#[derive(Parser)]
#[command(about = "Hameg HM1507 Oscilloscope HPGL to SVG converter")]
struct Args {
    /// Input HPGL file
    input: PathBuf,

    /// Output SVG file [default: <input>.svg]
    #[arg(short, long)]
    output: Option<PathBuf>,

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

fn main() -> Result<()> {
    let args = Args::parse();

    let config = Config {
        scale: args.scale,
        width: args.width,
        height: args.height,
        stroke_width: args.stroke_width,
    };

    let output = args
        .output
        .unwrap_or_else(|| args.input.with_extension("svg"));

    let content = fs::read_to_string(&args.input)?;
    let mut svg = String::new();
    generate_svg(&content, &config, &mut svg);

    fs::write(&output, svg).context("Failed to write SVG file")
}
