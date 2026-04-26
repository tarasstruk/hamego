package hameg

import (
	"errors"
	"fmt"
	"time"

	"go.bug.st/serial"
)

// Oscilloscope represents an open connection to a Hameg HM1507 oscilloscope.
//
// Create one with Open, then call Close when done. All methods are safe to
// call from a single goroutine; the type is not concurrency-safe.
type Oscilloscope struct {
	port    serial.Port
	timeout time.Duration
}

// Open establishes a serial connection to the oscilloscope on portName.
//
// It configures the port to 8N2 (8 data bits, no parity, 2 stop bits) with
// hardware RTS flow control asserted, then performs the HM1507 baud-rate
// handshake (SPACE + CR) required before any other commands are accepted.
func Open(portName string, baudRate int) (*Oscilloscope, error) {
	mode := &serial.Mode{
		BaudRate: baudRate,
		DataBits: 8,
		Parity:   serial.NoParity,
		StopBits: serial.TwoStopBits,
	}
	port, err := serial.Open(portName, mode)
	if err != nil {
		return nil, fmt.Errorf("opening serial port %s: %w", portName, err)
	}

	// Assert RTS so the scope will accept commands.
	if err := port.SetRTS(true); err != nil {
		_ = port.Close()
		return nil, fmt.Errorf("asserting RTS on %s: %w", portName, err)
	}

	osc := &Oscilloscope{
		port:    port,
		timeout: 2 * time.Second,
	}

	// Baud-rate auto-detection handshake: send SPACE + CR.
	if err := osc.writeLine(" "); err != nil {
		_ = port.Close()
		return nil, fmt.Errorf("baud-rate handshake on %s: %w", portName, err)
	}

	return osc, nil
}

// Close releases the serial port, first sending the "rm0" command to return
// the scope to local (front-panel) control.
func (o *Oscilloscope) Close() error {
	_ = o.writeLine("rm0")
	return o.port.Close()
}

// SetTimeout overrides the read timeout used for all serial read operations.
// The default is 2 seconds.
func (o *Oscilloscope) SetTimeout(d time.Duration) {
	o.timeout = d
}

// Autoset triggers the oscilloscope's automatic setup function, equivalent to
// pressing the AUTOSET button on the front panel.
func (o *Oscilloscope) Autoset() error {
	return o.writeLine("AUTOSET")
}

// writeLine sends an ASCII command string followed by a carriage-return (0x0D)
// to the oscilloscope. This is used for pure-ASCII commands.
func (o *Oscilloscope) writeLine(cmd string) error {
	buf := append([]byte(cmd), '\r')
	_, err := o.port.Write(buf)
	return err
}

// writeRaw sends an arbitrary byte sequence to the oscilloscope without any
// automatic CR appending. Use this for commands that embed binary parameters.
func (o *Oscilloscope) writeRaw(data []byte) error {
	_, err := o.port.Write(data)
	return err
}

// readExact reads exactly n bytes from the oscilloscope within the configured
// timeout. It retries short reads until all bytes arrive or the timeout fires.
func (o *Oscilloscope) readExact(n int) ([]byte, error) {
	if err := o.port.SetReadTimeout(o.timeout); err != nil {
		return nil, fmt.Errorf("setting read timeout: %w", err)
	}
	buf := make([]byte, n)
	total := 0
	for total < n {
		nr, err := o.port.Read(buf[total:])
		total += nr
		if err != nil {
			return buf[:total], fmt.Errorf("read error after %d/%d bytes: %w", total, n, err)
		}
		if nr == 0 {
			return buf[:total], errors.New("read timeout: no data received from oscilloscope")
		}
	}
	return buf, nil
}

// query sends a query command (appending "?\r") and reads responseLen bytes.
//
// Query commands follow the pattern: COMMAND_NAME + "?" + CR, and the scope
// responds with responseLen bytes of binary data.
func (o *Oscilloscope) query(cmd string, responseLen int) ([]byte, error) {
	if err := o.writeLine(cmd + "?"); err != nil {
		return nil, fmt.Errorf("sending query %q: %w", cmd, err)
	}
	data, err := o.readExact(responseLen)
	if err != nil {
		return data, fmt.Errorf("reading response for query %q: %w", cmd, err)
	}
	return data, nil
}
