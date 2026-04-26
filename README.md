# hamego

Hameg HM1507 Oscilloscope interface tools. Converts HPGL plotter output from the oscilloscope to SVG.

## Usage

```sh
cargo run -- <input.hpgl>                  # Output: <input>.svg
cargo run -- input.hpgl -o output.svg      # Custom output path
cargo run -- samples/test.hpgl             # Example with sample file
```

Sample HPGL files are in `samples/`.
