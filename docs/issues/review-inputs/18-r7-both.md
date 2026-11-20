# #18 plan review, round 7 (design-code, design-security)
Reviewed commit: 324be273b5db567ab178b313e15f1bb1083a0ef9 (the fix is d333ca8; 324be27 only adds the report).
R6-B1: fixed. "At least one of the three is outside the chunk and is such a node" is true.
R6-B2: fixed. The refusal text built by the service (`%w: volume %q chunk %d: %s` with Strict's reason) starts with the ErrCollision prefix. In the forced step the cleared replica is the only staying one, so Strict's first rule gives `replica "<id>" has no <level> label`. That names the cleared replica by its node id (replica id and node id are the same in the design), and FailedPrecondition comes from errStatus. The text is true.
Opens nothing. No defects, no Nits.
