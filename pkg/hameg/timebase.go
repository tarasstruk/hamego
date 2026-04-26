package hameg

import "fmt"

// TimeBaseConfig holds the configuration for a sweep time base.
type TimeBaseConfig struct {
	TimeDiv TimeDiv // horizontal time-per-division
	Single  bool    // true = single-shot (HOLD) mode
}

// SetTimeBase applies cfg to the specified time base on the oscilloscope.
//
// The timebase settings byte encoding follows the Hameg HM1507 RS-232 protocol:
//
//	bit 5 (0x20): single-shot mode
//	bits 0–4:     TimeDiv index (0 = 50 ns/div … 28 = 100 s/div)
func (o *Oscilloscope) SetTimeBase(base TimeBase, cfg TimeBaseConfig) error {
	if int(cfg.TimeDiv) >= len(timeDivValues) {
		return fmt.Errorf("invalid TimeDiv index %d", int(cfg.TimeDiv))
	}

	var b byte
	if cfg.Single {
		b |= 0x20
	}
	b |= byte(cfg.TimeDiv) & 0x1F

	var baseName string
	switch base {
	case TimeBaseA:
		baseName = "A"
	case TimeBaseB:
		baseName = "B"
	default:
		return fmt.Errorf("invalid time base %d: must be TimeBaseA or TimeBaseB", int(base))
	}

	// Build: "TB<A|B>=<binary_byte>\r"
	cmd := []byte(fmt.Sprintf("TB%s=", baseName))
	cmd = append(cmd, b, '\r')
	return o.writeRaw(cmd)
}
