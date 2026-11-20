# #75 round 10 (design-code, design-security)
R9 Nit fixed: main.go:77-79 prints to os.Stderr and exits 1; metactl (lab.sh:529-535) runs plain ssh (lib.sh:145, no -t), so remote stderr reaches local stderr and `2>&1` on the metactl call captures it. No existing wrapper is the right place: meta_try (lab.sh:1654) captures stderr in META_ERR but targets VM_A only, while the trap tries each cluster instance. The changed sentence is right.
No defects.
