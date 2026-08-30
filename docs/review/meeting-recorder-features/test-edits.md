# Test recipe edit ledger

Every edit to an **existing** test assertion on this branch is recorded here,
with the adjudicator verdict that authorised it.

## Why this file exists

Test recipes on this branch are written **before** the implementation and are
then **frozen**. An implementation that fails a test is presumed wrong. When a
test genuinely looks incorrect, the process is:

1. Stop. Do not edit the test.
2. Spawn an **independent adjudicator subagent** with fresh context.
3. Give it the test source, the requirement it encodes (quoted from the plan),
   the failing output, and the implementation diff. The implementer's argument
   is included but labelled as a self-serving brief; the adjudicator's default
   answer is "the implementation is wrong."
4. Verdicts: `test_is_wrong` / `implementation_is_wrong` /
   `requirement_is_ambiguous`.
5. **Only `test_is_wrong` authorises an edit.** `requirement_is_ambiguous`
   escalates to the repository owner — it is never self-resolved.

Adding **new** tests is always allowed and is not recorded here. The freeze
covers weakening, deleting, loosening, or changing the meaning of an existing
assertion. `#[ignore]` counts as deleting.

**An empty table below is the expected outcome.** A long one is evidence the
tests were written to fit the code rather than the requirement.

## Edits

| # | Date | Test (file::test_name) | Stage | Verdict | Adjudicator's reasoning | Change made |
|---|------|------------------------|-------|---------|-------------------------|-------------|
| _(none yet)_ | | | | | | |
