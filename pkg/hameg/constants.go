// Package hameg provides an interface to the Hameg HM1507 oscilloscope via
// its RS-232 serial port.
//
// The oscilloscope communicates at 8N2 (no parity, 8 data bits, 2 stop bits)
// with hardware RTS/CTS flow control. The baud rate is auto-detected by the
// scope on first power-up; this driver defaults to 19200 baud. A SPACE + CR
// handshake is performed on Open to trigger baud-rate lock-in.
//
// Protocol reference: Hameg HM1507 User Manual, "RS-232 Remote Control" chapter.
package hameg

import "fmt"

// VoltDiv represents the vertical voltage-per-division setting.
type VoltDiv int

// Voltage-per-division constants ordered by the index used in the RS-232
// protocol byte encoding (bits 0–3 of the channel settings byte).
const (
	VDiv1mV   VoltDiv = iota // 1 mV/div
	VDiv2mV                  // 2 mV/div
	VDiv5mV                  // 5 mV/div
	VDiv10mV                 // 10 mV/div
	VDiv20mV                 // 20 mV/div
	VDiv50mV                 // 50 mV/div
	VDiv100mV                // 100 mV/div
	VDiv200mV                // 200 mV/div
	VDiv500mV                // 500 mV/div
	VDiv1V                   // 1 V/div
	VDiv2V                   // 2 V/div
	VDiv5V                   // 5 V/div
	VDiv10V                  // 10 V/div
	VDiv20V                  // 20 V/div
)

var voltDivValues = []float64{
	0.001, 0.002, 0.005,
	0.010, 0.020, 0.050,
	0.100, 0.200, 0.500,
	1.0, 2.0, 5.0, 10.0, 20.0,
}

var voltDivNames = []string{
	"1mV", "2mV", "5mV",
	"10mV", "20mV", "50mV",
	"100mV", "200mV", "500mV",
	"1V", "2V", "5V", "10V", "20V",
}

// String returns the human-readable label for the VoltDiv (e.g. "100mV").
func (v VoltDiv) String() string {
	if int(v) < len(voltDivNames) {
		return voltDivNames[v]
	}
	return fmt.Sprintf("VoltDiv(%d)", int(v))
}

// Value returns the volts-per-division as a float64.
func (v VoltDiv) Value() float64 {
	if int(v) < len(voltDivValues) {
		return voltDivValues[v]
	}
	return 0
}

// ParseVoltDiv parses a human-readable voltage-per-division string such as
// "100mV" or "2V" and returns the corresponding VoltDiv constant.
func ParseVoltDiv(s string) (VoltDiv, error) {
	for i, name := range voltDivNames {
		if equalFold(name, s) {
			return VoltDiv(i), nil
		}
	}
	return 0, fmt.Errorf("unknown volt/div %q – valid values: %v", s, voltDivNames)
}

// TimeDiv represents the horizontal time-per-division setting.
type TimeDiv int

// Time-per-division constants ordered by the index used in the RS-232
// protocol byte encoding (bits 0–4 of the timebase settings byte).
const (
	TDiv50ns  TimeDiv = iota // 50 ns/div
	TDiv100ns                // 100 ns/div
	TDiv200ns                // 200 ns/div
	TDiv500ns                // 500 ns/div
	TDiv1us                  // 1 µs/div
	TDiv2us                  // 2 µs/div
	TDiv5us                  // 5 µs/div
	TDiv10us                 // 10 µs/div
	TDiv20us                 // 20 µs/div
	TDiv50us                 // 50 µs/div
	TDiv100us                // 100 µs/div
	TDiv200us                // 200 µs/div
	TDiv500us                // 500 µs/div
	TDiv1ms                  // 1 ms/div
	TDiv2ms                  // 2 ms/div
	TDiv5ms                  // 5 ms/div
	TDiv10ms                 // 10 ms/div
	TDiv20ms                 // 20 ms/div
	TDiv50ms                 // 50 ms/div
	TDiv100ms                // 100 ms/div
	TDiv200ms                // 200 ms/div
	TDiv500ms                // 500 ms/div
	TDiv1s                   // 1 s/div
	TDiv2s                   // 2 s/div
	TDiv5s                   // 5 s/div
	TDiv10s                  // 10 s/div
	TDiv20s                  // 20 s/div
	TDiv50s                  // 50 s/div
	TDiv100s                 // 100 s/div
)

var timeDivValues = []float64{
	50e-9, 100e-9, 200e-9, 500e-9,
	1e-6, 2e-6, 5e-6, 10e-6, 20e-6, 50e-6,
	100e-6, 200e-6, 500e-6,
	1e-3, 2e-3, 5e-3, 10e-3, 20e-3, 50e-3,
	100e-3, 200e-3, 500e-3,
	1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0,
}

var timeDivNames = []string{
	"50ns", "100ns", "200ns", "500ns",
	"1us", "2us", "5us", "10us", "20us", "50us",
	"100us", "200us", "500us",
	"1ms", "2ms", "5ms", "10ms", "20ms", "50ms",
	"100ms", "200ms", "500ms",
	"1s", "2s", "5s", "10s", "20s", "50s", "100s",
}

// String returns the human-readable label for the TimeDiv (e.g. "1ms").
func (t TimeDiv) String() string {
	if int(t) < len(timeDivNames) {
		return timeDivNames[t]
	}
	return fmt.Sprintf("TimeDiv(%d)", int(t))
}

// Value returns the seconds-per-division as a float64.
func (t TimeDiv) Value() float64 {
	if int(t) < len(timeDivValues) {
		return timeDivValues[t]
	}
	return 0
}

// ParseTimeDiv parses a human-readable time-per-division string such as
// "1ms" or "500us" and returns the corresponding TimeDiv constant.
func ParseTimeDiv(s string) (TimeDiv, error) {
	for i, name := range timeDivNames {
		if equalFold(name, s) {
			return TimeDiv(i), nil
		}
	}
	return 0, fmt.Errorf("unknown time/div %q – valid values: %v", s, timeDivNames)
}

// Channel identifies an input channel on the oscilloscope.
type Channel int

const (
	Channel1 Channel = 1
	Channel2 Channel = 2
)

// TimeBase identifies a sweep time base.
type TimeBase int

const (
	TimeBaseA TimeBase = iota
	TimeBaseB
)

// Coupling represents the input coupling mode of a channel.
type Coupling int

const (
	CouplingDC  Coupling = iota // DC coupled
	CouplingAC                  // AC coupled
	CouplingGND                 // Input grounded (for zero-reference)
)

// WaveformSamples is the number of digitised samples per acquired waveform.
const WaveformSamples = 1024

// VerticalDivisions is the number of visible vertical screen divisions.
const VerticalDivisions = 8

// HorizontalDivisions is the number of visible horizontal screen divisions.
const HorizontalDivisions = 10

// DefaultBaudRate is the baud rate used when connecting to the oscilloscope.
const DefaultBaudRate = 19200

// equalFold compares two ASCII strings case-insensitively without importing
// the strings package at the top level of constants.go.
func equalFold(a, b string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		ca, cb := a[i], b[i]
		if ca >= 'A' && ca <= 'Z' {
			ca += 'a' - 'A'
		}
		if cb >= 'A' && cb <= 'Z' {
			cb += 'a' - 'A'
		}
		if ca != cb {
			return false
		}
	}
	return true
}
