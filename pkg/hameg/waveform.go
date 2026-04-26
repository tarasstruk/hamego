package hameg

import (
	"encoding/csv"
	"fmt"
	"io"
	"strconv"
)

// Waveform holds a captured waveform together with the scaling parameters
// needed to convert raw ADC values to physical units.
type Waveform struct {
	// Samples contains the raw 8-bit ADC values (0–255), one per time step.
	// A full acquisition consists of WaveformSamples (1024) points.
	Samples []byte

	// VPerDiv is the channel's vertical sensitivity in volts per division at
	// the time of capture. Set this from the active ChannelConfig.VoltDiv.
	VPerDiv float64

	// SecPerDiv is the timebase's horizontal scale in seconds per division at
	// the time of capture. Set this from the active TimeBaseConfig.TimeDiv.
	SecPerDiv float64

	// Channel is the source channel (Channel1 or Channel2).
	Channel Channel
}

// ReadWaveform acquires a raw waveform from the given channel.
//
// The oscilloscope is queried with "WF<n>?" and responds with WaveformSamples
// (1024) bytes of 8-bit ADC data. The caller should populate Waveform.VPerDiv
// and Waveform.SecPerDiv with values matching the current scope settings before
// calling Voltage or Time.
func (o *Oscilloscope) ReadWaveform(ch Channel) (*Waveform, error) {
	if ch != Channel1 && ch != Channel2 {
		return nil, fmt.Errorf("invalid channel %d: must be 1 or 2", int(ch))
	}
	cmd := fmt.Sprintf("WF%d", int(ch))
	data, err := o.query(cmd, WaveformSamples)
	if err != nil {
		return nil, fmt.Errorf("reading waveform for channel %d: %w", int(ch), err)
	}
	return &Waveform{
		Samples: data,
		Channel: ch,
	}, nil
}

// Voltage converts the raw ADC sample at index i to a voltage in volts.
//
// The conversion assumes that the ADC midpoint (128) corresponds to 0 V and
// that the full 8-division vertical screen spans 256 ADC counts.
//
//	voltage = (sample[i] − 128) × (VPerDiv × VerticalDivisions / 256)
func (w *Waveform) Voltage(i int) float64 {
	if i < 0 || i >= len(w.Samples) {
		return 0
	}
	return (float64(w.Samples[i]) - 128.0) *
		(w.VPerDiv * float64(VerticalDivisions) / 256.0)
}

// Time returns the time in seconds corresponding to sample index i.
//
//	t(i) = i × (SecPerDiv × HorizontalDivisions / WaveformSamples)
func (w *Waveform) Time(i int) float64 {
	return float64(i) *
		(w.SecPerDiv * float64(HorizontalDivisions) / float64(WaveformSamples))
}

// WriteCSV writes the waveform as a two-column CSV (time [s], voltage [V]) to
// out. The first row is a header; each subsequent row is one sample point.
func (w *Waveform) WriteCSV(out io.Writer) error {
	cw := csv.NewWriter(out)
	if err := cw.Write([]string{"time_s", "voltage_V"}); err != nil {
		return err
	}
	for i := range w.Samples {
		row := []string{
			strconv.FormatFloat(w.Time(i), 'g', -1, 64),
			strconv.FormatFloat(w.Voltage(i), 'g', -1, 64),
		}
		if err := cw.Write(row); err != nil {
			return err
		}
	}
	cw.Flush()
	return cw.Error()
}
