package csi

import (
	"bufio"
	"context"
	"encoding/base64"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"net"
	"strings"
	"time"

	"github.com/flaminio/flaminio/meta/domain"
	"github.com/flaminio/flaminio/pkg/captoken"
)

// Dataplane is what the node plugin needs from the node's data plane (flaminio-io node).
type Dataplane interface {
	// Status returns the attached volume and its device, or empty strings.
	Status(ctx context.Context) (volume, device string, err error)
	// AttachChallenge returns the healthy daemon's process nonce, before metadata issues a handoff.
	AttachChallenge(ctx context.Context) ([]byte, error)
	// Attach starts or revalidates the exact target lifetime bound to token and handoff.
	Attach(ctx context.Context, volume string, token, handoff []byte) (device string, err error)
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
// first (docs/issues/6). Kubelet's calls have a deadline of their own (2 minutes), so this is a
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
	stop := context.AfterFunc(ctx, func() { _ = conn.Close() })
	defer stop()
	_ = conn.SetDeadline(s.deadline(ctx))
	if _, err := fmt.Fprintf(conn, "%s\n", line); err != nil {
		return "", fmt.Errorf("data plane: %w", err)
	}
	answer, err := bufio.NewReader(conn).ReadString('\n')
	if err != nil {
		return "", fmt.Errorf("data plane: no answer: %w", err)
	}
	answer = strings.TrimSpace(answer)
	if answer == "ok" {
		return "", nil
	}
	if rest, ok := strings.CutPrefix(answer, "ok "); ok {
		return rest, nil
	}
	return "", controlError(strings.TrimPrefix(answer, "error: "))
}

type controlError string

func (e controlError) Error() string { return "data plane: " + string(e) }

func (s ControlSocket) AttachChallenge(ctx context.Context) ([]byte, error) {
	answer, err := s.command(ctx, "attach-challenge")
	if err != nil {
		return nil, err
	}
	nonce, err := hex.DecodeString(answer)
	if err != nil || len(answer) != 64 || answer != strings.ToLower(answer) || !captoken.ValidTargetNonce(nonce) {
		return nil, errors.New("data plane: invalid attach challenge; a healthy handoff-capable daemon is required")
	}
	return nonce, nil
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
// own deadline (docs/issues/6).
//
// Retries keep these exact credentials. A newer epoch may have to wait for the old device's
// ordinary metadata refresh, deposition and retirement; status cannot authorize adoption.
func (s ControlSocket) Attach(ctx context.Context, volume string, token, handoff []byte) (string, error) {
	if !domain.ValidID(volume) || len(token) == 0 || base64.StdEncoding.EncodedLen(len(token)) > 4096 || len(handoff) != captoken.HandoffLen {
		return "", errors.New("data plane: valid volume, bounded token and 134-byte target handoff are required")
	}
	ctx, cancel := context.WithTimeout(ctx, s.timeout())
	defer cancel()
	line := "attach " + volume + " " + base64.StdEncoding.EncodeToString(token) + " " + base64.RawURLEncoding.EncodeToString(handoff)
	for {
		within := max(time.Until(s.deadline(ctx))-answerMargin, 0)
		dev, err := s.command(ctx, fmt.Sprintf("%s %d", line, within.Milliseconds()))
		if err == nil || !retryAttach(err) {
			return dev, err
		}
		timer := time.NewTimer(100 * time.Millisecond)
		select {
		case <-ctx.Done():
			timer.Stop()
			return "", fmt.Errorf("data plane: attach wait ended: %w (last attempt: %v)", ctx.Err(), err)
		case <-timer.C:
		}
	}
}

func retryAttach(err error) bool {
	var reply controlError
	if errors.As(err, &reply) {
		return strings.Contains(string(reply), "has an existing device lifetime; retry this handoff only after that device stops") ||
			string(reply) == "existing target lifetime is no longer live" ||
			string(reply) == "already attaching" || strings.HasPrefix(string(reply), "no answer within ") ||
			string(reply) == "the node dropped the attach without an answer (see its log)"
	}
	var network net.Error
	return errors.As(err, &network) || errors.Is(err, io.EOF) || errors.Is(err, io.ErrUnexpectedEOF)
}

func (s ControlSocket) Detach(ctx context.Context, volume string) error {
	if !domain.ValidID(volume) {
		return errors.New("data plane: invalid volume id")
	}
	_, err := s.command(ctx, "detach "+volume)
	if err != nil && strings.Contains(err.Error(), "is not attached here") {
		return nil
	}
	return err
}

func (s ControlSocket) Delete(ctx context.Context, volume string) error {
	if !domain.ValidID(volume) {
		return errors.New("data plane: invalid volume id")
	}
	_, err := s.command(ctx, "delete "+volume)
	return err
}
