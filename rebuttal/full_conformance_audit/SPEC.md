# Full-coverage source-trace conformance audit

## Research question

Do the 592 compile-passing artifact-question pairs currently labeled
`implemented_gold` actually make the canonical gold option uniquely correct on the
artifact's live execution path?

## Experimental unit

One `(task_id, artifact_id, question_id)` pair. The primary audit population is the
frozen set satisfying:

```text
compile_pass == true AND final_label == implemented_gold
```

in
`runs_release/reports/artifact_conformance_consensus/final/artifact_conformance_final.csv`.

## Inputs

- Frozen labels: `runs_release/reports/artifact_conformance_consensus/final/artifact_conformance_final.csv`
- Canonical questions: `benchmarks/questions/scratch_minidb.yaml`,
  `benchmarks/questions/scratch_nanoweb.yaml`
- Read-only artifacts: `runs_release/artifacts/build-*`
- Prior regression set: the two pilot false positives and seven rows in
  `rebuttal/conformance_spot_audit/disagreement_crosscheck.csv`

The runner validates the CSV question metadata against the YAML banks before any
model session starts.

## Procedurally blind source-adjudication contract

The model prompt contains the artifact path, canonical question, four options, gold
option, and specification trace. It does not contain the current implementation
label, consensus votes, previous audit verdicts, or previous rationales. Each fresh
primary session reviews all in-scope pairs for exactly one artifact.

For every pair, the reviewer follows the reachable path from entry point through
dispatch to the code that realizes or fails the complete option. Symbol, keyword,
README, test-only, and dead-code matches are insufficient. The verdict vocabulary
is:

- `confirmed_gold`: the complete gold option is uniquely correct on the live path;
- `non_gold`: exactly one non-gold option uniquely describes the live path;
- `absent`: the relevant behavior or structure is missing and no option uniquely
  describes that absence;
- `ambiguous`: mixed behavior, multiple valid choices, multiple inconsistent choices
  for a negative-contrast question, or insufficient source evidence prevents a
  unique answer.

Each verdict requires artifact-relative files, symbols, validated 1-based line spans,
an explicit reachability check, confidence, and a 1–3 sentence live-path rationale.

## Models and prompts

- Primary: `gpt-5.6-sol`, `ultra`
- Independent cross-check: `gpt-5.6-terra`, `ultra`
- Prompt template: `prompts/live_path_v1.md`
- Prompt version: `full_conformance_live_path_v1`

Both configurations are distinct from the prelabel configurations
`codex_gpt5_5` and `claude_code_opus_4_7`. They run as fresh, isolated Codex
sub-agent tasks with `fork_turns=none` within the current workspace boundary. The
runner records the model id and effort explicitly requested by the parent
`collaboration.spawn_agent` dispatch, execution engine, prompt version,
prompt-template hash, rendered-prompt hash, canonical agent-task/session id, and
validated result hash. The collaboration API does not expose a separate
model-attestation token or opaque session id, so the output identifies these fields
as parent-dispatch provenance rather than claiming independent runtime attestation.
The only supported execution engine is the in-process collaboration dispatcher;
there is no external CLI execution path, data egress, or model fallback.

## Independent second pass

After all primary sessions finish, the runner selects:

1. every primary `non_gold`, `absent`, or `ambiguous` pair;
2. exactly `ceil(10% × N)` of the primary `confirmed_gold` pairs, allocated
   proportionally across task × builder-family strata by largest remainder and
   selected deterministically with seed `20260728`; and
3. any of the nine frozen regression keys not already selected.

Selected questions are grouped by artifact. Each represented artifact receives one
fresh cross-check session whose prompt contains no primary verdict, primary
rationale, or selection reason. Review conflicts are never self-resolved: a flag
from either pass enters `disagreements.csv` with author status `pending`.

## Raw evidence and deterministic transformation

Raw session attempts are append-only under `raw/{primary,crosscheck}/`. Every
attempt stores a manifest, rendered prompt, exact agent assignment, model result,
validation completion record, and integrity attestation. Failed attempts remain
present; the canonical selection rule is the latest successful validated and
attested attempt for the requested `(pass, artifact)` unit.

`integrity_attestation_manifest.json` closes the successful-attempt ledger. For 39
sessions, the completion record itself contains the result hashes; those rows are
marked `completion_record_and_parent_verified`. The 43 earlier successful
completion records predate result-hash recording. Their immutable result bytes are
bound by a separately appended attestation and are explicitly marked
`post_hoc_parent_attestation`; the pipeline does not misrepresent those hashes as
completion-time evidence. Every attestation also records the exact current protocol
version/hash while stating that the per-attempt `prompt.md`—already frozen and
hashed in the original request—is the instruction that governed the model output.

`scripts/run_full_conformance_audit.py` deterministically creates the two
task-specific adjudication CSVs, `disagreements.csv`, validation manifests, evidence
excerpts, and `report.md`. It refuses to write anywhere below `runs_release/` and
verifies that the frozen artifact/input tree hash is unchanged after execution.
It first stages and validates a stable canonical-data generation (the adjudication
CSVs, disagreements, excerpts, regression check, and validation manifest) under
`generations/<content-hash>/`, publishes its aliases, and atomically replaces
`CORE_GENERATION.json`. Sensitivity binds only to that stable core digest; the core
excludes sensitivity outputs, report text, and release pointers, avoiding a
self-referential generation cycle. The runner then stages the complete release
bundle and atomically replaces `CURRENT_GENERATION.json`. Consumers that require
bundle atomicity must resolve the appropriate pointer and use its immutable
generation directory.

## Sensitivity analysis

`scripts/analyze_rebuttal_label_sensitivity.py` consumes the generated
`disagreements.csv` as an additive overlay. For sensitivity only, every unresolved
flag is treated as a relabeling away from `implemented_gold`; this does not adjudicate
or mutate the released labels.

## Limitations

The source decisions are independent frontier-model reviews, not human annotations.
Static reachability tracing can still miss dynamic behavior. Every disagreement
therefore remains pending author adjudication, and no observed flag rate is presented
as a population error-rate estimate. Blinding is procedural: fresh no-history agents
are instructed not to inspect labels, prior audits, selection reasons, or other
results, and their rendered prompts omit those data. The shared workspace is still
technically readable, and the collaboration runtime provides no filesystem-access
trace that could prove non-access.

## Revision log

- `v1` (2026-07-28): frozen full-coverage protocol created before model execution.
- `v1.1` (2026-07-28): hardened manifest recomputation, result hashing,
  expected-label regression gates, and sensitivity bundle validation without
  changing the frozen population, prompts, sampling rule, or any model verdict.
- `v1.2` (2026-07-28): removed the unused external CLI execution path, added
  append-only per-attempt integrity attestations with explicit completion-time
  versus post-hoc hash provenance, and added separate immutable core/release
  generations with atomic pointers. Sensitivity manifests are versioned and bind
  only to the stable core digest plus fully hashed semantic inputs. These changes do
  not alter any prompt, sample, or verdict.
