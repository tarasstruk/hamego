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

