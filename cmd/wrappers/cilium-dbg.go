package main

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
)

func main() {
	// Find Rust binary
	rustBinary := "/opt/cilium/seriousum-dbg"
	if _, err := os.Stat(rustBinary); err != nil {
		// Try local path for development
		exePath, _ := os.Executable()
		binDir := filepath.Dir(exePath)
		rustBinary = filepath.Join(binDir, "seriousum-dbg")
	}

	// Execute Rust implementation
	cmd := exec.Command(rustBinary, os.Args[1:]...)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	cmd.Stdin = os.Stdin

	if err := cmd.Run(); err != nil {
		if exitErr, ok := err.(*exec.ExitError); ok {
			os.Exit(exitErr.ExitCode())
		}
		fmt.Fprintf(os.Stderr, "Error: %v\n", err)
		os.Exit(1)
	}
}
