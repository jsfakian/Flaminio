package domain

import (
	"bufio"
	"encoding/json"
	"os"
	"slices"
	"testing"
)

type vectorChunk struct {
	Replicas []string `json:"replicas"`
	ISR      []string `json:"isr"`
	ISRVer   uint64   `json:"isr_ver"`
}

type vector struct {
	Op        string     `json:"op"`
	MinISR    uint32     `json:"min_isr"`
	Placement [][]string `json:"placement"`
	Node      string     `json:"node"`
	Epoch     uint64     `json:"epoch"`
	Changes   []struct {
		Chunk       uint32   `json:"chunk"`
		ExpectedVer uint64   `json:"expected_ver"`
		NewISR      []string `json:"new_isr"`
	} `json:"changes"`
	Applied bool `json:"applied"`
	After   struct {
		Epoch  uint64        `json:"epoch"`
		Target string        `json:"target"`
		Chunks []vectorChunk `json:"chunks"`
	} `json:"after"`
}

// TestVectors replays the transitions recorded from the Rust reference model.
func TestVectors(t *testing.T) {
	f, err := os.Open("../testdata/isr-vectors.jsonl")
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	var v *Volume
	sc := bufio.NewScanner(f)
	sc.Buffer(nil, 1<<20)
	line, applied, rejected := 0, 0, 0
	for sc.Scan() {
		line++
		var x vector
		if err := json.Unmarshal(sc.Bytes(), &x); err != nil {
			t.Fatalf("line %d: %v", line, err)
		}
		switch x.Op {
		case "create":
			v = NewVolume(Spec{ID: "v", MinISR: x.MinISR}, x.Placement)
			continue
		case "attach":
			v.Attach(x.Node)
		case "change_isr":
			changes := make([]IsrChange, len(x.Changes))
			named := make([]Chunk, len(x.Changes))
			for i, c := range x.Changes {
				changes[i] = IsrChange{Chunk: c.Chunk, ExpectedVer: c.ExpectedVer, NewISR: c.NewISR}
				named[i] = v.Chunks[c.Chunk]
			}
			ok := CheckIsrChange(&v.Header, named, x.Epoch, changes)
			if ok != x.Applied {
				t.Fatalf("line %d: applied %v, Rust model says %v", line, ok, x.Applied)
			}
			if ok {
				ApplyIsrChange(named, changes)
				for i, c := range changes {
					v.Chunks[c.Chunk] = named[i]
				}
			}
		}
		if x.Applied {
			applied++
		} else {
			rejected++
		}
		if v.Epoch != x.After.Epoch || v.Target != x.After.Target {
			t.Fatalf("line %d: epoch %d target %q, want %d %q", line, v.Epoch, v.Target, x.After.Epoch, x.After.Target)
		}
		for i, c := range x.After.Chunks {
			got := v.Chunks[i]
			if !slices.Equal(got.ISR, c.ISR) || got.ISRVer != c.ISRVer || !slices.Equal(got.Replicas, c.Replicas) {
				t.Fatalf("line %d chunk %d: %+v, want %+v", line, i, got, c)
			}
		}
	}
	if err := sc.Err(); err != nil {
		t.Fatal(err)
	}
	if applied < 1000 || rejected < 500 {
		t.Fatalf("too few vectors: %d applied, %d rejected", applied, rejected)
	}
	t.Logf("%d transitions match the Rust model (%d applied, %d rejected)", applied+rejected, applied, rejected)
}

func TestSpecValidate(t *testing.T) {
	good := Spec{ID: "pvc-1", SizeBytes: 10 << 30, ChunkBytes: 256 << 20, Replicas: 3, MinISR: 2}
	if err := good.Validate(); err != nil {
		t.Fatal(err)
	}
	for name, mutate := range map[string]func(*Spec){
		"bad id":          func(s *Spec) { s.ID = "../etc" },
		"chunk not pow2":  func(s *Spec) { s.ChunkBytes = 3 << 20 },
		"chunk too small": func(s *Spec) { s.ChunkBytes = 4096 },
		"size unaligned":  func(s *Spec) { s.SizeBytes = 1000 },
		"no replicas":     func(s *Spec) { s.Replicas = 0 },
		"six replicas":    func(s *Spec) { s.Replicas = 6 },
		"min above rf":    func(s *Spec) { s.MinISR = 4 },
		"min zero":        func(s *Spec) { s.MinISR = 0 },
	} {
		s := good
		mutate(&s)
		if s.Validate() == nil {
			t.Errorf("%s: accepted", name)
		}
	}
}

func TestPlaceSpreadsLoad(t *testing.T) {
	nodes := []NodeLoad{{"a", 0}, {"b", 0}, {"c", 5}, {"d", 0}}
	p, err := Place(nodes, 6, 2)
	if err != nil {
		t.Fatal(err)
	}
	count := map[string]int{}
	for _, c := range p {
		if c[0] == c[1] {
			t.Fatalf("chunk on one node twice: %v", c)
		}
		for _, n := range c {
			count[n]++
		}
	}
	// 12 replicas: a, b and d take 4 each before the loaded c gets any.
	if count["a"] != 4 || count["b"] != 4 || count["d"] != 4 || count["c"] != 0 {
		t.Fatalf("unbalanced: %v", count)
	}
	if _, err := Place(nodes[:1], 1, 2); err == nil {
		t.Fatal("placed 2 replicas on 1 node")
	}
}
