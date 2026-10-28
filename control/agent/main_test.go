package agent

import (
	"testing"

	"go.uber.org/goleak"
)

// TestMain fails the package's tests if any goroutine they started is still running at the end.
func TestMain(m *testing.M) { goleak.VerifyTestMain(m) }
