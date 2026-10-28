package csi

import (
	"bufio"
	"context"
	"encoding/base64"
	"fmt"
	"net"
	"strings"
	"time"
)

// Dataplane is what the node plugin needs from the node's data plane (flaminio-io node).
type Dataplane interface {
	// Status returns the attached volume and its device, or empty strings.
	Status(ctx context.Context) (volume, device string, err error)
	// Attach starts the volume's target here with the capability token, and returns its device.
	Attach(ctx context.Context, volume string, token []byte) (device string, err error)
	// Detach stops the volume's target and device. Not attached is not an error.
	Detach(ctx context.Context, volume string) error
	// Delete removes this node's chunk replicas of a volume deleted from the metadata service.
	// Nothing to delete is not an error.
	Delete(ctx context.Context, volume string) error
}

// ControlSocket talks to flaminio-io node over its control socket: one command per
// connection, one line each way (dataplane/crates/flaminio-io/src/control.rs).
type ControlSocket struct {
	Path string
	// Timeout bounds one command; zero or less means DefaultControlTimeout. A caller's earlier
	// deadline wins.
	Timeout time.Duration
}

// DefaultControlTimeout bounds a control command when the caller's deadline does not come
// first (docs/6). Kubelet's calls have a deadline of their own (2 minutes), so this is a
// backstop for a data plane that never answers, and the limit for callers without one, such
// as the tombstone sweep. It must stay above the data plane's own limits on a command
// (ATTACH_LIMIT and DELETE_LIMIT, 50 s, in control.rs, whose tests read this line), so that
// the data plane's answer, which names the cause, comes first.
const DefaultControlTimeout = 60 * time.Second

func (s ControlSocket) timeout() time.Duration {
	if s.Timeout <= 0 {
		return DefaultControlTimeout
	}
	return s.Timeout
}

// answerMargin is how much sooner than this caller the data plane must end an attach, for its
// answer to come back in time.
const answerMargin = time.Second

// deadline is when a command gives up: after its timeout, or at the caller's deadline if that
// comes first.
func (s ControlSocket) deadline(ctx context.Context) time.Time {
	deadline := time.Now().Add(s.timeout())
	if dl, ok := ctx.Deadline(); ok && dl.Before(deadline) {
		deadline = dl
	}
	return deadline
}

func (s ControlSocket) command(ctx context.Context, line string) (string, error) {
	var d net.Dialer
	conn, err := d.DialContext(ctx, "unix", s.Path)
	if err != nil {
		return "", fmt.Errorf("data plane: %w", err)
	}
	defer conn.Close()
	_ = conn.SetDeadline(s.deadline(ctx))
	if _, err := fmt.Fprintf(conn, "%s\n", line); err != nil {
		return "", fmt.Errorf("data plane: %w", err)
	}
	answer, err := bufio.NewReader(conn).ReadString('\n')
	if err != nil {
		return "", fmt.Errorf("data plane: no answer: %w", err)
	}
	answer = strings.TrimSpace(answer)
	if rest, ok := strings.CutPrefix(answer, "ok"); ok {
		return strings.TrimSpace(rest), nil
	}
	return "", fmt.Errorf("data plane: %s", strings.TrimPrefix(answer, "error: "))
}

func (s ControlSocket) Status(ctx context.Context) (string, string, error) {
	answer, err := s.command(ctx, "status")
	if err != nil {
		return "", "", err
	}
	if answer == "none" {
		return "", "", nil
	}
	volume, device, ok := strings.Cut(answer, " ")
	if !ok {
		return "", "", fmt.Errorf("data plane: unexpected status %q", answer)
	}
	return volume, device, nil
}

// Attach tells the data plane how long this caller still waits, less answerMargin, so that it
// ends the attach in time to answer with its cause rather than leave the caller with only its
// own deadline (docs/6).
func (s ControlSocket) Attach(ctx context.Context, volume string, token []byte) (string, error) {
	within := max(time.Until(s.deadline(ctx))-answerMargin, 0)
	line := fmt.Sprintf("attach %s %s %d", volume, base64.StdEncoding.EncodeToString(token), within.Milliseconds())
	return s.command(ctx, line)
}

func (s ControlSocket) Detach(ctx context.Context, volume string) error {
	_, err := s.command(ctx, "detach "+volume)
	if err != nil && strings.Contains(err.Error(), "is not attached here") {
		return nil
	}
	return err
}

func (s ControlSocket) Delete(ctx context.Context, volume string) error {
	_, err := s.command(ctx, "delete "+volume)
	return err
}
