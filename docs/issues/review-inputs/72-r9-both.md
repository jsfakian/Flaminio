# #72 round 9 (incremental, design-code and design-security): growth-bullet clause

R8-N1: fixed. The clause now reads "longest prefix that fits 120 compares and 120 writes, at least one chunk". It agrees with #70 design.md:86-89 (120 operations and 120 compares bounded separately; k shrinks to the longest fitting prefix; one chunk always fits).

Defects: none at Low or worse.
Nit: the clause says "writes" where #70 says "operations" (the same bound, kv.go:33 "120 ops"). Use "operations" if you want the two documents to match word for word.
