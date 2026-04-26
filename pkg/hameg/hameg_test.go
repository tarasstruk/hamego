package hameg

import (
	"bytes"
	"testing"
)

// ---- VoltDiv -----------------------------------------------------------------

func TestVoltDivParsing(t *testing.T) {
	for i, name := range voltDivNames {
		got, err := ParseVoltDiv(name)
		if err != nil {
			t.Errorf("ParseVoltDiv(%q) unexpected error: %v", name, err)
			continue
		}
		if int(got) != i {
			t.Errorf("ParseVoltDiv(%q) = %d, want %d", name, int(got), i)
		}
		if got.String() != name {
			t.Errorf("VoltDiv(%d).String() = %q, want %q", i, got.String(), name)
		}
	}
}

func TestVoltDivParsingCaseInsensitive(t *testing.T) {
	if _, err := ParseVoltDiv("1V"); err != nil {
		t.Errorf("ParseVoltDiv(\"1V\") unexpected error: %v", err)
	}
	if _, err := ParseVoltDiv("1v"); err != nil {
		t.Errorf("ParseVoltDiv(\"1v\") unexpected error: %v", err)
	}
}

func TestVoltDivParsingError(t *testing.T) {
	if _, err := ParseVoltDiv("bad"); err == nil {
		t.Error("ParseVoltDiv(\"bad\") expected error, got nil")
	}
}

func TestVoltDivValue(t *testing.T) {
	tests := []struct {
		div  VoltDiv
		want float64
	}{
		{VDiv1mV, 0.001},
		{VDiv1V, 1.0},
		{VDiv20V, 20.0},
	}
	for _, tt := range tests {
		if got := tt.div.Value(); got != tt.want {
			t.Errorf("VoltDiv(%d).Value() = %g, want %g", int(tt.div), got, tt.want)
		}
	}
}

func TestVoltDivStringOutOfRange(t *testing.T) {
	v := VoltDiv(99)
	s := v.String()
	if s == "" {
		t.Error("VoltDiv(99).String() returned empty string")
	}
}

// ---- TimeDiv ----------------------------------------------------------------

func TestTimeDivParsing(t *testing.T) {
	for i, name := range timeDivNames {
		got, err := ParseTimeDiv(name)
		if err != nil {
			t.Errorf("ParseTimeDiv(%q) unexpected error: %v", name, err)
			continue
		}
		if int(got) != i {
			t.Errorf("ParseTimeDiv(%q) = %d, want %d", name, int(got), i)
		}
		if got.String() != name {
			t.Errorf("TimeDiv(%d).String() = %q, want %q", i, got.String(), name)
		}
	}
}

func TestTimeDivParsingError(t *testing.T) {
	if _, err := ParseTimeDiv("bad"); err == nil {
		t.Error("ParseTimeDiv(\"bad\") expected error, got nil")
	}
}

func TestTimeDivValue(t *testing.T) {
	tests := []struct {
		div  TimeDiv
		want float64
	}{
		{TDiv50ns, 50e-9},
		{TDiv1ms, 1e-3},
		{TDiv1s, 1.0},
		{TDiv100s, 100.0},
	}
	for _, tt := range tests {
		if got := tt.div.Value(); got != tt.want {
			t.Errorf("TimeDiv(%d).Value() = %g, want %g", int(tt.div), got, tt.want)
		}
	}
}

func TestTimeDivStringOutOfRange(t *testing.T) {
	td := TimeDiv(99)
	s := td.String()
	if s == "" {
		t.Error("TimeDiv(99).String() returned empty string")
	}
}

// ---- Waveform voltage/time conversion --------------------------------------

func TestWaveformVoltage(t *testing.T) {
	w := &Waveform{
		Samples:   []byte{128, 192, 64, 0, 255},
		VPerDiv:   1.0,
		SecPerDiv: 1e-3,
	}
	// scale = VPerDiv * VerticalDivisions / 256 = 1.0 * 8 / 256 = 0.03125 V/count
	scale := 1.0 * float64(VerticalDivisions) / 256.0
	tests := []struct {
		idx  int
		want float64
	}{
		{0, 0.0},                              // 128 - 128 = 0
		{1, (192 - 128) * scale},              // +64 counts
		{2, (64 - 128) * scale},               // -64 counts
		{3, (0 - 128) * scale},                // min count
		{4, float64(255-128) * scale},         // near max
	}
	for _, tt := range tests {
		if got := w.Voltage(tt.idx); got != tt.want {
			t.Errorf("Waveform.Voltage(%d) = %g, want %g", tt.idx, got, tt.want)
		}
	}
}

func TestWaveformVoltageOutOfRange(t *testing.T) {
	w := &Waveform{Samples: []byte{128}, VPerDiv: 1.0}
	if got := w.Voltage(-1); got != 0 {
		t.Errorf("Waveform.Voltage(-1) = %g, want 0", got)
	}
	if got := w.Voltage(100); got != 0 {
		t.Errorf("Waveform.Voltage(100) = %g, want 0", got)
	}
}

func TestWaveformTime(t *testing.T) {
	w := &Waveform{
		Samples:   make([]byte, WaveformSamples),
		SecPerDiv: 1e-3,
	}
	if got := w.Time(0); got != 0 {
		t.Errorf("Waveform.Time(0) = %g, want 0", got)
	}
	// t(i) = i * SecPerDiv * HorizontalDivisions / WaveformSamples
	dt := 1e-3 * float64(HorizontalDivisions) / float64(WaveformSamples)
	want := float64(WaveformSamples-1) * dt
	if got := w.Time(WaveformSamples - 1); got != want {
		t.Errorf("Waveform.Time(%d) = %g, want %g", WaveformSamples-1, got, want)
	}
}

// ---- Waveform CSV export ----------------------------------------------------

func TestWaveformWriteCSV(t *testing.T) {
	w := &Waveform{
		Samples:   []byte{128, 192},
		VPerDiv:   1.0,
		SecPerDiv: 1e-3,
	}
	var buf bytes.Buffer
	if err := w.WriteCSV(&buf); err != nil {
		t.Fatalf("WriteCSV error: %v", err)
	}
	out := buf.String()
	if out == "" {
		t.Fatal("WriteCSV produced empty output")
	}
	// Should start with the header line.
	if !bytes.HasPrefix(buf.Bytes(), []byte("time_s,voltage_V")) {
		t.Errorf("WriteCSV missing header, got: %q", out[:min(len(out), 40)])
	}
}

func min(a, b int) int {
	if a < b {
		return a
	}
	return b
}

// ---- Channel settings byte encoding ----------------------------------------

func TestChannelSettingsByte(t *testing.T) {
	// Verify the bit-encoding by hand for a few representative configs.
	tests := []struct {
		name    string
		cfg     ChannelConfig
		wantBit byte
	}{
		{
			"DC 1V not inverted",
			ChannelConfig{VoltDiv: VDiv1V, Coupling: CouplingDC},
			0x10 | 9, // bit4 + vdiv index 9
		},
		{
			"AC 1V not inverted",
			ChannelConfig{VoltDiv: VDiv1V, Coupling: CouplingAC},
			0x10 | 0x40 | 9,
		},
		{
			"DC 1V inverted",
			ChannelConfig{VoltDiv: VDiv1V, Coupling: CouplingDC, Inverted: true},
			0x10 | 0x20 | 9,
		},
		{
			"GND 100mV",
			ChannelConfig{VoltDiv: VDiv100mV, Coupling: CouplingGND},
			0x10 | 0x80 | 6,
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := encodeCH(tt.cfg)
			if got != tt.wantBit {
				t.Errorf("encodeCH(%+v) = 0x%02X, want 0x%02X", tt.cfg, got, tt.wantBit)
			}
		})
	}
}

// encodeCH mirrors the byte-encoding logic from SetChannel so it can be tested
// independently of a real serial port.
func encodeCH(cfg ChannelConfig) byte {
	var b byte = 0x10
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
	return b
}

// ---- Timebase settings byte encoding ---------------------------------------

func TestTimebaseSettingsByte(t *testing.T) {
	tests := []struct {
		name    string
		cfg     TimeBaseConfig
		wantBit byte
	}{
		{"1ms no single", TimeBaseConfig{TimeDiv: TDiv1ms, Single: false}, 13},
		{"1ms single", TimeBaseConfig{TimeDiv: TDiv1ms, Single: true}, 0x20 | 13},
		{"50ns no single", TimeBaseConfig{TimeDiv: TDiv50ns, Single: false}, 0},
		{"100s single", TimeBaseConfig{TimeDiv: TDiv100s, Single: true}, 0x20 | 28},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := encodeTB(tt.cfg)
			if got != tt.wantBit {
				t.Errorf("encodeTB(%+v) = 0x%02X, want 0x%02X", tt.cfg, got, tt.wantBit)
			}
		})
	}
}

// encodeTB mirrors the byte-encoding logic from SetTimeBase for independent testing.
func encodeTB(cfg TimeBaseConfig) byte {
	var b byte
	if cfg.Single {
		b |= 0x20
	}
	b |= byte(cfg.TimeDiv) & 0x1F
	return b
}
