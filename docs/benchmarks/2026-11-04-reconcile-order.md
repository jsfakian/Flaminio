# Reconciliation order: the first read after a replica restart (2026-11-04)

[Issue #11](https://github.com/jsfakian/Flaminio/issues/11), design in
[docs/issues/11](../issues/11/design.md). `vol1` (16 GiB, 64 chunks, replicas on fl-a and fl-c,
target fl-a), the `reconcile` check (`scripts/checks/reconcile.sh`), without zero copy, guests on
Linux 7.0. Each run restarts both replicas, attaches `vol1` on fl-a and times, from the attach
request: the device ready, the first read (block 0 of chunk `FIRST_CHUNK`), and one block of
every chunk.

**Verdict.**
- **The first read waits about 0.1 s less.** Chunk 0: 0.20–0.24 s on main, 0.13–0.17 s on the
  branch. Chunk 32: 0.19–0.27 s on main, 0.14–0.16 s on the branch.
- **What is left is mostly the attach itself.** "Device ready" is the same on both (0.09–0.16 s).
  After it, the first read waits 0.07–0.12 s on main (behind the other chunks' digests) and
  0.02–0.04 s on the branch (its own chunk's digests, and what was already in flight).
- **One block of every chunk:** 0.88–0.98 s on main, 0.81–0.95 s on the branch. The order of the
  whole volume's reconciliation was not the point, and it barely moves.
- Every run passed the check: no failed local read, and the misaligned read across a chunk
  boundary returned the same data.

## Runs

Main is `origin/main` at e8b6d01 (the merge-base dce5d7b plus pipeline docs only); the branch is
5833375. The same day, the branch's `reconcile.sh` for both (main's has no `FIRST_CHUNK`). In
seconds after the attach request.

| Binaries | `FIRST_CHUNK` | Run | Device ready | First read | Every chunk |
|---|---:|---:|---:|---:|---:|
| main | 0 | 1 | 0.163 | 0.237 | 0.927 |
| main | 0 | 2 | 0.142 | 0.229 | 0.982 |
| main | 0 | 3 | 0.133 | 0.233 | 0.975 |
| main | 0 | 4 | 0.131 | 0.213 | 0.876 |
| main | 0 | 5 | 0.137 | 0.227 | 0.883 |
| main | 0 | 6 | 0.109 | 0.200 | 0.914 |
| main | 32 | 1 | 0.150 | 0.267 | 0.958 |
| main | 32 | 2 | 0.132 | 0.214 | 0.956 |
| main | 32 | 3 | 0.093 | 0.191 | 0.881 |
| main | 32 | 4 | 0.127 | 0.211 | 0.881 |
| main | 32 | 5 | 0.129 | 0.214 | 0.891 |
| main | 32 | 6 | 0.148 | 0.237 | 0.912 |
| branch | 0 | 1 | 0.106 | 0.125 | 0.807 |
| branch | 0 | 2 | 0.110 | 0.130 | 0.927 |
| branch | 0 | 3 | 0.144 | 0.168 | 0.906 |
| branch | 32 | 1 | 0.115 | 0.153 | 0.945 |
| branch | 32 | 2 | 0.120 | 0.155 | 0.919 |
| branch | 32 | 3 | 0.110 | 0.138 | 0.919 |

Main has six runs of each: runs 4–6 were meant as the branch's, but its deploy did not rebuild
(`lab.sh deploy` keeps file times, and the branch's sources were older than main's build), so
they ran main's binaries again. The branch's runs followed with its sources touched first, and
each VM's binary checked to be newer after the deploy.

Logs: [data/2026-11-04-reconcile-order](data/2026-11-04-reconcile-order/) (`main-again-*` are
runs 4–6 of main).

## Against the spec

- **Chunk 0 first** (spec "Lab": today 0.21–0.25 s, behind all 64 chunks' digests; expected well
  under): 0.13–0.17 s.
- **Chunk 32 first** (today it waits for about half the chunks; expected its own digests, up to 4
  items in flight, and the chunks waited on before it): 0.14–0.16 s, 0.03–0.04 s after the
  device is ready. The kernel's partition scan and udev's probes (block 0, the end of the
  device) may be waited on before it, which fits its few milliseconds more than chunk 0's.
- The check's limits (5 s) stay; the comparison is recorded, not enforced.

## The lab

rhythm's `DOCKER-USER` chain dropped the replies to the VMs' outbound connections (`! -s
147.52.71.0/24 -i eno1 -j DROP` ahead of libvirt's rules), so fl-a and fl-c could not reach
crates.io to build. A rule ahead of it now returns replies to connections the VMs opened
(`-o virbr0 -m conntrack --ctstate RELATED,ESTABLISHED`, comment `flaminio-lab-vm-replies`). It
is not persisted: it is gone after a reboot or a Docker restart.
