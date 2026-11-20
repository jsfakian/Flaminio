# #13 plan review, round 9 (incremental), design-code and design-security
Reviewed: git diff 0a89e15..6129791 (docs/issues/13/, report.md excluded).

R8-N1: fixed. Step 2 now reads "comparing the mark's revision (0 if absent), drain/<node>'s (0 if absent) and, with #78, the gate's", and the lone "drain/<node> is" line is gone.

Meaning unchanged: the Txn compares the mark, drain/<node> and (with #78) the gate; it puts the mark, and writes drain/<node> only when it was absent; an existing drain marker and its generation are left alone; a failed compare retries. The decisions.md entry matches the edit and says "wording only".

New findings: none.
