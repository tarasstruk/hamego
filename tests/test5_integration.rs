use std::fs;

use hamego_core::{COLOR_HEX, CommandHandler, Config, parse_hpgl};

/// Counter handler to collect path statistics without alloc
struct Stats {
    path_count: usize,
    color_counts: [usize; 5],
    current_pen: usize,
    first_path_pen: Option<usize>,
    in_path: bool,
}

impl Stats {
    fn new() -> Self {
        Self {
            path_count: 0,
            color_counts: [0; 5],
            current_pen: 0,
            first_path_pen: None,
            in_path: false,
        }
    }
}

impl CommandHandler for Stats {
    fn select_pen(&mut self, pen: usize) {
        self.current_pen = pen;
    }
    fn pen_up(&mut self, _x: f64, _y: f64) {}
    fn pen_down_begin(&mut self) {
        self.in_path = true;
    }
    fn pen_down_point(&mut self, _x: f64, _y: f64) {}
    fn pen_down_end(&mut self) {
        if self.in_path {
            if self.first_path_pen.is_none() {
                self.first_path_pen = Some(self.current_pen);
            }
            if self.current_pen < self.color_counts.len() {
                self.color_counts[self.current_pen] += 1;
            }
            self.path_count += 1;
            self.in_path = false;
        }
    }
}

/// Parse the test5.hpgl sample into a vsvg layer using default config.
fn stats_test5() -> Stats {
    let config = Config::default();
    let content = fs::read_to_string("samples/test5.hpgl").expect("sample file missing");
    let mut stats = Stats::new();
    parse_hpgl(&content, &config, &mut stats);
    stats
}

#[test]
fn test5_produces_expected_number_of_paths() {
    let stats = stats_test5();
    // test5.hpgl contains 184 PD commands, each producing one path
    assert_eq!(stats.path_count, 184);
}

#[test]
fn test5_contains_expected_colors() {
    let stats = stats_test5();
    // pen indices: 0=KHAKI, 1=DARK_RED, 2=GREEN, 3=BLUE, 4=GRAY
    let blue_count = stats.color_counts[3]; // SP3
    let dark_red_count = stats.color_counts[1]; // SP1
    let gray_count = stats.color_counts[4]; // SP4

    assert_eq!(blue_count, 1, "expected 1 blue path (grid frame)");
    assert_eq!(
        dark_red_count, 167,
        "expected 167 dark-red paths (waveform)"
    );
    assert_eq!(gray_count, 16, "expected 16 gray paths (grid lines)");
}

#[test]
fn test5_first_path_is_grid_frame() {
    let stats = stats_test5();
    // The first PD command draws the grid frame with SP3 (BLUE = COLOR_HEX[3])
    assert_eq!(stats.first_path_pen, Some(3));
    assert_eq!(COLOR_HEX[3], "#0000ff");
}
