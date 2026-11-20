# Issue #16 plan review, round 9 (incremental), design-code and design-security
Reviewed: ba823dc..46fec33.

## Round-8 findings
- R8-1: fixed. Design Tests now says the drain only delays the close: EOF, any read error (connection reset included) or the timeout ends it, none asserted; assertions stay on accept's error and the connector's text. decisions.md records it.

## New findings
None. The change is wording only; no rule changed.
