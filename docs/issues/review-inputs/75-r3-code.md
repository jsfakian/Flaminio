# Plan review #75, round 3, design-code

Round-2 findings (75-r2-code.md): C1 (head above cap), C2 (refusal after Attach), C3 (covers), C4 (deposition), C5 (S4 bound), C6 (kernel re-arm) are all fixed in design.md and spec.md; checked against the code (Options auth.go:19-28, serve flags main.go:84-101, drain_one deposition branch engine.rs:1841-1853, controller.go:245/265, meta.sh:28 ExecStart). The wait-bound formula holds (io_buf default 512 KiB, max 1 MiB, node.rs:102,222; 128 s at the floors).

New defects from the revision:

1. Medium - `a_request_across_chunks_is_charged_once` cannot fail. design.md Tests S2: "burst 1: a charge of 2 is never admitted". With the new clamp (a cost above the burst is charged as the burst), a request wrongly charged twice at burst 1 is clamped to 1 and admitted exactly like a correct single charge. The test no longer separates once from twice. Fix: use burst >= 2 (e.g. IOPS burst 2, a request over two chunks; a second request must be held after one correct charge, and admitted only after one correct charge's refill).

2. Low - Two stated rules have no test. (a) The CSI controller "if the delete fails, its error" (design.md CSI bullet) and (b) "nothing is deleted on AlreadyExists" (the rule that protects a volume holding data). TestControllerQoS only checks the AlreadyExists code, not that the volume remains; TestControllerQoSMixedInstances checks "no volume left" only. Fix: add to the CSI tests an assertion that the fake's DeleteVolume is not called on AlreadyExists, and a case where the dropped-qos create's delete fails and that error is returned.

3. Low - The switch's "off" behaviour on reads is not pinned. Design says off refuses only `CreateVolume`; the GetVolume refusal to a non-enforcing `io` caller must stay unconditional (a limited volume exists after the switch is turned off again). No test serves a limited volume with the switch off. Fix: one case in TestGetVolumeQoSNeedsAnEnforcingDataPlane with the switch off and a stored limited volume.

4. Low - A non-CSI create path is unguarded while instances are mixed: `metactl create` with a policy sent to an older instance drops `qos` silently, and only the CSI controller compares. Design should say metactl performs the same readback (or that the operator must not use it before the switch is on), and S5 should test it if so.

5. Nit - adr-proposal.md Status says "Accepted" while the file is not in docs/adr/; AGENTS.md calls accepted ADRs binding. Say "Accepted with the plan, in the issue folder" is understood, or move it when implemented (it already says so); no change needed beyond making the binding status explicit.
