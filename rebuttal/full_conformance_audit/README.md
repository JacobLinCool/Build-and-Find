# Full-coverage conformance audit

This directory holds the post-submission source-trace audit of the 592 scored
compile-pass artifact-question pairs. `SPEC.md` describes the protocol and
`report.md` summarizes the audit run. The adjudicated-label paragraphs and table
at the end of the paper's Appendix H, and the evidence-audit overlap sentence in
Appendix I, come from `label_sensitivity_adjudicated/`, which
`make label-sensitivity` regenerates from the files below.

## Files

- `SPEC.md`, `prompts/live_path_v1.md`, and `result_schema.json`: the audit
  protocol (v1.2), the prompt template, and the JSON schema of a session result.
  The core generation manifest records the hash of `SPEC.md`, and
  `execution_manifest.json` the hashes of the prompt template and the schema.
  Because these hashes are recorded, `SPEC.md` keeps its original wording
  ("independent cross-check", "isolated" sessions). The blinding between the two
  passes was procedural: each prompt received only the inputs listed in
  `report.md`.
- `report.md`: summary of the audit run (see "Edits after generation").
- `minidb_adjudication.csv`, `nanoweb_adjudication.csv`: one row per audited
  pair with the verdicts of both passes, evidence files, symbols and line spans,
  rationales, and model and prompt provenance, including the hashes of each
  session result and its integrity attestation.
- `disagreements.csv`: the 205 pairs flagged by either pass.
- `evidence_excerpts.jsonl`: line-numbered text of every cited span, with the
  sha256 of its source file.
- `population_manifest.csv`, `artifact_manifest.csv`: the audited pairs and
  artifacts, with artifact tree hashes.
- `crosscheck_manifest.csv`, `crosscheck_sampling_manifest.json`: the pairs
  selected for the cross-check pass and the selection rule.
- `regression_check.csv`: the nine known label errors and whether each pass
  reproduced them.
- `execution_manifest.json`, `validation_manifest.json`, `output_manifest.json`:
  inputs and configuration, validation counts, and output hashes.
- `integrity_attestation_manifest.json`: the closed ledger of the 82 successful
  session attempts. For each attempt it gives the repository-relative path of
  the raw records (not released), the hashes of the integrity attestation and of
  the selected result, and whether the result hash was recorded at completion
  time or attested afterwards. The core generation manifest records its hash.
- `CORE_GENERATION.json` and
  `generations/345e9f073af7b83f6ed35a069afe4c1dc78f9aac5c74e40b09c34b33cc428bfd/generation_manifest.json`:
  the core-generation pointer and manifest. `make label-sensitivity` checks the
  root CSVs against the hashes in this manifest before it runs.
- `author_decisions.csv`, `model_assisted_decisions.csv`: decisions on all 205
  flags (below).
- `label_sensitivity_adjudicated/`: the sensitivity outputs that the paper uses
  (below).

The public release does not include the raw per-session audit records (`raw/`),
the agent job lists, the other generation bundles and pointers, the July
sensitivity outputs (`label_sensitivity/`), the author's adjudication worksheet,
or the review report of the model-assisted decisions; these stay in the authors'
records. The adjudication tables and manifests above record the hashes of the
raw session results and attestations, the current core and release generations,
and the July outputs.

## Rerunning the audit

`scripts/run_full_conformance_audit.py` writes one assignment per artifact and
pass (`--phase agent-primary-prepare` and `--phase agent-crosscheck-prepare`)
without launching a model. The sessions were run from these assignments by an
in-process agent dispatcher (`--engine collaboration`) that is not part of this
repository, so they cannot be rerun from the release. The script then validated
and attested the returned results and wrote the outputs (`--phase finalize`);
these phases read the raw session records, which are not released. Everything
downstream of the audit tables reruns with `make label-sensitivity`.

## `author_decisions.csv`

One row per author decision on a full-audit flag: 43 rows, covering every flag on
the `codex_gpt5_4_mini_low` (25) and `codex_gpt5_5_low` (18) artifacts. The rows
are transcribed from the `AUTHOR DECISION` entries of the author's adjudication
worksheet, which is kept in the authors' records and is not part of the public
release.

Columns:

- `task_id`, `artifact_id`, `question_id`: the flagged pair.
- `flag_basis`: the audit pass that flagged the pair (`both_passes`,
  `primary_only`, or `crosscheck_only`); it must match `disagreements.csv`.
- `author_decision`: `absent_or_ambiguous`, `non_gold`, or
  `keep_implemented_gold`.
- `decided_by`: `author`.
- `decided_on`: the decision date (see below).
- `worksheet_item`: the item number in the worksheet.
- `rationale`: `not recorded at decision time` on every row. The worksheet records
  each decision without a reason, and no reason was added afterwards.

The worksheet records decisions but no dates, so `decided_on` is inferred rather
than recorded. All 43 rows carry `2026-07-31`, the modification date of the
worksheet file. The adjudicated copy of the disagreement table was written the
same day, and the OpenReview comments that report the 43 decisions (41
confirmed, 2 kept) are dated 31 Jul 2026. `author_decisions.csv` itself was
transcribed from the worksheet on 2026-10-10.

## `model_assisted_decisions.csv`

One row per flag without an author decision: 162 rows, so that together with
`author_decisions.csv` every one of the 205 flags has a decision. The decisions
were produced by model-assisted adjudication (Claude): two passes per flag, the
second without access to the audit verdicts, and an arbitration pass where the
two disagreed. The author reviewed a report of these decisions and approved them
on 2026-10-16, so every row has `review_status` `author_approved` and
`reviewed_on` `2026-10-16`. `decided_by` still records them as model-assisted
adjudication. Rationales that follow an author decision as a precedent cite it as
`worksheet #N`, the `worksheet_item` of that row in `author_decisions.csv`.

Columns: `task_id`, `artifact_id`, `question_id`, `flag_basis` (as in
`author_decisions.csv`), `decision` (same vocabulary as `author_decision`),
`actual_option` (`A`–`D`, `none`, or `multiple`), `evidence` (artifact
`file:line` ranges), `rationale`, `confidence` (`high`, `medium`, `low`; the lower
of the two passes when they agreed), `pass_agreement` (`agree` or `arbitrated`),
`decided_by`, `review_status` (`pending_author_review` or `author_approved`),
`decided_on`, and `reviewed_on` (the review date; empty while the review is
pending).

## `label_sensitivity_adjudicated/`

Written by `make label-sensitivity`, which runs
`scripts/analyze_rebuttal_label_sensitivity.py` with `--author-decisions`,
`--model-assisted-decisions`, and `--label-corrections`. Its baseline scenario
uses the released labels with
`runs_release/reports/artifact_conformance_consensus/final/label_corrections_v1_1.csv`
applied, the same labels as the paper analysis. The `fully_adjudicated` scenario
adds the model-assisted relabels to the author-adjudicated set; the other
scenarios do not read `model_assisted_decisions.csv` and are unchanged by it.
`evidence_audit_overlap.csv` counts, for each scenario, the answers of the
evidence-citation audit sample (`runs_release/reports/evidence_citation_audit_sample.csv`)
that fall on relabeled pairs. `manifest.json` records the command, the script
hash, and the hashes of every input and output.

## July sensitivity outputs (`label_sensitivity/`)

The July finalize step of `scripts/run_full_conformance_audit.py` also wrote
`label_sensitivity/`, without author decisions and with the released v1.0 labels
as baseline. `output_manifest.json` records the hashes of these files, which are
aliases of the copies in generation `74c8e301…`, so they are kept as generated.
Their `manifest.json` records the hash of the analysis script at that time
(`4526b263…`). The script has since gained `--author-decisions` and
`--label-corrections`, so that hash no longer matches the current script. The
paper does not use these outputs, and they are not part of the public release.

## Line endings in recorded hashes

Input hashes in manifests written before 2026-10-10 were computed on CRLF
working copies of the input CSVs. In this directory, this applies to the `labels`
input (`artifact_conformance_final.csv`) of `execution_manifest.json`; the July
sensitivity manifests record CRLF hashes of `artifact_conformance_final.csv`,
`find_runs.csv` and `question_answers.csv`. To verify these hashes, convert the
inputs to CRLF first, for example `perl -pe 's/\n/\r\n/' FILE | shasum -a 256`.
The hash that the July manifests record for
`impact/conditioned_builder_scores.csv` matches the CRLF form of its version at
commit `f831273`; the file was regenerated on 2026-10-10, which changed only its
`ci95_low` and `ci95_high` columns. Manifests written since 2026-10-10 hash the
LF bytes stored in git.

## Edits after generation

`report.md` was revised on 2026-10-16: its wording now matches the paper's
description of the two model passes and their procedural blinding, and it marks
the files that are not part of the public release. Its numbers are unchanged.
`output_manifest.json` records the hash of the generated version
(`89f95d9c…`), so the current file no longer matches that entry.
