Reviewed b2b76b7e28215afc331cd6c5a07ee0350027a010 incrementally from 513b973. Reviewer: /root/remaining_plan_reviews/review56_r2_security.

R1-S1/S2 fixed: unsupported VM recovery/admission blocks; legacy exploration suppresses unsafe failover. R1-S3 separate coordinator stream has explicit ambiguity/deadlines; R1-S4 defines strict bounded recovery records.

- R2-S1 — Medium: lab-contracts.md:43–51 requires helper invocation identity before termination; :184–185 requires helper cleanup before controller restoration. Ledger :153–182 contains only pair, power and controller_hold. Persist a typed helper action before startup with endpoint/node/run identity and verified systemd invocation ownership. Define intent-only crash recovery and conditional termination. Fixtures: crash around startup/identity recording; preserve foreign or replaced invocation.

No Nits. Dependencies: #82 18fb58576f2d5a8cbf31463619c652fbf475a5bc; #83 67862ad667ac3121b453c9227cef1e8ec5248138. No edits or lab.
