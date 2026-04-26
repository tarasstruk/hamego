# hamego

Go interface tools for the **Hameg HM1507** oscilloscope.

`hamego` communicates with the oscilloscope over its RS-232 serial port and
provides both a reusable Go library (`pkg/hameg`) and a CLI tool (`cmd/hamego`).

---

## Hardware requirements

| Item | Detail |
|------|--------|
| Oscilloscope | Hameg HM1507, HM1507-2, or HM1507-3 |
| Cable | 9-pin DB9 straight (1:1) cable |
| Serial adapter | RS-232 port or USB-to-RS-232 adapter |
| Flow control | RTS/CTS hardware handshaking required |

**Serial settings**: 19200 baud · 8 data bits · no parity · 2 stop bits · RTS/CTS

---

## Installation

```sh
go install github.com/tarasstruk/hamego/cmd/hamego@latest
```

Or build from source:

```sh
git clone https://github.com/tarasstruk/hamego.git
cd hamego
go build -o hamego ./cmd/hamego
```

---

## CLI usage

### Autoset

Trigger the oscilloscope's automatic setup (equivalent to pressing AUTOSET):

```sh
hamego autoset -p /dev/ttyUSB0
```

### Capture waveform

Acquire 1024 samples from channel 1 and write them as CSV:

```sh
hamego capture -p /dev/ttyUSB0 -c 1 --vdiv 1V --tdiv 1ms -o waveform.csv
```

The `--vdiv` and `--tdiv` flags tell the tool what the scope is currently set to
so that raw ADC counts are converted to physical volts and seconds. Omitting
them leaves the values at zero (raw counts are still exported).

Output to stdout (pipe-friendly):

```sh
hamego capture -p /dev/ttyUSB0 -c 2 --vdiv 500mV --tdiv 100us
```

### Configure a channel

```sh
# 500 mV/div, AC coupled
hamego set-channel -p /dev/ttyUSB0 -c 1 --vdiv 500mV --coupling AC

# 2 V/div, DC coupled, inverted
hamego set-channel -p /dev/ttyUSB0 -c 2 --vdiv 2V --coupling DC --inverted
```

**Valid `--vdiv` values**: `1mV` `2mV` `5mV` `10mV` `20mV` `50mV` `100mV`
`200mV` `500mV` `1V` `2V` `5V` `10V` `20V`

### Configure a time base

```sh
# Time base A at 1 ms/div
hamego set-timebase -p /dev/ttyUSB0 --base A --tdiv 1ms

# Time base A at 500 µs/div, single-shot mode
hamego set-timebase -p /dev/ttyUSB0 --base A --tdiv 500us --single
```

**Valid `--tdiv` values**: `50ns` `100ns` `200ns` `500ns` `1us` `2us` `5us`
`10us` `20us` `50us` `100us` `200us` `500us` `1ms` `2ms` `5ms` `10ms` `20ms`
`50ms` `100ms` `200ms` `500ms` `1s` `2s` `5s` `10s` `20s` `50s` `100s`

### Global flags

| Flag | Default | Description |
|------|---------|-------------|
| `-p`, `--port` | `/dev/ttyUSB0` | Serial port (e.g. `COM3` on Windows) |
| `--baud` | `19200` | Baud rate |

---

## Library usage

```go
import "github.com/tarasstruk/hamego/pkg/hameg"

osc, err := hameg.Open("/dev/ttyUSB0", hameg.DefaultBaudRate)
if err != nil {
    log.Fatal(err)
}
defer osc.Close()

// Configure channel 1: 1 V/div, AC coupled
err = osc.SetChannel(hameg.Channel1, hameg.ChannelConfig{
    VoltDiv:  hameg.VDiv1V,
    Coupling: hameg.CouplingAC,
})

// Configure time base A: 1 ms/div
err = osc.SetTimeBase(hameg.TimeBaseA, hameg.TimeBaseConfig{
    TimeDiv: hameg.TDiv1ms,
})

// Capture waveform from channel 1
wf, err := osc.ReadWaveform(hameg.Channel1)
wf.VPerDiv   = hameg.VDiv1V.Value()   // 1.0 V
wf.SecPerDiv = hameg.TDiv1ms.Value()  // 1e-3 s

// Write CSV
wf.WriteCSV(os.Stdout)
```

---

## RS-232 protocol notes

The HM1507 uses a proprietary ASCII/binary mixed protocol:

- **Init**: send `SPACE + CR` to trigger baud-rate auto-detection.
- **Config commands**: `CH1=<byte>\r`, `CH2=<byte>\r`, `TBA=<byte>\r`, `TBB=<byte>\r`
  where `<byte>` is a single binary byte encoding the settings (see source for
  bit-field layout).
- **Autoset**: `AUTOSET\r`
- **Waveform query**: `WF1?\r` / `WF2?\r` → 1024 bytes of raw 8-bit ADC data.
- **Disconnect**: `rm0\r` releases remote control back to the front panel.

Full protocol documentation is in the *Hameg HM1507 User Manual*, "RS-232
Remote Control" chapter.

---

## License

MIT – see [LICENSE](LICENSE).