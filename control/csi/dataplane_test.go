package csi

import (
	"bufio"
	"context"
	"net"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
	"time"
)

// controlServer listens on a Unix socket and, for each connection, reads the command, waits
// `delay`, then answers `answer`; with no answer it holds the connection until the test ends.
// The commands it read go to `lines`, if not nil. The socket lives in a short directory: macOS
// allows socket paths of only 104 bytes.
func controlServer(t *testing.T, delay time.Duration, answer string, lines chan<- string) string {
	t.Helper()
	dir, err := os.MkdirTemp("", "fl")
	if err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, "c.sock")
	l, err := net.Listen("unix", path)
	if err != nil {
		t.Fatal(err)
	}
	done := make(chan struct{})
	served := make(chan struct{})
	go func() {
		defer close(served)
		for {
			conn, err := l.Accept()
			if err != nil {
				return
			}
			line, _ := bufio.NewReader(conn).ReadString('\n')
			if lines != nil {
				lines <- strings.TrimSpace(line)
			}
			if answer == "" {
				<-done
			} else {
				time.Sleep(delay)
				_, _ = conn.Write([]byte(answer + "\n"))
			}
			_ = conn.Close()
		}
	}()
	t.Cleanup(func() {
		close(done)
		_ = l.Close()
		<-served
		_ = os.RemoveAll(dir)
	})
	return path
}

func TestControlSocketGivesUpOnASilentDataPlane(t *testing.T) {
	path := controlServer(t, 0, "", nil)
	start := time.Now()
	_, err := ControlSocket{Path: path, Timeout: 50 * time.Millisecond}.Attach(context.Background(), "v", []byte("t"))
	if err == nil || !strings.Contains(err.Error(), "no answer") {
		t.Fatalf("got %v, want no answer", err)
	}
	if d := time.Since(start); d > time.Second {
		t.Fatalf("gave up after %v", d)
	}
}

func TestControlSocketKeepsTheCallersEarlierDeadline(t *testing.T) {
	path := controlServer(t, 0, "", nil)
	ctx, cancel := context.WithTimeout(context.Background(), 50*time.Millisecond)
	defer cancel()
	start := time.Now()
	_, err := ControlSocket{Path: path, Timeout: 10 * time.Second}.Attach(ctx, "v", []byte("t"))
	if err == nil {
		t.Fatal("attach succeeded")
	}
	if d := time.Since(start); d > time.Second {
		t.Fatalf("gave up after %v", d)
	}
}

func TestControlSocketDefaultWaitsForTheDataPlane(t *testing.T) {
	// main.go sets no Timeout; a negative one falls back too. That the default is above the
	// data plane's own limits is checked on the Rust side, which reads it (control.rs).
	for _, s := range []ControlSocket{{}, {Timeout: -time.Second}} {
		if got := s.timeout(); got != DefaultControlTimeout {
			t.Fatalf("timeout() with %v = %v, want %v", s.Timeout, got, DefaultControlTimeout)
		}
	}
	path := controlServer(t, 200*time.Millisecond, "ok /dev/ublkb3", nil)
	dev, err := ControlSocket{Path: path}.Attach(context.Background(), "v", []byte("t"))
	if err != nil || dev != "/dev/ublkb3" {
		t.Fatalf("got %q, %v", dev, err)
	}
}

func TestControlSocketTellsTheDataPlaneItsTimeLeft(t *testing.T) {
	lines := make(chan string, 1)
	path := controlServer(t, 0, "ok /dev/ublkb3", lines)
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if _, err := (ControlSocket{Path: path}).Attach(ctx, "v", []byte("t")); err != nil {
		t.Fatal(err)
	}
	// "attach v dA== MS": the caller's 3 s, less the answer's margin.
	fields := strings.Fields(<-lines)
	if len(fields) != 4 {
		t.Fatalf("command %q", fields)
	}
	ms, err := strconv.Atoi(fields[3])
	if err != nil || ms > 2000 || ms < 1500 {
		t.Fatalf("time left %q, want about 2000 ms", fields[3])
	}
}

// An older data plane takes only `attach VOLUME TOKEN`: the plugin sends that after its usage
// line, so that the two can be upgraded in either order.
func TestControlSocketFallsBackForAnOlderDataPlane(t *testing.T) {
	dir, err := os.MkdirTemp("", "fl")
	if err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, "c.sock")
	l, err := net.Listen("unix", path)
	if err != nil {
		t.Fatal(err)
	}
	served := make(chan struct{})
	go func() {
		defer close(served)
		for {
			conn, err := l.Accept()
			if err != nil {
				return
			}
			line, _ := bufio.NewReader(conn).ReadString('\n')
			answer := "ok /dev/ublkb3"
			if len(strings.Fields(line)) != 3 {
				answer = "error: usage: attach VOLUME TOKEN-BASE64 | detach VOLUME | delete VOLUME | status"
			}
			_, _ = conn.Write([]byte(answer + "\n"))
			_ = conn.Close()
		}
	}()
	t.Cleanup(func() {
		_ = l.Close()
		<-served
		_ = os.RemoveAll(dir)
	})
	dev, err := ControlSocket{Path: path}.Attach(context.Background(), "v", []byte("t"))
	if err != nil || dev != "/dev/ublkb3" {
		t.Fatalf("got %q, %v", dev, err)
	}
}
