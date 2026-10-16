# Question-only control exclusions

A question-only control run gives the finder only the question bank. The
question-only control excludes runs whose transcripts show reads outside their
workspace. `excluded_question_only_runs.csv` lists the 11 excluded runs of the
72 released question-only runs; the other files in this directory are generated
from that list and the released tables.

## Excluded runs

| exposure | runs | what the transcript shows |
| --- | ---: | --- |
| `answer_bearing` | 3 | reads of the task specification, a gold-annotated question file, or another finder's answer file |
| `seed_project` | 6 | reads of the benchmark's seed projects for edit tasks (`benchmarks/seed_projects/`) |
| `other_artifact` | 2 | reads of another find run's workspace copy of a build artifact |

Columns of `excluded_question_only_runs.csv`:

- `find_run_id`, `finder_id`, `task_id`, `finder_trial_id`: the run as recorded in
  `runs_release/tables/find_runs.csv`.
- `exposure`: one of the three classes above.
- `evidence`: what the run read outside its workspace.

## How the list was produced

The list comes from a transcript audit of the agent logs of all 144 control runs
(72 question-only, 72 spec-only). These logs are not part of the release. Every
tool call that read, searched, or listed a path outside the run's own workspace
was flagged, each flag was reviewed against the returned tool output, and a
second, independent parse of the logs found the same runs. All 11 listed runs
read file contents outside their workspace. No spec-only run read a file outside
its workspace, so the spec-only control is unchanged.

## Effect on question-only accuracy

| question-only runs | accuracy |
| --- | ---: |
| all 72 released runs | 94.5% (1021/1080) |
| excluding the 11 listed runs (61 runs) | 97.2% (889/915) |
| excluding only the 3 answer-bearing runs (69 runs) | 96.6% (1000/1035) |

Differences between an artifact-conditioned or spec-only slice and the
question-only control use matched strata: when a control run is excluded, the
rows of the same finder, task, and finder trial also leave the other arm. The
frozen low-prior and implementation-signal question sets stay as selected on the
released controls (all 72 runs).

## Regenerating the outputs

```bash
BNF_RUNS_ROOT=runs_release uv run python scripts/analyze_control_exclusions.py
```

The script replays the released `validity_stress/bootstrap_lift.csv` first and
stops if it does not reproduce exactly. It writes:

- `printed_values.csv`: every control-dependent value printed in the paper, with
  its location, printed text, and exact value.
- `calibration_rows.csv`: Table 1 and Table 14 rows (all-row accuracy, matched
  difference with 95% interval, implemented-gold accuracy and perfect-run rate).
- `contrasts.csv`: each question-cluster bootstrap contrast and its RNG stream.
- `repeatability.csv`: Table 2 and Table 15 rows.
- `low_prior_questions.csv`: agreement of the ten frozen low-prior questions
  before and after the exclusions.
- `finder_calibration.csv`: Table 9 rows.
- `finder_selection.csv`: the core-finder selector rerun on the corrected control.
- `summary.json`: inputs, input hashes, parameters, and checks.
- `report.md`: the same values grouped by paper location.
