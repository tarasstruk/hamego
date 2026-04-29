use wasm_bindgen::prelude::*;
use hamego_core::{Config, generate_svg_string};

/// Returns an SVG string generated from the given HPGL input and config parameters.
#[wasm_bindgen]
pub fn render_hpgl(
    hpgl: &str,
    scale: f64,
    width: f64,
    height: f64,
    stroke_width: f64,
) -> String {
    let config = Config { scale, width, height, stroke_width };
    generate_svg_string(hpgl, &config)
}

/// Injects the generated SVG into a DOM element identified by container_id.
#[wasm_bindgen]
pub fn render_hpgl_to_dom(
    hpgl: &str,
    scale: f64,
    width: f64,
    height: f64,
    stroke_width: f64,
    container_id: &str,
) {
    let svg = render_hpgl(hpgl, scale, width, height, stroke_width);

    let window = web_sys::window().expect("no global window");
    let document = window.document().expect("no document");
    let container = document
        .get_element_by_id(container_id)
        .expect("container element not found");
    container.set_inner_html(&svg);
}

