# Plan review #43, round 11 (13c1e90..f23ee17), design-code + design-security

Round-10 findings: L1 fixed (design Contracts, decisions and TestBlockedTable all say the text, `...` included, is at most 512 bytes, cut on a rune boundary; test asserts valid UTF-8). L2 fixed (step 4: that object gets no dry run, others go on; agrees with step 7, table, TestAdoptionRefused). Nit "naming each" fixed (step 7, spec). Nit `spec` entry fixed (step 9, table, decisions). Nit StorageClass "never changed or deleted" fixed (design adopted-objects, decisions, S7 test; agrees with spec l.82 and step 5).

No new defects; changed sentences agree with the others on each subject.
