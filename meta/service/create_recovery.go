package service

import (
	"context"
	"errors"
	"log/slog"
	"time"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"

	"github.com/flaminio/flaminio/meta/domain"
	"github.com/flaminio/flaminio/meta/kv"
)

const (
	recoveryInterval     = 5 * time.Second
	recoveryPassTimeout  = 2 * time.Second
	recoveryPageSize     = 64
	recoveryMaxItems     = 64
	recoveryMaxMutations = 8
	recoveryMaxKVCalls   = 512
	recoveryCohortSize   = 64
)

// recoverySource is one of the four rotating discovery sources.
type recoverySource int

const (
	srcRoots recoverySource = iota
	srcHeaders
	srcPlanOrphans
	srcChunkOrphans
	srcCount
)

type sourceCursor struct {
	after   string
	tail    []kv.KeyRev
	next    string
	done    bool
	fetched bool
}

type rootObservation struct {
	key string
	rev int64
	at  time.Time
}

// StartCreateRecovery runs RunCreateRecovery immediately and every 5s until StopCreateRecovery.
// The worker is joined before returning from Stop so KV may Close afterwards.
func (s *Service) StartCreateRecovery(parent context.Context) {
	s.recoveryMu.Lock()
	defer s.recoveryMu.Unlock()
	if s.recoveryStop != nil {
		return
	}
	ctx, cancel := context.WithCancel(parent)
	s.recoveryStop = cancel
	s.recoveryDone = make(chan struct{})
	go func() {
		defer close(s.recoveryDone)
		s.createRecoveryLoop(ctx)
	}()
}

// StopCreateRecovery cancels the recovery worker and waits for it to exit.
func (s *Service) StopCreateRecovery() {
	s.recoveryMu.Lock()
	cancel := s.recoveryStop
	done := s.recoveryDone
	s.recoveryStop = nil
	s.recoveryDone = nil
	s.recoveryMu.Unlock()
	if cancel != nil {
		cancel()
	}
	if done != nil {
		<-done
	}
}

func (s *Service) createRecoveryLoop(ctx context.Context) {
	s.runRecoveryPass(ctx)
	t := time.NewTicker(recoveryInterval)
	defer t.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-t.C:
			s.runRecoveryPass(ctx)
		}
	}
}

func (s *Service) runRecoveryPass(parent context.Context) {
	s.recoveryPassMu.Lock()
	defer s.recoveryPassMu.Unlock()
	ctx, cancel := context.WithTimeout(parent, recoveryPassTimeout)
	defer cancel()
	ctx = kv.WithWorkBudget(ctx, recoveryMaxKVCalls)
	if err := s.RunCreateRecovery(ctx); err != nil && !errors.Is(err, context.Canceled) && !errors.Is(err, context.DeadlineExceeded) && !errors.Is(err, kv.ErrWorkBudget) {
		slog.Warn("create recovery pass", "err", err)
	}
	s.recoveryFirstSrc = (s.recoveryFirstSrc + 1) % int(srcCount)
}

// RunCreateRecovery performs one bounded non-overlapping recovery pass (docs/issues/76 S7).
func (s *Service) RunCreateRecovery(ctx context.Context) error {
	cursors := s.recoveryCursors
	if cursors == nil {
		cursors = make([]sourceCursor, srcCount)
		s.recoveryCursors = cursors
	}
	items := 0
	mutations := 0
	seenOps := map[string]struct{}{}

	order := make([]recoverySource, 0, srcCount)
	for i := 0; i < int(srcCount); i++ {
		order = append(order, recoverySource((s.recoveryFirstSrc+i)%int(srcCount)))
	}

	for _, src := range order {
		if items >= recoveryMaxItems || mutations >= recoveryMaxMutations {
			break
		}
		if err := ctx.Err(); err != nil {
			return err
		}
		n, m, err := s.recoverSource(ctx, src, &cursors[src], &items, &mutations, seenOps)
		_ = n
		if err != nil {
			if errors.Is(err, kv.ErrWorkBudget) || errors.Is(err, context.Canceled) || errors.Is(err, context.DeadlineExceeded) {
				return err
			}
			slog.Warn("create recovery source", "src", int(src), "err", err)
		}
		_ = m
	}
	s.advanceCohort(ctx)
	return nil
}

func (s *Service) recoverSource(ctx context.Context, src recoverySource, cur *sourceCursor, items, mutations *int, seenOps map[string]struct{}) (int, int, error) {
	if !cur.fetched || (len(cur.tail) == 0 && !cur.done) {
		if err := s.fetchSourcePage(ctx, src, cur); err != nil {
			return 0, 0, err
		}
	}
	examined := 0
	for len(cur.tail) > 0 && *items < recoveryMaxItems && *mutations < recoveryMaxMutations {
		if err := ctx.Err(); err != nil {
			return examined, 0, err
		}
		kr := cur.tail[0]
		cur.tail = cur.tail[1:]
		cur.after = kr.Key // advance only through examined keys
		*items++
		examined++

		op := ""
		switch src {
		case srcRoots:
			op = parseCreateOpFromRootKey(kr.Key)
		case srcPlanOrphans:
			op, _ = parseCreateOpFromPlanKey(kr.Key)
		case srcHeaders:
			// volume id from key
		}

		if op != "" {
			if _, ok := seenOps[op]; ok {
				continue
			}
			seenOps[op] = struct{}{}
		}

		mutated, err := s.processRecoveryItem(ctx, src, kr)
		if err != nil {
			if errors.Is(err, kv.ErrWorkBudget) {
				return examined, 0, err
			}
			// examined keys that error still consume the item budget
			continue
		}
		if mutated {
			*mutations++
		}
	}
	if len(cur.tail) == 0 && cur.done {
		// wrap only after the tail is consumed
		cur.after = ""
		cur.fetched = false
		cur.done = false
		cur.next = ""
	} else if len(cur.tail) == 0 && !cur.done {
		cur.fetched = false // fetch next page using after
	}
	return examined, 0, nil
}

func (s *Service) fetchSourcePage(ctx context.Context, src recoverySource, cur *sourceCursor) error {
	if len(cur.tail) > 0 {
		return nil // do not refetch/overwrite an unconsumed tail
	}
	var pfx string
	switch src {
	case srcRoots:
		pfx = createRootPrefix
	case srcHeaders:
		pfx = "/flaminio/v1/volumes/"
	case srcPlanOrphans:
		pfx = createPlanPrefix
	case srcChunkOrphans:
		pfx = "/flaminio/v1/chunks/"
	}
	page, err := s.kv.ScanKeys(ctx, pfx, cur.after, recoveryPageSize)
	if err != nil {
		return err
	}
	cur.tail = append([]kv.KeyRev(nil), page.Entries...)
	cur.next = page.Next
	cur.done = page.Done
	cur.fetched = true
	return nil
}

func (s *Service) processRecoveryItem(ctx context.Context, src recoverySource, kr kv.KeyRev) (bool, error) {
	switch src {
	case srcRoots:
		return s.recoverRoot(ctx, kr)
	case srcHeaders:
		return s.recoverHeader(ctx, kr)
	case srcPlanOrphans:
		return s.recoverPlanOrphan(ctx, kr)
	case srcChunkOrphans:
		return s.recoverChunkOrphan(ctx, kr)
	}
	return false, nil
}

func (s *Service) recoverRoot(ctx context.Context, kr kv.KeyRev) (bool, error) {
	e, ok, err := s.kv.GetBounded(ctx, kr.Key, domain.MaxCreateRecordBytes)
	if err != nil || !ok {
		return false, err
	}
	root, err := decodeRoot(e.Value)
	if err != nil {
		return false, err
	}
	op := parseCreateOpFromRootKey(kr.Key)

	he, hok, herr := s.kv.GetBounded(ctx, volumeKey(root.Volume), domain.MaxCreateRecordBytes)
	var h domain.Header
	if herr != nil && !errors.Is(herr, kv.ErrOversized) {
		return false, herr
	}
	if hok && herr == nil {
		h, _ = decodeHeaderBounded(he.Value)
	}

	switch root.State {
	case rootCleaning, rootCopying:
		if hok && h.Creating && !h.Deleting && h.CreateOperation == op {
			_, err := s.helpCreate(ctx, op, he.Rev, h)
			return err == nil, err
		}
		// admitted but header absent/Deleting/other op → GC eligible
		if s.gcEligibleAdmitted(root, hok, h, op) {
			return s.gcRoot(ctx, kr.Key, e.Rev, volumeKey(root.Volume), he.Rev, hok)
		}
	case rootComplete:
		if hok && !h.Creating && !h.Deleting && h.CreateOperation == op {
			return s.gcRoot(ctx, kr.Key, e.Rev, volumeKey(root.Volume), he.Rev, true)
		}
	case rootPreparing, rootSealed:
		s.noteRootObservation(kr.Key, e.Rev)
		if s.unchangedPreparing(kr.Key, e.Rev) && !hok {
			return s.gcRoot(ctx, kr.Key, e.Rev, volumeKey(root.Volume), 0, false)
		}
	}
	return false, nil
}

func (s *Service) gcEligibleAdmitted(root createRoot, hok bool, h domain.Header, op string) bool {
	if !hok {
		return true
	}
	if h.Deleting {
		return true
	}
	if h.CreateOperation != "" && h.CreateOperation != op {
		return true
	}
	return false
}

func (s *Service) noteRootObservation(key string, rev int64) {
	if s.recoveryCohort == nil {
		s.recoveryCohort = map[string]rootObservation{}
	}
	s.recoveryCohort[key] = rootObservation{key: key, rev: rev, at: time.Now()}
	// Bound cohort size.
	for len(s.recoveryCohort) > recoveryCohortSize {
		var oldest string
		var t time.Time
		first := true
		for k, o := range s.recoveryCohort {
			if first || o.at.Before(t) {
				oldest, t, first = k, o.at, false
			}
		}
		delete(s.recoveryCohort, oldest)
	}
}

func (s *Service) unchangedPreparing(key string, rev int64) bool {
	o, ok := s.recoveryPrev[key]
	return ok && o.rev == rev
}

func (s *Service) advanceCohort(ctx context.Context) {
	_ = ctx
	s.recoveryPrev = s.recoveryCohort
	s.recoveryCohort = map[string]rootObservation{}
}

func (s *Service) gcRoot(ctx context.Context, rootKey string, rootRev int64, headerKey string, headerRev int64, headerPresent bool) (bool, error) {
	cmps := []kv.Cmp{{Key: rootKey, Rev: rootRev}}
	if headerPresent {
		cmps = append(cmps, kv.Cmp{Key: headerKey, Rev: headerRev})
	} else {
		cmps = append(cmps, kv.Cmp{Key: headerKey, Rev: 0})
	}
	ok, err := s.kv.Txn(ctx, cmps, []kv.Op{{Key: rootKey, Delete: true}})
	return ok, err
}

func (s *Service) recoverHeader(ctx context.Context, kr kv.KeyRev) (bool, error) {
	e, ok, err := s.kv.GetBounded(ctx, kr.Key, domain.MaxCreateRecordBytes)
	if err != nil || !ok {
		return false, err
	}
	h, err := decodeHeaderBounded(e.Value)
	if err != nil {
		return false, err
	}
	id := h.ID
	if id == "" {
		id = kr.Key[len("/flaminio/v1/volumes/"):]
		h.ID = id
	}
	// Deleting first.
	if h.Deleting {
		if isUnpublishedCreating(h) {
			err := s.stepUnpublishedRollback(ctx, h, e.Rev)
			return err == nil, err
		}
		return false, nil // published deletion: leave conservative path
	}
	if h.Creating {
		if h.CreateOperation == "" {
			// Legacy: leave for explicit delete / compatible retry; do not invent a plan.
			return false, nil
		}
		re, rok, rerr := s.kv.GetBounded(ctx, createRootKey(h.CreateOperation), domain.MaxCreateRecordBytes)
		if rerr != nil {
			return false, rerr
		}
		if !rok {
			return false, status.Errorf(codes.DataLoss, "creating volume %q missing create root %s", id, truncID(h.CreateOperation))
		}
		root, err := decodeRoot(re.Value)
		if err != nil {
			return false, err
		}
		_, err = s.progressCreate(ctx, h.CreateOperation, root, re.Rev, e.Rev, &domain.Volume{Header: h})
		return err == nil, err
	}
	return false, nil
}

func (s *Service) recoverPlanOrphan(ctx context.Context, kr kv.KeyRev) (bool, error) {
	op, ok := parseCreateOpFromPlanKey(kr.Key)
	if !ok {
		return false, nil
	}
	_, rok, err := s.kv.GetBounded(ctx, createRootKey(op), domain.MaxCreateRecordBytes)
	if err != nil {
		return false, err
	}
	if rok {
		return false, nil // live root owns these plans
	}
	// Delete up to 32 plan keys for this op comparing root absence and per-key revisions.
	page, err := s.kv.ScanKeys(ctx, createPlanOpPrefix(op), "", createBatchSize)
	if err != nil {
		return false, err
	}
	if len(page.Entries) == 0 {
		return false, nil
	}
	cmps := []kv.Cmp{{Key: createRootKey(op), Rev: 0}}
	ops := make([]kv.Op, 0, len(page.Entries))
	for _, e := range page.Entries {
		cmps = append(cmps, kv.Cmp(e))
		ops = append(ops, kv.Op{Key: e.Key, Delete: true})
	}
	applied, err := s.kv.Txn(ctx, cmps, ops)
	return applied, err
}

func (s *Service) recoverChunkOrphan(ctx context.Context, kr kv.KeyRev) (bool, error) {
	// chunks/<id>/<index> → volume id between prefixes
	rest := kr.Key[len("/flaminio/v1/chunks/"):]
	slash := -1
	for i := 0; i < len(rest); i++ {
		if rest[i] == '/' {
			slash = i
			break
		}
	}
	if slash <= 0 {
		return false, nil
	}
	id := rest[:slash]
	_, hok, err := s.kv.Get(ctx, volumeKey(id))
	if err != nil {
		return false, err
	}
	if hok {
		return false, nil
	}
	err = s.cleanupOrphanPublicChunks(ctx, id)
	return err == nil, err
}
