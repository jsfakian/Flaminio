# Plan review #43, round 9 (36670aa..a78216f), design-code + design-security

## Round-8 findings
- R8-1 fixed in text (step 9 order, message lists all found); test covers only drift + failed apply (see R9-1).
- R8-2 fixed: TestMidwayFailureResumes has a failed live read and a failed dry run.
- R8-3 fixed: step 4 says no dry run; TestAdoptionRefused asserts it.
- R8-4 fixed ("or none on a first pass").
- R8-5 fixed (S3 "for its object").

## Consistency of the changed lines
Step 9 order agrees with steps 2-8: InvalidInput only arises at step 2 and ends there; step 3/6/8 errors go to 9 with ApplyFailed; steps 4/6 refusals give PreflightRefused at 7; drift from 5. A failed step stops the pass, so at most one object has ApplyFailed, and "one per object" holds (a drifted or refused object is never dry-run or written). Step 4's "leaves the pass" agrees with 5-8.

## Findings
### R9-1 Low - precedence cases other than drift+ApplyFailed are untested
The rule is a three-way order. Only "drift, then another object's apply fails" is tested. Not tested: refusal plus drift (PreflightRefused wins, message still names the drifted fields: step 5 precedes step 7), and refusal found in step 4 or an earlier dry run, then a later dry run errors (ApplyFailed wins, refusal still listed). An implementation that keeps only the winning reason's objects passes these tests for refusals.
Fix: add to TestPreflightRefusalWritesNothing a case with a drifted DaemonSet (reason PreflightRefused, both objects in the message) and one with a refusal and a failing later dry run (ApplyFailed, both listed).

### R9-2 Low - Blocked message has no bound
The message now joins up to six objects, each with an API server error text. Condition messages have a size limit (32768); an over-long one makes the status patch fail every pass, so the status goes stale, and the plan says an error "changes nothing". Fix: state a per-object truncation (e.g. 512 chars) in Contracts and test it.

## Nits
- Contracts: "<the fields or the error>" does not cover a refusal's reason text (step 7); say "the fields, the refusal or the error".
- Step 4 applies to the StorageClass too, while step 5 says it is "never compared"; a controller-owned StorageClass blocks all writes. State that this is intended (or exempt it).
