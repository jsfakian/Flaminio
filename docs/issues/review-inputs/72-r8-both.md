# #72 round 8 (incremental, design-code and design-security): diff 4eadb5f..eea3f2c

R7-1: fixed. design.md S6 row (TestInitCapacity) now has the non-`all` call with an untouched readable node pending on a legacy tombstone: empty blocked_by, marked false, ledger unchanged. Agrees with contracts :93 and :138 and spec S6.
R7-N1: fixed in substance (growth bullet, design.md:85-89 now states k shrinks to the longest fitting prefix).

Defects in the changed sentences:
- Nit: "fit 120 operations" is looser than #70's Charges, which bounds 120 ops (1+k+D+#73 writes) and 120 compares (2+D+#73 compares) separately, and says one chunk always fits. Since #72 may land second and build this without opening #70, say "120 compares and 120 writes" (and "at least one chunk"). Only ops depend on k; compares depend on D, so a reader may think only the sum matters.

No Critical/High/Medium/Low findings.
