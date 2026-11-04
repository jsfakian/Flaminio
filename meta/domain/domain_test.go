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
	Moving   []string `json:"moving"`
}

type vector struct {
	Op         string     `json:"op"`
	FirstEpoch uint64     `json:"first_epoch"`
	MinISR     uint32     `json:"min_isr"`
	Placement  [][]string `json:"placement"`
	Node       string     `json:"node"`
	Epoch      uint64     `json:"epoch"`
	Chunk      uint32     `json:"chunk"`
	From       string     `json:"from"`
	To         string     `json:"to"`
	Changes    []struct {
		Chunk       uint32   `json:"chunk"`
		ExpectedVer uint64   `json:"expected_ver"`
		NewISR      []string `json:"new_isr"`
		NewReplicas []string `json:"new_replicas"`
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
			v.Start(x.FirstEpoch)
			continue
		case "attach":
			v.Attach(x.Node)
		case "move":
			ok := CheckMove(&v.Chunks[x.Chunk], x.From, x.To)
			if ok != x.Applied {
				t.Fatalf("line %d: move applied %v, Rust model says %v", line, ok, x.Applied)
			}
			if ok {
				ApplyMove(&v.Chunks[x.Chunk], x.From, x.To)
			}
		case "change_isr":
			changes := make([]IsrChange, len(x.Changes))
			named := make([]Chunk, len(x.Changes))
			for i, c := range x.Changes {
				changes[i] = IsrChange{Chunk: c.Chunk, ExpectedVer: c.ExpectedVer, NewISR: c.NewISR,
					ChangeReplicas: c.NewReplicas != nil, NewReplicas: c.NewReplicas}
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
			var moving []string
			if got.Moving != nil {
				moving = []string{got.Moving.From, got.Moving.To}
			}
			if !slices.Equal(got.ISR, c.ISR) || got.ISRVer != c.ISRVer || !slices.Equal(got.Replicas, c.Replicas) ||
				!slices.Equal(moving, c.Moving) {
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
	nodes := []NodeLoad{{ID: "a"}, {ID: "b"}, {ID: "c", Replicas: 5}, {ID: "d"}}
	p, err := Place(nodes, 6, 2, "", DomainNode, PolicyStrict)
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
	if _, err := Place(nodes[:1], 1, 2, "", DomainNode, PolicyStrict); err == nil {
		t.Fatal("placed 2 replicas on 1 node")
	}
}

func TestPlacePreferredNode(t *testing.T) {
	nodes := []NodeLoad{{ID: "a", Replicas: 10}, {ID: "b"}, {ID: "c"}}
	p, err := Place(nodes, 4, 2, "a", DomainNode, PolicyStrict)
	if err != nil {
		t.Fatal(err)
	}
	load := map[string]int{}
	for _, r := range p {
		if r[0] != "a" {
			t.Fatalf("chunk without a replica on the preferred node: %v", p)
		}
		load[r[1]]++
	}
	if load["b"] != 2 || load["c"] != 2 {
		t.Fatalf("the other replicas do not spread: %v", p)
	}
	// An ineligible preference (not among the nodes) is ignored.
	if p, err := Place(nodes, 1, 2, "w", DomainNode, PolicyStrict); err != nil || p[0][0] == "w" {
		t.Fatalf("%v %v", p, err)
	}
}

func TestPlaceStrictZoneRefusesShortTopology(t *testing.T) {
	// Two zones, three replicas: strict must refuse.
	nodes := []NodeLoad{
		{ID: "a1", Zone: "z1", Rack: "r1"},
		{ID: "a2", Zone: "z1", Rack: "r2"},
		{ID: "b1", Zone: "z2", Rack: "r3"},
		{ID: "b2", Zone: "z2", Rack: "r4"},
	}
	if _, err := Place(nodes, 1, 3, "", DomainZone, PolicyStrict); err == nil {
		t.Fatal("strict zone placement accepted a 2-zone topology for 3 replicas")
	}
}

func TestPlaceBestEffortFallsBack(t *testing.T) {
	// One zone, two racks: zone+bestEffort falls back to rack.
	nodes := []NodeLoad{
		{ID: "a", Zone: "z", Rack: "r1"},
		{ID: "b", Zone: "z", Rack: "r2"},
		{ID: "c", Zone: "z", Rack: "r1"},
	}
	p, err := Place(nodes, 4, 2, "", DomainZone, PolicyBestEffort)
	if err != nil {
		t.Fatal(err)
	}
	for _, c := range p {
		ra, rb := rackOf(nodes, c[0]), rackOf(nodes, c[1])
		if ra == rb {
			t.Fatalf("replicas share a rack after zone→rack fallback: %v", c)
		}
	}
	// One rack only: falls back further to node.
	sameRack := []NodeLoad{
		{ID: "a", Zone: "z", Rack: "r"},
		{ID: "b", Zone: "z", Rack: "r"},
	}
	p, err = Place(sameRack, 2, 2, "", DomainZone, PolicyBestEffort)
	if err != nil {
		t.Fatal(err)
	}
	for _, c := range p {
		if c[0] == c[1] {
			t.Fatalf("replicas share a node: %v", c)
		}
	}
}

func TestPlaceNoSharedDomain(t *testing.T) {
	nodes := []NodeLoad{
		{ID: "a1", Zone: "z1", Rack: "r1"},
		{ID: "a2", Zone: "z1", Rack: "r2"},
		{ID: "b1", Zone: "z2", Rack: "r3"},
		{ID: "b2", Zone: "z2", Rack: "r4"},
		{ID: "c1", Zone: "z3", Rack: "r5"},
		{ID: "c2", Zone: "z3", Rack: "r6"},
	}
	for _, level := range []FailureDomain{DomainZone, DomainRack, DomainNode} {
		p, err := Place(nodes, 12, 3, "", level, PolicyStrict)
		if err != nil {
			t.Fatalf("%s: %v", level, err)
		}
		for _, c := range p {
			seen := map[string]bool{}
			for _, id := range c {
				n := nodes[indexOf(nodes, id)]
				d := n.domainKey(level)
				if seen[d] {
					t.Fatalf("%s: shared domain in %v", level, c)
				}
				seen[d] = true
			}
		}
	}
}

func TestPlaceDomainSpreadsLoad(t *testing.T) {
	// Three zones, two nodes each; 2 replicas at zone: load should spread across zones and
	// within a zone across its nodes.
	nodes := []NodeLoad{
		{ID: "a1", Zone: "z1"}, {ID: "a2", Zone: "z1"},
		{ID: "b1", Zone: "z2"}, {ID: "b2", Zone: "z2"},
		{ID: "c1", Zone: "z3"}, {ID: "c2", Zone: "z3"},
	}
	p, err := Place(nodes, 6, 2, "", DomainZone, PolicyStrict)
	if err != nil {
		t.Fatal(err)
	}
	count := map[string]int{}
	zoneCount := map[string]int{}
	for _, c := range p {
		za, zb := zoneOf(nodes, c[0]), zoneOf(nodes, c[1])
		if za == zb {
			t.Fatalf("replicas share a zone: %v", c)
		}
		for _, id := range c {
			count[id]++
			zoneCount[zoneOf(nodes, id)]++
		}
	}
	for _, z := range []string{"z1", "z2", "z3"} {
		if zoneCount[z] != 4 {
			t.Fatalf("zone load %v (want 4 each)", zoneCount)
		}
	}
	for _, id := range []string{"a1", "a2", "b1", "b2", "c1", "c2"} {
		if count[id] != 2 {
			t.Fatalf("node load %v (want 2 each)", count)
		}
	}
}

func TestPlacePreferredRespectsDomain(t *testing.T) {
	nodes := []NodeLoad{
		{ID: "a", Zone: "z1", Replicas: 10},
		{ID: "b", Zone: "z2"},
		{ID: "c", Zone: "z3"},
	}
	p, err := Place(nodes, 3, 2, "a", DomainZone, PolicyStrict)
	if err != nil {
		t.Fatal(err)
	}
	for _, c := range p {
		if c[0] != "a" {
			t.Fatalf("preferred not first: %v", c)
		}
		if zoneOf(nodes, c[1]) == "z1" {
			t.Fatalf("second replica in preferred zone: %v", c)
		}
	}
}

func rackOf(nodes []NodeLoad, id string) string { return nodes[indexOf(nodes, id)].Rack }
func zoneOf(nodes []NodeLoad, id string) string { return nodes[indexOf(nodes, id)].Zone }
func indexOf(nodes []NodeLoad, id string) int {
	for i, n := range nodes {
		if n.ID == id {
			return i
		}
	}
	panic(id)
}
