# Option-free check

The option-free check asks eight of the ten low-prior questions as open questions,
with no answer choices, and grades the free-text answers against a rubric. It
compares answers written with a released build artifact in the workspace to
answers written from the question alone. `report.md` gives the results and
`summary.json` the same values in machine-readable form.

## Design

- **Questions.** The low-prior subset is the ten questions whose question-only
  three-trial agreement is below 90% on the released controls
  (`reports/eandd_final/low_prior/manifest.json`). The check uses eight of them:
  `q_db_01`, `q_db_05`, `q_db_06`, `q_db_15`, `q_nw_01`, `q_nw_04`, `q_nw_07`, and
  `q_nw_10`. The other two, `q_db_07` and `q_nw_11`, are negative-contrast items
  that ask for the inconsistent option and therefore require visible choices.
- **Fixed before the runs.** The grading rubric (`rubric.md`) and a deterministic
  artifact rule were fixed before any session ran. The rule takes, per task, the
  compile-pass artifacts that are implemented-gold on all 15 questions under the
  released labels and whose builder family differs from both finder families, and
  keeps the lexicographically first build run id. It selects one MiMo-built
  artifact per task:
  - `scratch_minidb`: `build-scratch_minidb-claude_code_mimo_v2_5_low-t0-s20260501-51f333b3`
  - `scratch_nanoweb`: `build-scratch_nanoweb-claude_code_mimo_v2_5_high-t1-s20260502-44ad1601`
- **Finders.** `codex_gpt5_5_high` and `claude_code_opus_4_7_high`.
- **Conditions.** In `artifact_open` the workspace is a copy of the selected
  artifact, and each answer must cite the source files that implement the
  behavior it describes. In `question_only_open` the workspace holds only a
  `CONTROL.md` marker, and answers carry no evidence fields.
- **Scale.** 2 finders x 2 conditions x 3 trials x 2 tasks = 24 sessions of four
  questions each, giving 96 responses.

Session workspaces were created inside the repository checkout, under
`rebuttal/open_pilot/workspaces/`; the runner's copy of `rubric.md` sits two
directory levels above each workspace. The multiple-choice comparison in
`report.md` uses the released find runs of the same finders on the same artifacts
and questions (May 2026); the option-free sessions ran in July 2026.

The freeze date in `rubric.md` (2026-07-24) is a local date in UTC+8. The first
session record was created at 2026-07-23T18:25:46Z (`summary.json`,
`compute.first_session_created_at`), which is 2026-07-24 02:25 in UTC+8; record
dates in `summary.json` are UTC.

## Grading

The author graded all 96 answers. The grading sheet lists the answers in a fixed
pseudo-random order, sorted by `sha256("20260725|<open_find_run_id>|<question_id>")`,
and omits condition, finder, trial, and run identifiers; `grading_key.csv` maps
each `grading_id` to them. The sheet shows each claim with its rationale. The
rubric hides the evidence citations during pass 1, but the frozen pass-1 sheet
includes them (`summary.json`, `grading.pass1_rows_with_evidence_citations`);
because only artifact-condition answers carry citations, the condition was often
inferable from a row.

1. Pass 1 graded correctness (`claim_correct`: `correct`, `incorrect`, or
   `unanswered`) against `rubric.md`, with no partial credit, and was frozen.
2. Pass 2 graded whether the cited files contain the code that implements the
   described behavior (`evidence_sufficient`) and was frozen. It left every pass-1
   correctness label unchanged.

`grader_note` records the reasoning on borderline rows. `rubric.md` assigns
`not_applicable` as the evidence label of question-only rows; the pass-2 sheet
labels those rows `no_evidence_cited`. Reported counts use the evidence label of
artifact-condition rows only.

An LLM graded the same rows under the same rubric (`llm_prelabels.csv`) for a
consistency check. Its labels enter only the agreement counts in `report.md`.

The grading sheets were written by an export step that is not part of the
release. `scripts/analyze_option_free_check.py` checks that every sheet row
reproduces the recorded claim, evidence, and rationale of its session and that the
row order follows the shuffle rule.

## Implementation labels of the selected pairs

The artifact rule used the released labels, under which both artifacts are
implemented-gold on all 15 questions. The full source-trace audit later flagged
pairs on both artifacts, including three of the eight option-free pairs. Two of
them, `q_db_05` on the minidb artifact and `q_nw_07` on the nanoweb artifact, are
among the nine observed source-audit errors; the third, `q_db_01` on the minidb
artifact, changes label under the adjudicated labels. Correctness grading
applies the rubric's accept and reject criteria as frozen. `report.md` adds two sensitivities that drop the relabeled pairs
from both conditions, and lists the labels of every option-free pair.

## Files

| file | contents |
| --- | --- |
| `records/openfind-*.json` | one record per session: condition, task, source artifact, run manifest, the four answers (claim, evidence files and symbols, rationale), and usage |
| `grading_key.csv` | `grading_id` to run id, condition, finder, trial, seed, and source artifact |
| `grading_sheet.pass1.csv` | the frozen correctness pass |
| `grading_sheet.pass2.csv` | the frozen evidence pass: the same rows with `evidence_sufficient` added |
| `rubric.md` | the grading rubric fixed before the runs |
| `llm_prelabels.csv` | LLM labels for the consistency check |
| `summary.json`, `report.md` | generated by `scripts/analyze_option_free_check.py` |

## Release copy

`scripts/run_open_pilot.py` runs the sessions and writes raw records and
per-session logs to `rebuttal/open_pilot/`. This directory is a sanitized copy of
that output:

- each record drops `usage.trace_path`, a local path to the agent session log;
  all other fields are unchanged;
- per-session logs (prompt, stdout, stderr, usage events), session workspaces, the
  build cache, the launch log, and spreadsheet copies of the grading sheets are
  not included;
- the LLM label file was renamed from
  `llm_prelabels.DO_NOT_OPEN_BEFORE_HUMAN_GRADING.csv`;
- `rubric.md` is unchanged, so the paths it names refer to the runner's directory
  `rebuttal/open_pilot/`.

`uv run python scripts/run_open_pilot.py --dry-run` renders the four prompts; their
hashes equal the `manifest.prompt_hash` of the records.

## Regenerating the outputs

```bash
BNF_RUNS_ROOT=runs_release uv run python scripts/analyze_option_free_check.py
```

`make paper-analysis` runs the same command after the control exclusions.
