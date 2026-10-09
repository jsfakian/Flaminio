package agent

import (
	"regexp"
	"strings"
	"testing"
)

type fakeProber struct {
	sig     map[string]string
	ours    map[string]bool
	dirty   map[string]bool
	holders map[string][]string
}

func (f fakeProber) Signature(p string) (string, error)     { return f.sig[p], nil }
func (f fakeProber) Ours(p string) bool                     { return f.ours[p] }
func (f fakeProber) Blank(p string, _ uint64) (bool, error) { return !f.dirty[p], nil }
func (f fakeProber) Holders(name string) ([]string, error)  { return f.holders[name], nil }

// A lab VM, plus one device of each kind the policy must refuse.
const lsblk = `{"blockdevices": [
 {"name":"sr0","path":"/dev/sr0","type":"rom","size":374784,"ro":true,"rm":true,"mountpoints":[],"serial":"QM00001","fstype":"iso9660"},
 {"name":"vda","path":"/dev/vda","type":"disk","size":42949672960,"ro":false,"rm":false,"mountpoints":[],"pttype":"gpt",
  "children":[{"name":"vda1","path":"/dev/vda1","type":"part","size":41759522304,"mountpoints":["/"],"fstype":"ext4"}]},
 {"name":"vdb","path":"/dev/vdb","type":"disk","size":107374182400,"ro":false,"rm":false,"mountpoints":[],"serial":"flaminio-data"},
 {"name":"vdc","path":"/dev/vdc","type":"disk","size":107374182400,"ro":false,"rm":false,"mountpoints":[],"serial":"spare-1"},
 {"name":"vdd","path":"/dev/vdd","type":"disk","size":107374182400,"ro":false,"rm":false,"mountpoints":[null],"serial":"spare-2"},
 {"name":"vde","path":"/dev/vde","type":"disk","size":107374182400,"ro":false,"rm":false,"mountpoints":[],"serial":"spare-3"},
 {"name":"vdf","path":"/dev/vdf","type":"disk","size":107374182400,"ro":false,"rm":false,"mountpoints":[],"serial":"spare-4"},
 {"name":"vdg","path":"/dev/vdg","type":"disk","size":1048576,"ro":false,"rm":false,"mountpoints":[],"serial":"tiny"},
 {"name":"vdh","path":"/dev/vdh","type":"disk","size":107374182400,"ro":true,"rm":false,"mountpoints":[],"serial":"ro"},
 {"name":"loop0","path":"/dev/loop0","type":"loop","size":107374182400,"ro":false,"rm":false,"mountpoints":[]},
 {"name":"ublkb1","path":"/dev/ublkb1","type":"disk","size":17179869184,"ro":false,"rm":false,"mountpoints":[]}
]}`

func TestClaimPolicy(t *testing.T) {
	devs, err := ParseLsblk([]byte(lsblk))
	if err != nil {
		t.Fatal(err)
	}
	pr := fakeProber{
		sig:     map[string]string{"/dev/vdc": "TYPE=LVM2_member USAGE=raid"},
		dirty:   map[string]bool{"/dev/vdd": true},
		holders: map[string][]string{"vde": {"dm-0"}},
		ours:    map[string]bool{},
	}
	got := map[string]string{}
	for _, d := range Decide(devs, Policy{MinSize: 1 << 30}, pr) {
		got[d.Name] = string(d.State) + ": " + d.Reason
	}
	want := map[string]string{
		"sr0":    "refused: not a whole disk",
		"vda":    "refused: has partitions",
		"vdb":    "claimable: blank",
		"vdc":    "refused: signature: TYPE=LVM2_member",
		"vdd":    "refused: not blank",
		"vde":    "refused: in use by dm-0",
		"vdf":    "claimable: blank",
		"vdg":    "refused: smaller than",
		"vdh":    "refused: read-only",
		"loop0":  "refused: not a whole disk",
		"ublkb1": "refused: a virtual device",
	}
	for name, w := range want {
		if !strings.HasPrefix(got[name], w) {
			t.Errorf("%s: %q, want %q…", name, got[name], w)
		}
	}
	// Two blank disks: ambiguous without a selector; the lab's selector picks vdb.
	if _, err := Pick(Decide(devs, Policy{MinSize: 1 << 30}, pr)); err == nil {
		t.Fatal("two claimable devices: no error")
	}
	d, err := Pick(Decide(devs, Policy{MinSize: 1 << 30, Serial: regexp.MustCompile("data")}, pr))
	if err != nil || d.Path != "/dev/vdb" || d.State != Claimable {
		t.Fatalf("pick: %+v %v", d, err)
	}
	// A Flaminio disk is used as it is, and wins over blank ones.
	pr.ours["/dev/vdf"] = true
	d, err = Pick(Decide(devs, Policy{MinSize: 1 << 30}, pr))
	if err != nil || d.Path != "/dev/vdf" || d.State != Ours {
		t.Fatalf("pick with a Flaminio disk: %+v %v", d, err)
	}
	// The lsblk signature columns count too, whatever blkid says.
	devs[2].FSType = "xfs"
	for _, d := range Decide(devs, Policy{MinSize: 1 << 30}, pr) {
		if d.Name == "vdb" && d.State != Refused {
			t.Fatalf("vdb with xfs: %+v", d)
		}
	}
}
