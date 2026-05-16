use anyhow::{Context, Result};
use core::fmt::Write as _;
use std::io::Read as StdRead;
use std::path::PathBuf;

use clap::Parser;
use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use hamego_core::async_parser::{
    AsyncCommandHandler, DEFAULT_DELIM, DEFAULT_MAX_PTS, ParseError, parse_hpgl_async,
};
use hamego_core::{CommandHandler, Config, SvgWriter};

// ---------------------------------------------------------------------------
// Static I/O buffer — always behind Mutex, same pattern on desktop & embedded
// ---------------------------------------------------------------------------

static IO_BUFFER: Mutex<CriticalSectionRawMutex, [u8; 4096]> = Mutex::new([0u8; 4096]);

// ---------------------------------------------------------------------------
// std::fs::File → embedded-io-async::Read bridge (blocking, OK for desktop)
// ---------------------------------------------------------------------------

struct StdFileReader(std::fs::File);

impl embedded_io_async::ErrorType for StdFileReader {
    type Error = embedded_io_async::ErrorKind;
}

impl embedded_io_async::Read for StdFileReader {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.0
            .read(buf)
            .map_err(|_| embedded_io_async::ErrorKind::Other)
    }
}

// ---------------------------------------------------------------------------
// AsyncCommandHandler adapter wrapping the no_std SvgWriter
// ---------------------------------------------------------------------------

struct AsyncSvgWriter<'a> {
    inner: SvgWriter<'a, String>,
}

impl<'a> AsyncSvgWriter<'a> {
    fn new(buf: &'a mut String, stroke_width: f64) -> Self {
        Self {
            inner: SvgWriter::new(buf, stroke_width),
        }
    }
}

impl AsyncCommandHandler for AsyncSvgWriter<'_> {
    async fn select_pen(&mut self, pen: usize) {
        self.inner.select_pen(pen);
    }
    async fn pen_up(&mut self, x: f64, y: f64) {
        self.inner.pen_up(x, y);
    }
    async fn pen_down_begin(&mut self) {
        self.inner.pen_down_begin();
    }
    async fn pen_down_point(&mut self, x: f64, y: f64) {
        self.inner.pen_down_point(x, y);
    }
    async fn pen_down_end(&mut self) {
        self.inner.pen_down_end();
    }
    async fn complete(&mut self) {}
}

// ---------------------------------------------------------------------------
// CLI args
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    match run().await {
        Ok(()) => std::process::exit(0),
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}

async fn run() -> Result<()> {
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

    let svg_width = config.width * config.scale;
    let svg_height = config.height * config.scale;

    let mut svg = String::new();
    let _ = write!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}">"#,
        svg_width, svg_height, svg_width, svg_height
    );

    let file = std::fs::File::open(&args.input).context("Failed to open input file")?;
    let reader = StdFileReader(file);

    let mut handler = AsyncSvgWriter::new(&mut svg, config.stroke_width);

    let mut io_buf = IO_BUFFER.lock().await;

    parse_hpgl_async::<DEFAULT_MAX_PTS, DEFAULT_DELIM, _, _>(
        reader,
        &config,
        &mut handler,
        &mut *io_buf,
    )
    .await
    .map_err(|e| match e {
        ParseError::TooManyPoints => anyhow::anyhow!("HPGL path exceeds maximum point count"),
        ParseError::TokenTooLong => anyhow::anyhow!("HPGL token too long"),
        ParseError::Io(io) => anyhow::anyhow!("I/O error reading HPGL file: {:?}", io),
    })?;

    svg.push_str("</svg>");

    std::fs::write(&output, svg).context("Failed to write SVG file")
}
