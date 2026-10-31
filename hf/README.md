---
license:
- cc-by-4.0
- apache-2.0
pretty_name: Build-and-Find
language:
- en
- code
tags:
- coding-agents
- agent-evaluation
- benchmark
- software-engineering
- intent-recovery
- rust
task_categories:
- question-answering
- text-generation
task_ids:
- multiple-choice-qa
size_categories:
- 10K<n<100K
configs:
- config_name: find_runs
  default: true
  data_files:
  - split: test
    path: runs_release/tables/find_runs.csv
- config_name: question_answers
  data_files:
  - split: test
    path: runs_release/tables/question_answers.csv
- config_name: build_runs
  data_files:
  - split: test
    path: runs_release/tables/build_runs.csv
- config_name: panel_membership
  data_files:
  - split: test
    path: runs_release/tables/panel_membership.csv
- config_name: implementation_labels
  data_files:
  - split: test
    path: runs_release/reports/artifact_conformance_consensus/final/artifact_conformance_final.csv
- config_name: label_corrections_v1_1
  data_files:
  - split: test
    path: runs_release/reports/artifact_conformance_consensus/final/label_corrections_v1_1.csv
- config_name: author_decisions
  data_files:
  - split: test
    path: rebuttal/full_conformance_audit/author_decisions.csv
- config_name: model_assisted_decisions
  data_files:
  - split: test
    path: rebuttal/full_conformance_audit/model_assisted_decisions.csv
---

# Build-and-Find

Build-and-Find is a protocol and benchmark for evaluating repositories that
coding agents write for other agents to use. A builder agent receives a hidden
specification and writes a codebase; a finder agent is given only that codebase
and a specification-traced multiple-choice question bank. The benchmark reports
whether finders recover the intended behavior and design choices (recovery
accuracy, repeatability, implementation coverage) and how much inspection the
recovery takes (inspection effort), with question-only and spec-only controls.
This dataset holds the task pack, the generated artifacts, every run record, the
canonical tables, the derived reports, and the post-review audit records behind
the paper.

- Paper: https://openreview.net/forum?id=y48mj8HiWu
- Code: https://github.com/JacobLinCool/Build-and-Find
- DOI: [10.57967/hf/11347](https://doi.org/10.57967/hf/11347)
- Version: 1.1.0 (camera-ready), revision `v1.1.0` of this dataset; changes
  from the reviewed v1.0.0 are listed in `CHANGELOG.md`.

## Dataset structure

The data files have the same paths as in the code repository. The seed projects
and the `edit_*` tasks of the code repository are not used by the reported study
and are not included. To check the files with `make release-check`, download the
data directories into a checkout of the code, from its root:

```bash
HF_DATASET_URL="https://huggingface.co/datasets/JacobLinCool/Build-and-Find"
uvx --from huggingface_hub hf download "${HF_DATASET_URL#https://huggingface.co/datasets/}" \
  --repo-type dataset --revision v1.1.0 \
  --include "benchmarks/**" --include "runs_release/**" --include "rebuttal/**" \
  --local-dir .
```

Downloading the whole dataset into a checkout would replace the code
repository's `README.md` with this card. `croissant.json` is the same file as
`metadata/croissant.json` in the code repository: it gives each file as its URL
at revision `v1.1.0` and each file set as paths in this repository.

| path | contents | license |
|---|---|---|
| `benchmarks/specs/`, `benchmarks/questions/` | two Rust build-from-scratch task specifications (`scratch_minidb`, `scratch_nanoweb`) and their question banks, 15 questions each, with gold answers and specification traces | CC BY 4.0 |
| `runs_release/records/` | 1,924 sanitized run records: 48 builds, 1,728 artifact-conditioned find runs, 144 control find runs, 4 control build records | CC BY 4.0 |
| `runs_release/artifacts/build-*/` | the 48 generated codebases, one directory per build run | Apache-2.0 |
| `runs_release/artifacts/control-build-*/` | the 4 control-build workspaces of the question-only and spec-only controls (`CONTROL.md`, and a copy of the task specification in the spec-only workspaces) | CC BY 4.0 |
| `runs_release/tables/` | canonical tables (`build_runs.csv`, `find_runs.csv`, `question_answers.csv`, `panel_membership.csv`, `experiments.json`) and `MANIFEST.json` | CC BY 4.0 |
| `runs_release/reports/` | derived reports, including the v1.1.0 layers listed below | CC BY 4.0 |
| `rebuttal/full_conformance_audit/` | post-review source-trace audit of the implementation labels: protocol, audit tables, evidence excerpts with source-file hashes, manifests, decisions, and the adjudicated-label sensitivity analysis | CC BY 4.0 |
| `rebuttal/conformance_spot_audit/` | post-review spot audit of 24 consensus prelabels | CC BY 4.0 |
| `croissant.json` | Croissant 1.1 metadata with Responsible AI fields (`metadata/croissant.json` in the code repository) | CC BY 4.0 |
| `LICENSE-DATA`, `LICENSE-CODE` | license notices: CC BY 4.0 for the data, Apache-2.0 for the generated codebases | |
| `CHANGELOG.md` | changes from v1.0.0 to v1.1.0 | CC BY 4.0 |

The v1.1.0 report layers are `runs_release/reports/eandd_final/control_exclusions/`,
`runs_release/reports/eandd_final/transcript_audit/`,
`runs_release/reports/option_free_check/`, `runs_release/reports/runtime_tests/`,
`runs_release/reports/artifact_conformance_consensus/final/label_corrections_v1_1.csv`,
and `runs_release/reports/artifact_conformance_consensus/impact/conditioned_builder_scores_by_task.csv`.

### Configurations

| config | file | rows | one row per |
|---|---|---:|---|
| `find_runs` (default) | `runs_release/tables/find_runs.csv` | 1,872 | find run (artifact-conditioned or control) |
| `question_answers` | `runs_release/tables/question_answers.csv` | 28,080 | answered question |
| `build_runs` | `runs_release/tables/build_runs.csv` | 52 | build run, including the 4 control build records |
| `panel_membership` | `runs_release/tables/panel_membership.csv` | 151,428 | (panel, table, row) membership |
| `implementation_labels` | `.../final/artifact_conformance_final.csv` | 720 | artifact-question pair (v1.0 labels) |
| `label_corrections_v1_1` | `.../final/label_corrections_v1_1.csv` | 15 | corrected artifact-question pair |
| `author_decisions` | `rebuttal/full_conformance_audit/author_decisions.csv` | 43 | audit flag decided by the author |
| `model_assisted_decisions` | `rebuttal/full_conformance_audit/model_assisted_decisions.csv` | 162 | audit flag decided by model-assisted adjudication and approved by the author |

The tables keep every released run. Analyses select rows by the `panel_ids`
column: `paper_final_scratch` is the primary panel (status-ok, compile-pass
builds), `question_only` and `spec_only` are the controls, and
`artifact_present_diagnostic` adds the compile-failed and empty artifacts. The
v1.1.0 question-only control excludes the 11 runs listed in
`runs_release/reports/eandd_final/control_exclusions/excluded_question_only_runs.csv`;
those runs are still in the tables. Implementation-aware analyses apply
`label_corrections_v1_1.csv` on top of `artifact_conformance_final.csv`.

```python
from datasets import load_dataset

repo_id = "https://huggingface.co/datasets/JacobLinCool/Build-and-Find".removeprefix("https://huggingface.co/datasets/")
find_runs = load_dataset(repo_id, "find_runs", split="test")
answers = load_dataset(repo_id, "question_answers", split="test")
```

## Intended use

- Evaluating generated coding-agent artifacts by whether a calibrated finder
  panel recovers specification-traced design intent from them, and how much
  inspection the recovery takes once the recovery and stability gates hold.
- Reproducing the paper's tables and reports from the stored records, inspecting
  the analysis, and auditing the implementation labels.
- Comparing new builders, finders, or harnesses against the frozen evidence
  contract, using the scripted core finder panel of the code repository.

## Out-of-scope use

- Certifying deployment safety, security, runtime correctness beyond the
  recorded compile probe and builder-written tests, or human maintainability.
- Public leaderboards on this task pack or universal single-axis model rankings:
  the results are panel-local and the task pack is public.
- Training a model on the task pack and then evaluating it on the same pack.
- Treating the multiple-choice accuracy as an artifact-quality score: on this
  high-prior pack it is a validity gate.

## Data collection

Runs were collected with the Build-and-Find harness between 30 April and 3 May
2026 (UTC) from twelve agent configurations: six models, each at high and low
reasoning effort.

| CLI | models |
|---|---|
| Codex CLI | `gpt-5.5`, `gpt-5.4-mini` |
| Claude Code CLI | `claude-opus-4-7`, `claude-sonnet-4-6`, `mimo-v2.5-pro`, `mimo-v2.5` (MiMo through an Anthropic-compatible endpoint) |

- **Builds.** Each of the 12 builders built each task twice from the hidden
  specification: 48 builds, 41 of which pass the compile probe
  (`cargo build --release`).
- **Finds.** Each of the 12 finders answered the question bank of each of the 48
  artifacts three times: 1,728 artifact-conditioned find runs. Option order is
  shuffled per run with a recorded seed, answers are written to a file in the
  finder's workspace, and effort is recorded as bytes exchanged with the model.
- **Controls.** Each finder answered each question bank three times with no
  artifact (question-only) and three times with the specification only
  (spec-only): 144 control find runs.
- **Environment.** Builder and finder workspaces were per-run directories inside
  the harness repository. The agent CLIs ran with permission prompts disabled and
  without an OS-level sandbox.
- **Post-review layers (July to October 2026).** A transcript check of the find
  runs; an option-free check in which GPT-5.5-high and Opus 4.7-high answered
  eight low-prior questions without answer choices (24 sessions, 96 answers); one
  run of each artifact's builder-written test suite (`cargo build --release` and
  `cargo test`); and model-assisted source-trace audits of the implementation
  labels.

## Annotations

- **Gold answers.** The question banks and their gold answers were written by
  the author with LLM assistance and reviewed by the author. Every question
  carries a source trace to a specification intention.
- **Implementation labels** (`implementation_labels`). Each artifact-question
  pair was prelabeled when the stable answers of the Codex GPT-5.5-high and
  Claude Opus 4.7-high finders agreed (696 pairs); the other 24 pairs were
  labeled by manual review. The prelabels are triage labels, not independent
  conformance annotations.
- **v1.1.0 correction** (`label_corrections_v1_1`). The one build without source
  files has its 15 pairs relabeled `absent_or_ambiguous`
  (`label_source = rule_empty_artifact`).
- **Source-trace audit and adjudication.** After review, a model-assisted source
  trace of the 592 scored compile-pass pairs (a primary and a cross-check pass)
  flagged 205 pairs. Every flag has a recorded decision: 43 by the author
  (`author_decisions`; 41 relabeled, 2 kept) and 162 drafted by model-assisted
  adjudication, with two passes (the second without access to the audit
  verdicts) and arbitration where they disagreed, then reviewed and approved by
  the author (`model_assisted_decisions`; 154 relabeled, 8 kept;
  `decided_by` records the model-assisted source). The adjudicated labels enter
  the sensitivity analysis in
  `rebuttal/full_conformance_audit/label_sensitivity_adjudicated/`; the main
  analyses use the released labels with the v1.1.0 correction.
- **Option-free grading.** The author graded the 96 free-text answers against a
  rubric fixed before the sessions, in two frozen passes (claim correctness, then
  evidence sufficiency). An LLM labeled the same rows for a consistency check
  only.

No crowdsourced or human-subject annotation was used. The raw agent sessions of
the audits are not included; the audit tables keep each session's id and result
hash.

## Corrections in v1.1.0

v1.1.0 adds files and changes analysis outputs; the v1.0.0 records, tables,
artifacts, and implementation labels are unchanged.

- **Control exclusions.** 11 of the 72 question-only control runs read files
  outside their workspace (3 of them answer-bearing files) and are excluded from
  the question-only control. Question-only accuracy changes from 94.5% to 97.2%.
- **Matched control differences.** Differences from the question-only control are
  computed over matched finder, task, and trial strata with bootstrap intervals.
- **Label corrections and adjudication records.** The empty-artifact correction
  above, and the audit decisions with the adjudicated-label sensitivity analysis.
- **Transcript audit.** No primary-panel finder read the specification or the
  gold-annotated question files; excluding the 62 primary-panel runs whose
  transcripts reach outside their workspace changes accuracy by at most 0.11 pp
  and leaves every R_b rank unchanged.
- **Added checks.** The option-free check, the builder-written test-suite runs
  (the full suite passes in 40 of the 41 compile-pass builds), and per-task R_b.
- **R_b intervals.** The bootstrap now keeps repeated draws of a cell as separate
  cells, which widens some intervals; point estimates and ranks are unchanged.

## Limitations

- **High-prior task pack.** Question-only accuracy is 97.2%, so accuracy is a
  validity gate, and inspection effort carries the artifact comparison only after
  the recovery and stability gates. Two Rust tasks and twelve agent
  configurations make every result panel-local.
- **Unsandboxed runs.** The agents could read files outside their workspaces. The
  control exclusions and the transcript audit document which runs did; other
  effects of the shared environment cannot be ruled out.
- **Label provenance.** The scored implementation labels come from finder
  consensus (589 pairs) and manual review (3 pairs). Post-submission checks found
  9 label errors, and under the adjudicated labels 195 of the 592 scored pairs
  move away from implemented-gold. Unflagged pairs were not re-reviewed, and
  builder rankings are conditional on the label set.
- **Question construction.** The question banks were written with LLM assistance;
  the drafting and distractor-selection histories were not retained.
- **Correctness.** The compile probe and the builder-written tests indicate
  self-consistency, not specification conformance.
- **Drift and contamination.** Hosted models and CLIs change, so new runs are not
  bit-identical; seeds reproduce harness-side randomization only. The public task
  pack may enter training data; contamination-resistant studies need private
  packs with the same schema.

## Personal and sensitive information

The dataset contains no human-subject data and no personal or sensitive
information. Run records hold model and adapter ids, seeds, hashes, provider
session ids, timestamps, answers, and usage metrics; local paths and per-run
session logs were removed before release. Audit records name the author or
model-assisted adjudication as the decision source.

## Licenses

Data are released under CC BY 4.0 (`LICENSE-DATA`). The generated codebases
under `runs_release/artifacts/build-*/` are source code under Apache-2.0
(`LICENSE-CODE`), like the harness in the code repository. License fields that
the builder agents wrote into the codebases, such as `license` entries in
`Cargo.toml` or license sections in a README, are generated content kept as
produced, not license grants; the release licenses every generated codebase
under Apache-2.0. The control-build workspaces under
`runs_release/artifacts/control-build-*/` are data under CC BY 4.0.

## Citation

```bibtex
@inproceedings{buildandfind2026,
  title     = {Build-and-Find: An Effort-Aware Protocol for Evaluating Agent-Managed Codebases},
  author    = {Jhen-Ke Lin},
  booktitle = {Advances in Neural Information Processing Systems (NeurIPS 2026), Evaluations and Datasets Track},
  year      = {2026},
  url       = {https://openreview.net/forum?id=y48mj8HiWu}
}
```

## Maintenance

Versions are released as new files on top of the prior records. Questions and
corrections: issues in the code repository (https://github.com/JacobLinCool/Build-and-Find) or jacob.cs14@nycu.edu.tw.
