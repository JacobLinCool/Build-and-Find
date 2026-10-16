# Build-and-Find

Build-and-Find is a protocol and benchmark for evaluating repositories that
coding agents write for other agents to use. A builder agent receives a hidden
specification and writes a codebase; a finder agent is given only that codebase
and a specification-traced multiple-choice question bank, and writes its answers
to a file. The protocol reports recovery accuracy, repeatability, implementation
coverage, and inspection effort, with question-only and spec-only controls for
what finders can answer without the artifact. This repository contains the
harness, the two released Rust task packs, the generated artifacts and run
records, the analysis scripts, and the reports behind the paper's numbers.

- Paper: https://openreview.net/forum?id=y48mj8HiWu
- Dataset (Hugging Face): https://huggingface.co/datasets/JacobLinCool/Build-and-Find
- Code: https://github.com/JacobLinCool/Build-and-Find
- Changes since the reviewed release: [`CHANGELOG.md`](CHANGELOG.md)

## Versions

| version | contents |
|---|---|
| `v1.0.0` | The release as reviewed for the NeurIPS 2026 Evaluations and Datasets Track, without the manuscript sources. |
| `v1.1.0` | Camera-ready corrections and post-review checks. The v1.0 records, tables, artifacts, and implementation labels are unchanged; every correction is an added file or a change to an analysis script, a derived report, or a figure. [`CHANGELOG.md`](CHANGELOG.md) lists each change. |

Each version is one commit of this repository, tagged `v1.0.0` and `v1.1.0`. The
Hugging Face dataset holds the v1.1.0 data files at its revision `v1.1.0`.

`v1.0.0` keeps the reviewed files unchanged, which has three consequences:

- Its `runs_release/tables/MANIFEST.json` and `metadata/croissant.json` record
  the sizes and hashes of CRLF copies of `build_runs.csv`, `find_runs.csv`,
  `question_answers.csv`, and `panel_membership.csv`, while the repository
  stores these files with LF line endings. Run `make reviewer-check-fast` or
  `make release-check` before `scripts/verify_release.py`: both rewrite the four
  tables with CRLF line endings and refresh the checksums, and the verifier then
  passes. In a fresh checkout, `scripts/verify_release.py` alone reports a byte
  mismatch for `build_runs.csv`. v1.1.0 stores and records LF files.
- Its `make paper`, `make paper-force`, and `make paper-all` targets and its
  references to `paper/` need the manuscript sources, which are not published.
  The paper is at https://openreview.net/forum?id=y48mj8HiWu.
- The `url` fields of its `metadata/croissant.json` point to the anonymous
  mirror used during review.

## Install

Requires [uv](https://docs.astral.sh/uv/) and Python 3.13 or later.

```bash
git clone https://github.com/JacobLinCool/Build-and-Find.git
cd Build-and-Find
uv sync --extra dev --extra paper
```

The release data (`benchmarks/`, `runs_release/`, `metadata/croissant.json`, and
the audit records under `rebuttal/`) are in this repository. The Hugging Face
dataset (https://huggingface.co/datasets/JacobLinCool/Build-and-Find) holds the v1.1.0 data files under the same paths,
without the seed projects and the `edit_*` tasks. Its root also holds the
dataset card (`README.md`) and `croissant.json`, which is the same file as
`metadata/croissant.json`. To download the data files into a checkout, include
only the data directories, so that the dataset card does not replace this
README:

```bash
HF_DATASET_URL="https://huggingface.co/datasets/JacobLinCool/Build-and-Find"
uvx --from huggingface_hub hf download "${HF_DATASET_URL#https://huggingface.co/datasets/}" \
  --repo-type dataset --revision v1.1.0 \
  --include "benchmarks/**" --include "runs_release/**" --include "rebuttal/**" \
  --local-dir .
```

## Verify the release and reproduce the paper's numbers

None of the commands in this section call a model.

```bash
make release-check
```

`make release-check` rebuilds the canonical tables and the derived analysis
reports under `runs_release/reports/` from the run records in
`runs_release/records/`, refreshes the checksums in `metadata/croissant.json`,
and runs `scripts/verify_release.py`. The verifier checks the license files, the
set of released records, the artifact hashes, the table manifest, the required
reports, and every Croissant file entry. The rebuilt tables and reports match the
committed files except for the generation timestamps (`generated_at`) that some
reports record. `make reviewer-check-fast` runs a faster subset (spec
validation, table export, the Croissant refresh, release verification, and the
table tests), and `make test` and `make lint` run the test suite and `ruff`.

`metadata/croissant.json` gives each file as its URL in the Hugging Face dataset
at revision `v1.1.0` and each file set as paths in the dataset's git repository,
so `mlcroissant` can load it from any location. The verifier and
`make update-croissant` map these URLs to the files in the checkout.

To regenerate the figures, choose an output directory:

```bash
make release-figures FIGURES_DIR=figures
```

This reruns the analysis, writes the matrix plots to
`runs_release/reports/eandd_final/plots/`, and writes the paper figures (PDF, PNG,
SVG) and two LaTeX table fragments to `FIGURES_DIR` (default:
`runs_release/reports/figures/`). `make paper-analysis` reruns the analysis
without the figures; both targets read `runs_release/` by default.

The adjudicated-label sensitivity analysis (paper Appendix H, Table 12) is
regenerated separately:

```bash
make label-sensitivity
```

`runs_release/reports/reproducibility_index.md`, written by
`make reproducibility-index`, maps each main result of the paper to the scripts,
tables, and reports that produce it. The main sources are:

| paper result | file |
|---|---|
| main results, pivots, within-finder efficiency, validity stress, low-prior subset | `runs_release/reports/eandd_final/` (`insights.md`, `pivots/`, `within_finder_efficiency/`, `validity_stress/`, `low_prior/`) |
| question-only control values (Tables 1, 2, 4, 9, 14, 15) | `runs_release/reports/eandd_final/control_exclusions/` (`report.md`, `printed_values.csv`) |
| execution-environment sensitivity (Appendix B) | `runs_release/reports/eandd_final/transcript_audit/` |
| implementation-label results, R_b, per-task R_b (Table 16) | `runs_release/reports/artifact_conformance_consensus/impact/` |
| evidence-citation audit | `runs_release/reports/evidence_citation_audit.md` |
| option-free check (Section 4.2, Appendix L.1, Tables 19–20) | `runs_release/reports/option_free_check/summary.json` |
| builder-written test suites (Section 6, Appendix G) | `runs_release/reports/runtime_tests/summary.json` |
| adjudicated-label sensitivity (Appendix H, Table 12) and evidence-audit overlap (Appendix I) | `rebuttal/full_conformance_audit/label_sensitivity_adjudicated/` |

The stored records reproduce every reported analysis. New model runs depend on
provider models, CLIs, and endpoints that change over time, so they are not
expected to be bit-identical.

## Repository layout

```
benchmarks/
├── specs/                   task specifications (scratch_minidb, scratch_nanoweb)
├── questions/               specification-traced multiple-choice question banks
└── seed_projects/           seed code for the edit tasks
experiments/eandd_final_2026/SPEC.md   frozen experiment contract for the reported study
src/bnf/                     harness and `bnf` CLI
scripts/                     analysis, release, and audit scripts
tests/                       pytest suite
runs_release/                release tree (see below)
metadata/croissant.json      Croissant 1.1 metadata with Responsible AI fields
hf/README.md                 dataset card of the Hugging Face dataset
rebuttal/
├── full_conformance_audit/  post-review source-trace audit of implementation labels
└── conformance_spot_audit/  post-review spot audit of implementation labels
CHANGELOG.md                 changes from v1.0.0 to v1.1.0
CITATION.cff                 citation metadata
LICENSE, LICENSE-CODE, LICENSE-DATA
```

The `edit_*` specifications, question banks, and seed projects are not part of
the reported study; `bnf run all` runs the edit tasks only when they are named
with `--tasks` or when `--include-edit` is set.

### Release tree (`runs_release/`)

- `records/*.json`: one sanitized record per run: 48 builds, 1,728
  artifact-conditioned find runs, 144 control find runs (72 question-only and 72
  spec-only), and 4 control build records.
- `artifacts/<build_run_id>/`: the 48 generated codebases (`build-*`) and the
  4 control-build workspaces (`control-build-*`, with `CONTROL.md`, and a copy of
  the task specification in the two spec-only workspaces).
- `tables/`: the canonical projection of the records.
  - `build_runs.csv`: one row per build run.
  - `find_runs.csv`: one row per finder invocation, keyed by task, builder, build
    trial, finder, and finder trial, with per-run metrics.
  - `question_answers.csv`: one row per answered multiple-choice question.
  - `panel_membership.csv` and `experiments.json`: named panels such as
    `paper_final_scratch`, `high_effort_panel`, `low_effort_panel`, and
    `mimo_extension`. Primary paper panels require a status-ok build record, an
    existing artifact path, and a passing compile probe. The
    `artifact_present_diagnostic` panel keeps all status-ok artifact-present
    builds, including the compile-failed and empty artifacts summarized under
    `reports/eandd_final/validity_stress/`.
  - `MANIFEST.json`: sizes, row counts, hashes, and schema version of the tables.
- `reports/`: derived reports. The v1.1.0 additions are
  `eandd_final/control_exclusions/`, `eandd_final/transcript_audit/`,
  `option_free_check/`, `runtime_tests/`,
  `artifact_conformance_consensus/final/label_corrections_v1_1.csv`, and
  `artifact_conformance_consensus/impact/conditioned_builder_scores_by_task.csv`.
  `reproducibility_index.md` was named `claim_evidence.md` in v1.0.0.

`runs_release/` is pruned to the final logical panel, the spec-only and
question-only controls, and the build-fidelity diagnostics. Pilot, edit-task,
legacy, and superseded records and per-run session logs are not released. The
paper scripts read the records through the table layer.

New harness runs append records under `runs/` (or under `BNF_RUNS_ROOT`).
`make analysis-tables RUNS_ROOT=runs` builds the canonical tables for them, and
`make local-paper-analysis` runs the paper analysis on `runs/`; the paper scripts
select the frozen final panel. `make prepare-release` rebuilds `runs_release/`
from a full local `runs/` tree of the study. It deletes the existing
`runs_release/` first, so do not run it in a release checkout.

### Post-review audit records (`rebuttal/`)

These directories hold the implementation-label audits made after review. The
main analyses use the released labels with the v1.1 correction
(`label_corrections_v1_1.csv`); the adjudicated labels enter only the
sensitivity analysis.

- `rebuttal/full_conformance_audit/`: a model-assisted source trace of the 592
  scored compile-pass artifact-question pairs (`SPEC.md`, `report.md`), with the
  audit tables, evidence excerpts with source-file hashes, and manifests. It
  flagged 205 pairs. `author_decisions.csv` records the 43 decisions made by the
  author, and `model_assisted_decisions.csv` the 162 decisions drafted by
  model-assisted adjudication and approved by the author.
  `label_sensitivity_adjudicated/` is written by `make label-sensitivity`.
  `README.md` describes the files.
- `rebuttal/conformance_spot_audit/`: an earlier source audit of a 24-pair sample
  of consensus prelabels.

The raw agent sessions of these audits are not released; the audit tables keep
each session's id and result hash. The author's adjudication worksheet, from
which `author_decisions.csv` was transcribed, and the report the author reviewed
before approving the model-assisted decisions are also not released.

## Running new evaluations

### Prerequisites

- The `codex` and `claude` CLIs are on `PATH` and logged in (`codex login` /
  `claude login`). The harness drives both CLIs as subprocesses and does not own
  their sessions, so any agent whose CLI is unauthenticated fails.
- MiMo agents (`claude_code_mimo_*`) need a Bearer token in the `MIMO_API_KEY`
  environment variable. Other Anthropic-compatible providers use the same
  mechanism; each adapter names the environment variable it reads (see
  `src/bnf/runners/registry.py`).
- Provider endpoints are adapter configuration and are not needed for the
  release analysis.

### Safety

The adapters start `codex` with `--dangerously-bypass-approvals-and-sandbox` and
`claude` with `--dangerously-skip-permissions`, so agents run commands without
permission prompts and can read or write any file that your user account can
access. Run evaluations inside an OS-level sandbox, such as a disposable virtual
machine or container with resource limits, outside any repository you care
about. The released runs used per-run workspaces inside the harness repository
without such a sandbox; `runs_release/reports/eandd_final/transcript_audit/` and
`runs_release/reports/eandd_final/control_exclusions/` report which runs read
files outside their workspace. Setting `BNF_RUNS_ROOT` to an absolute path moves
the run workspaces out of the checkout, but it does not limit what an agent can
read.

### Harness quickstart

```bash
# Inspect the benchmark
uv run bnf agents list
uv run bnf specs list
uv run bnf specs validate                # cross-refs every question back to a spec intention
uv run bnf specs render-prompt scratch_minidb

FINAL_AGENTS=(
  claude_code_opus_4_7_high
  claude_code_opus_4_7_low
  claude_code_sonnet_4_6_high
  claude_code_sonnet_4_6_low
  codex_gpt5_5_high
  codex_gpt5_5_low
  codex_gpt5_4_mini_high
  codex_gpt5_4_mini_low
  claude_code_mimo_v2_5_pro_high
  claude_code_mimo_v2_5_pro_low
  claude_code_mimo_v2_5_high
  claude_code_mimo_v2_5_low
)
AGENTS="$(IFS=,; echo "${FINAL_AGENTS[*]}")"

# Dry-run the pipeline without invoking the real agents
BNF_DRY_RUN=1 uv run bnf run all \
  --tasks scratch_minidb \
  --builders "$AGENTS" \
  --finders "$AGENTS"

# Run the final-study task set. Read experiments/eandd_final_2026/SPEC.md for
# the frozen agent panel and trial contract before spending API budget.
uv run bnf run all \
  --tasks scratch_minidb,scratch_nanoweb \
  --builders "$AGENTS" \
  --finders "$AGENTS" \
  --build-trials 2 \
  --find-trials 3
```

`make eandd-final` runs the frozen final matrix, `make eandd-continue` resumes it
without rerunning successful records, and `make eandd-dry-run` smoke-tests it in
a temporary root. Records are written under `runs/` (or `BNF_RUNS_ROOT`).

### Final-study agents

| id                        | adapter     | model             |
|---------------------------|-------------|-------------------|
| `codex_gpt5_5_high`       | codex       | gpt-5.5           |
| `codex_gpt5_5_low`        | codex       | gpt-5.5           |
| `codex_gpt5_4_mini_high`  | codex       | gpt-5.4-mini      |
| `codex_gpt5_4_mini_low`   | codex       | gpt-5.4-mini      |
| `claude_code_opus_4_7_high`   | claude-code | claude-opus-4-7   |
| `claude_code_opus_4_7_low`    | claude-code | claude-opus-4-7   |
| `claude_code_sonnet_4_6_high` | claude-code | claude-sonnet-4-6 |
| `claude_code_sonnet_4_6_low`  | claude-code | claude-sonnet-4-6 |
| `claude_code_mimo_v2_5_pro_high` | claude-code | mimo-v2.5-pro  |
| `claude_code_mimo_v2_5_pro_low`  | claude-code | mimo-v2.5-pro  |
| `claude_code_mimo_v2_5_high`     | claude-code | mimo-v2.5      |
| `claude_code_mimo_v2_5_low`      | claude-code | mimo-v2.5      |

### Calibrated finder panel

For routine builder or harness comparisons, use the scripted v1 core finder
panel instead of rerunning every calibrated finder:

- Default core finders: `claude_code_sonnet_4_6_low`,
  `claude_code_sonnet_4_6_high`
- Prior-stress audit finder: `claude_code_mimo_v2_5_pro_low`

`scripts/select_eandd_core_finders.py` regenerates the selection and records it
under `runs_release/reports/eandd_final/core_finder_selection/`. The default
panel is smaller than the top formal-accuracy set because some high-accuracy
finders saturate the question-only controls on this task pack.

### Benchmark content

| task_id                    | family                | #FR | #SR | #ER | #MCQ |
|----------------------------|-----------------------|-----|-----|-----|------|
| `scratch_minidb`           | build_from_scratch    | 17  | 12  | 0   | 15   |
| `scratch_nanoweb`          | build_from_scratch    | 15  | 11  | 0   | 15   |

- **`scratch_minidb`**: an embeddable SQL-subset RDBMS: lexer, parser, planner,
  Volcano-style executor, page-based storage and WAL, transaction manager, REPL
  and batch modes.
- **`scratch_nanoweb`**: an HTTP/1.1 web framework: its own HTTP parser, trie
  router, middleware stack, rustls-gated TLS, Prometheus metrics, template
  engine, and graceful shutdown.

Both specifications set a scale target of several thousand lines of Rust. The
compile-pass artifacts have 2,515 to 9,103 lines (`scratch_minidb`) and 1,326 to
5,951 lines (`scratch_nanoweb`).

### Methodology guard-rails

- Builder prompts never contain canonical intention strings or requirement IDs.
- The finder is not told that a specification exists. It is given a codebase and
  multiple-choice questions and picks the option that best fits the code. Gold
  answers stay in the harness and are not copied into the finder workspace.
- Answers are written to a file (`find_answers.json`) in the finder's workspace,
  not parsed from stdout.
- Every Find question carries a `source_trace` back to a specification
  intention; `bnf specs validate` enforces the chain.
- Option order is shuffled per run with a seeded shuffle to reduce position bias
  (`FindQuestion.shuffled`).
- Each run records a `RunManifest` with `(spec_hash, prompt_hash,
  agent.model_id, seed, trial_id, env_hash)`. The seed reproduces harness-side
  randomization only, not provider-side sampling.
- Build-phase validation is a compile probe (`cargo build --release`). The
  primary analysis uses compile-passing artifacts; compile-failed and empty
  artifacts remain in the release as validity-stress diagnostics.
  `runs_release/reports/runtime_tests/` records one run of each artifact's
  builder-written test suite.
- The released task pack is high-prior: accuracy is a validity gate and
  calibration signal, and conditional inspection effort is an agent-facing proxy
  for builder artifact quality only after the recovery and stability gates.
- Builder-finder affinity is a panel-local diagnostic for interaction effects,
  not a universal model-family ranking.
- Metrics use bytes exchanged with the model (UTF-8), not tokens, to remain
  model-agnostic.

### Architecture

```
src/bnf/
├── schema/      BuildTaskSpec, FindQuestion, BuildRun, FindRun (pydantic + YAML)
├── prompts/     Deterministic builder & finder prompt rendering
├── runners/     AgentAdapter protocol + codex, claude-code adapters
├── build/       Workspace preparation, runner, cargo-based compile probe
├── find/        Runner + exact-match scorer; finder writes find_answers.json
├── analysis/    Builder × Finder matrix, bootstrap CI, CSV/JSON/Markdown
└── cli.py       bnf CLI
```

### Adding a new agent

Subclass `AgentAdapter` (or reuse `CodexAdapter` / `ClaudeCodeAdapter`), register
it with `bnf.runners.registry.register`, and it becomes available to
`bnf run all --builders ... --finders ...`.

## Licenses

- Code: Apache-2.0 ([`LICENSE`](LICENSE), [`LICENSE-CODE`](LICENSE-CODE)). This
  covers the harness, scripts, tests, build configuration, the generated code
  artifacts (`runs_release/artifacts/build-*/`), and the seed projects. License
  fields that the builder agents wrote into the artifacts, such as `license`
  entries in `Cargo.toml` or license sections in a README, are generated content
  kept as produced, not license grants; the release licenses every generated
  artifact under Apache-2.0.
- Data: CC BY 4.0 ([`LICENSE-DATA`](LICENSE-DATA)). This covers the task
  specifications, question banks, run records, the control-build workspaces
  (`runs_release/artifacts/control-build-*/`), canonical tables, derived
  reports, metadata, and audit records.

Model APIs and CLIs were used as provider services under their terms; no model
weights or provider CLI code are redistributed.

## Citation

If you use Build-and-Find, please cite the paper ([`CITATION.cff`](CITATION.cff)):

```bibtex
@inproceedings{buildandfind2026,
  title     = {Build-and-Find: An Effort-Aware Protocol for Evaluating Agent-Managed Codebases},
  author    = {Jhen-Ke Lin},
  booktitle = {Advances in Neural Information Processing Systems (NeurIPS 2026), Evaluations and Datasets Track},
  year      = {2026},
  url       = {https://openreview.net/forum?id=y48mj8HiWu}
}
```

## Contact

Please open an issue on GitHub, or write to jacob.cs14@nycu.edu.tw.
