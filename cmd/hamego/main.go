// Command hamego provides a command-line interface for controlling and
// capturing data from the Hameg HM1507 oscilloscope via RS-232.
//
// Usage:
//
//	hamego <command> [flags]
//
// Commands:
//
//	autoset       Trigger the oscilloscope automatic setup
//	capture       Capture a waveform and write to CSV
//	set-channel   Configure a channel (voltage/div, coupling, inversion)
//	set-timebase  Configure a time base (time/div, single-shot mode)
package main

import (
	"fmt"
	"os"

	"github.com/spf13/cobra"
	"github.com/tarasstruk/hamego/pkg/hameg"
)

func main() {
	if err := newRootCmd().Execute(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func newRootCmd() *cobra.Command {
	root := &cobra.Command{
		Use:   "hamego",
		Short: "Interface tools for the Hameg HM1507 oscilloscope",
		Long: `hamego provides command-line tools to control and acquire data from
the Hameg HM1507 oscilloscope via its RS-232 serial interface.`,
	}
	root.AddCommand(
		newAutosetCmd(),
		newCaptureCmd(),
		newSetChannelCmd(),
		newSetTimebaseCmd(),
	)
	return root
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

func addPortFlag(cmd *cobra.Command, port *string) {
	cmd.Flags().StringVarP(port, "port", "p", "/dev/ttyUSB0",
		"serial port connected to the oscilloscope (e.g. /dev/ttyUSB0 or COM3)")
	_ = cmd.MarkFlagRequired("port")
}

func addBaudFlag(cmd *cobra.Command, baud *int) {
	cmd.Flags().IntVar(baud, "baud", hameg.DefaultBaudRate, "RS-232 baud rate")
}

func openScope(port string, baud int) (*hameg.Oscilloscope, error) {
	osc, err := hameg.Open(port, baud)
	if err != nil {
		return nil, fmt.Errorf("connecting to oscilloscope on %s: %w", port, err)
	}
	return osc, nil
}

// ---------------------------------------------------------------------------
// autoset
// ---------------------------------------------------------------------------

func newAutosetCmd() *cobra.Command {
	var port string
	var baud int

	cmd := &cobra.Command{
		Use:   "autoset",
		Short: "Trigger the oscilloscope automatic setup",
		Long: `Send the AUTOSET command to the oscilloscope.

The scope will automatically adjust the timebase and channel sensitivities
to display the input signal clearly.`,
		RunE: func(cmd *cobra.Command, args []string) error {
			osc, err := openScope(port, baud)
			if err != nil {
				return err
			}
			defer osc.Close()
			if err := osc.Autoset(); err != nil {
				return fmt.Errorf("autoset: %w", err)
			}
			fmt.Fprintln(cmd.OutOrStdout(), "Autoset command sent.")
			return nil
		},
	}
	addPortFlag(cmd, &port)
	addBaudFlag(cmd, &baud)
	return cmd
}

// ---------------------------------------------------------------------------
// capture
// ---------------------------------------------------------------------------

func newCaptureCmd() *cobra.Command {
	var port string
	var baud int
	var channel int
	var output string
	var vdiv string
	var tdiv string

	cmd := &cobra.Command{
		Use:   "capture",
		Short: "Capture a waveform from a channel and write it to CSV",
		Long: `Read 1024 raw ADC samples from the specified channel and write them as
a two-column CSV (time_s, voltage_V) to a file or stdout.

Use --vdiv and --tdiv to supply the current scope settings so that the raw
samples are correctly scaled to physical units.

Example:
  hamego capture -p /dev/ttyUSB0 -c 1 --vdiv 1V --tdiv 1ms -o waveform.csv`,
		RunE: func(cmd *cobra.Command, args []string) error {
			osc, err := openScope(port, baud)
			if err != nil {
				return err
			}
			defer osc.Close()

			ch := hameg.Channel(channel)
			wf, err := osc.ReadWaveform(ch)
			if err != nil {
				return fmt.Errorf("reading waveform: %w", err)
			}

			if vdiv != "" {
				v, err := hameg.ParseVoltDiv(vdiv)
				if err != nil {
					return err
				}
				wf.VPerDiv = v.Value()
			}
			if tdiv != "" {
				td, err := hameg.ParseTimeDiv(tdiv)
				if err != nil {
					return err
				}
				wf.SecPerDiv = td.Value()
			}

			out := cmd.OutOrStdout()
			if output != "" && output != "-" {
				f, err := os.Create(output)
				if err != nil {
					return fmt.Errorf("creating output file %s: %w", output, err)
				}
				defer f.Close()
				out = f
			}

			if err := wf.WriteCSV(out); err != nil {
				return fmt.Errorf("writing CSV: %w", err)
			}
			if output != "" && output != "-" {
				fmt.Fprintf(os.Stderr, "Waveform saved to %s (%d samples).\n",
					output, len(wf.Samples))
			}
			return nil
		},
	}
	addPortFlag(cmd, &port)
	addBaudFlag(cmd, &baud)
	cmd.Flags().IntVarP(&channel, "channel", "c", 1, "channel number (1 or 2)")
	cmd.Flags().StringVarP(&output, "output", "o", "-",
		"output CSV file path (use - or omit for stdout)")
	cmd.Flags().StringVar(&vdiv, "vdiv", "",
		"vertical sensitivity for the channel (e.g. 1V, 100mV)")
	cmd.Flags().StringVar(&tdiv, "tdiv", "",
		"time/div setting for scaling (e.g. 1ms, 500us)")
	return cmd
}

// ---------------------------------------------------------------------------
// set-channel
// ---------------------------------------------------------------------------

func newSetChannelCmd() *cobra.Command {
	var port string
	var baud int
	var channel int
	var vdiv string
	var coupling string
	var inverted bool

	cmd := &cobra.Command{
		Use:   "set-channel",
		Short: "Configure a channel (voltage/div, coupling, inversion)",
		Long: `Send a channel configuration command to the oscilloscope.

Example:
  hamego set-channel -p /dev/ttyUSB0 -c 1 --vdiv 500mV --coupling AC`,
		RunE: func(cmd *cobra.Command, args []string) error {
			osc, err := openScope(port, baud)
			if err != nil {
				return err
			}
			defer osc.Close()

			v, err := hameg.ParseVoltDiv(vdiv)
			if err != nil {
				return err
			}

			var c hameg.Coupling
			switch coupling {
			case "DC", "dc":
				c = hameg.CouplingDC
			case "AC", "ac":
				c = hameg.CouplingAC
			case "GND", "gnd":
				c = hameg.CouplingGND
			default:
				return fmt.Errorf("unknown coupling %q – use DC, AC, or GND", coupling)
			}

			cfg := hameg.ChannelConfig{
				VoltDiv:  v,
				Coupling: c,
				Inverted: inverted,
			}
			if err := osc.SetChannel(hameg.Channel(channel), cfg); err != nil {
				return fmt.Errorf("set-channel: %w", err)
			}
			fmt.Fprintf(cmd.OutOrStdout(),
				"Channel %d configured: %s %s inverted=%v\n",
				channel, vdiv, coupling, inverted)
			return nil
		},
	}
	addPortFlag(cmd, &port)
	addBaudFlag(cmd, &baud)
	cmd.Flags().IntVarP(&channel, "channel", "c", 1, "channel number (1 or 2)")
	cmd.Flags().StringVar(&vdiv, "vdiv", "1V",
		"voltage/div setting (e.g. 1V, 500mV, 100mV)")
	cmd.Flags().StringVar(&coupling, "coupling", "DC",
		"input coupling: DC, AC, or GND")
	cmd.Flags().BoolVar(&inverted, "inverted", false, "invert the channel display")
	_ = cmd.MarkFlagRequired("vdiv")
	return cmd
}

// ---------------------------------------------------------------------------
// set-timebase
// ---------------------------------------------------------------------------

func newSetTimebaseCmd() *cobra.Command {
	var port string
	var baud int
	var base string
	var tdiv string
	var single bool

	cmd := &cobra.Command{
		Use:   "set-timebase",
		Short: "Configure a time base (time/div, single-shot mode)",
		Long: `Send a timebase configuration command to the oscilloscope.

Example:
  hamego set-timebase -p /dev/ttyUSB0 --base A --tdiv 1ms
  hamego set-timebase -p /dev/ttyUSB0 --base A --tdiv 500us --single`,
		RunE: func(cmd *cobra.Command, args []string) error {
			osc, err := openScope(port, baud)
			if err != nil {
				return err
			}
			defer osc.Close()

			td, err := hameg.ParseTimeDiv(tdiv)
			if err != nil {
				return err
			}

			var tb hameg.TimeBase
			switch base {
			case "A", "a":
				tb = hameg.TimeBaseA
			case "B", "b":
				tb = hameg.TimeBaseB
			default:
				return fmt.Errorf("unknown time base %q – use A or B", base)
			}

			cfg := hameg.TimeBaseConfig{
				TimeDiv: td,
				Single:  single,
			}
			if err := osc.SetTimeBase(tb, cfg); err != nil {
				return fmt.Errorf("set-timebase: %w", err)
			}
			fmt.Fprintf(cmd.OutOrStdout(),
				"Time base %s configured: %s single=%v\n", base, tdiv, single)
			return nil
		},
	}
	addPortFlag(cmd, &port)
	addBaudFlag(cmd, &baud)
	cmd.Flags().StringVar(&base, "base", "A", "time base selector (A or B)")
	cmd.Flags().StringVar(&tdiv, "tdiv", "1ms",
		"time/div setting (e.g. 1ms, 500us, 50ns)")
	cmd.Flags().BoolVar(&single, "single", false, "enable single-shot (hold) mode")
	_ = cmd.MarkFlagRequired("tdiv")
	return cmd
}
