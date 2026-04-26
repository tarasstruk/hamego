package hameg

import "fmt"

// ChannelConfig holds the configuration for a single oscilloscope input channel.
type ChannelConfig struct {
	VoltDiv  VoltDiv  // vertical sensitivity
	Coupling Coupling // input coupling (DC, AC, GND)
	Inverted bool     // invert the waveform display
}

// SetChannel applies cfg to the specified channel on the oscilloscope.
//
// The channel settings byte encoding follows the Hameg HM1507 RS-232 protocol:
//
//	bit 4 (0x10): channel enabled (always set)
//	bit 5 (0x20): invert channel
//	bit 6 (0x40): AC coupling (clear = DC coupling)
//	bit 7 (0x80): GND coupling
//	bits 0–3:     VoltDiv index (0 = 1 mV/div … 13 = 20 V/div)
func (o *Oscilloscope) SetChannel(ch Channel, cfg ChannelConfig) error {
	if ch != Channel1 && ch != Channel2 {
		return fmt.Errorf("invalid channel %d: must be 1 or 2", int(ch))
	}
	if int(cfg.VoltDiv) >= len(voltDivValues) {
		return fmt.Errorf("invalid VoltDiv index %d", int(cfg.VoltDiv))
	}

	var b byte = 0x10 // channel enabled
	if cfg.Inverted {
		b |= 0x20
	}
	switch cfg.Coupling {
	case CouplingAC:
		b |= 0x40
	case CouplingGND:
		b |= 0x80
	}
	b |= byte(cfg.VoltDiv) & 0x0F

	// Build: "CH<n>=<binary_byte>\r"  (binary byte must not be UTF-8 encoded)
	cmd := []byte(fmt.Sprintf("CH%d=", int(ch)))
	cmd = append(cmd, b, '\r')
	return o.writeRaw(cmd)
}
