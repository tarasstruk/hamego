# hamego

Hameg HM1507 Oscilloscope interface tools. Converts HPGL plotter output from the oscilloscope to SVG.


## Usage

```
hamego [OPTIONS] <INPUT>

Arguments:
  <INPUT>  Input HPGL file

Options:
  -o, --output <OUTPUT>              Output SVG file [default: <input>.svg]
      --scale <SCALE>                Scale factor for converting plotter units to SVG pixels [default: 0.25]
      --width <WIDTH>                HPGL canvas width in plotter units [default: 6540]
      --height <HEIGHT>              HPGL canvas height in plotter units [default: 4400]
      --stroke-width <STROKE_WIDTH>  Stroke width in SVG pixels [default: 1]
  -h, --help                         Print help
```

## Testing

```sh
cargo nextest run
```

## WebAssembly (Browser)

The `hamego-wasm` crate exposes two functions via `wasm-bindgen`:

- `render_hpgl(hpgl, scale, width, height, stroke_width) -> String` — returns SVG string
- `render_hpgl_to_dom(hpgl, scale, width, height, stroke_width, container_id)` — injects SVG into a DOM element

### Build

```sh
wasm-pack build hamego-wasm --target web
```

The generated files will be in `hamego-wasm/pkg/`. Copy them next to `hamego-wasm/www/index.html` and open it with a local HTTP server:

```sh
cp -r hamego-wasm/pkg hamego-wasm/www/pkg
cd hamego-wasm/www && python3 -m http.server
```

Then open `http://localhost:8000` in your browser, paste HPGL content and click **Render SVG**.

