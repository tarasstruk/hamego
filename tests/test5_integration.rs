use std::fs::File;
use std::io::{BufRead, BufReader};

use hamego::{COLORS, Config, elaborate};
use vsvg::{Color, PathTrait};

/// Parse the test5.hpgl sample into a vsvg layer using default config.
fn parse_test5() -> vsvg::Layer {
    let config = Config::default();
    let mut layer = vsvg::Layer::default();
    let mut current_point: Option<(f64, f64)> = None;
    let mut color = COLORS[0];

    let reader = BufReader::new(File::open("samples/test5.hpgl").expect("sample file missing"));
    let chunks = reader.split(b';');

    for chunk in chunks {
        let chunk = chunk.expect("failed to read chunk");
        if chunk.starts_with(b"\r") {
            break;
        }
        let buf = String::from_utf8(chunk).expect("invalid UTF-8");
        elaborate(&buf, &mut layer, &mut current_point, &mut color, &config);
    }

    layer
}

#[test]
fn test5_produces_expected_number_of_paths() {
    let layer = parse_test5();
    // test5.hpgl contains 184 PD commands, each producing one path
    assert_eq!(layer.paths.len(), 184);
}

#[test]
fn test5_contains_expected_colors() {
    let layer = parse_test5();

    let mut blue_count = 0;
    let mut dark_red_count = 0;
    let mut gray_count = 0;

    for path in &layer.paths {
        match path.metadata().color {
            Color::BLUE => blue_count += 1,
            Color::DARK_RED => dark_red_count += 1,
            Color::GRAY => gray_count += 1,
            other => panic!("unexpected color: {:?}", other),
        }
    }

    assert_eq!(blue_count, 1, "expected 1 blue path (grid frame)");
    assert_eq!(
        dark_red_count, 167,
        "expected 167 dark-red paths (waveform)"
    );
    assert_eq!(gray_count, 16, "expected 16 gray paths (grid lines)");
}

#[test]
fn test5_first_path_is_grid_frame() {
    let layer = parse_test5();
    let first = &layer.paths[0];

    // The first PD command draws the grid frame with SP3 (BLUE)
    assert_eq!(first.metadata().color, Color::BLUE);
}
