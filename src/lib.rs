use vsvg::{Color, PathTrait};

pub use hamego_core::Config;

pub const COLORS: [Color; 5] = [
    Color::KHAKI,
    Color::DARK_RED,
    Color::GREEN,
    Color::BLUE,
    Color::GRAY,
];

use hamego_core::{CommandHandler, parse_hpgl};

/// Adapter that converts streaming HPGL events into vsvg Layer paths.
struct VsvgHandler<'a> {
    layer: &'a mut vsvg::Layer,
    color: &'a mut Color,
    stroke_width: f64,
    current_path: Vec<(f64, f64)>,
}

impl CommandHandler for VsvgHandler<'_> {
    fn select_pen(&mut self, pen: usize) {
        if pen < COLORS.len() {
            *self.color = COLORS[pen];
        }
    }

    fn pen_up(&mut self, _x: f64, _y: f64) {}

    fn pen_down_begin(&mut self) {
        self.current_path.clear();
    }

    fn pen_down_point(&mut self, x: f64, y: f64) {
        self.current_path.push((x, y));
    }

    fn pen_down_end(&mut self) {
        if self.current_path.is_empty() {
            return;
        }
        let mut poly = vsvg::Path::from_points(self.current_path.drain(..));
        poly.metadata_mut().color = *self.color;
        poly.metadata_mut().stroke_width = self.stroke_width;
        self.layer.paths.push(poly);
    }
}

pub fn elaborate(buf: &str, layer: &mut vsvg::Layer, color: &mut Color, config: &Config) {
    let mut handler = VsvgHandler {
        layer,
        color,
        stroke_width: config.stroke_width,
        current_path: Vec::new(),
    };
    parse_hpgl(buf, config, &mut handler);
}
