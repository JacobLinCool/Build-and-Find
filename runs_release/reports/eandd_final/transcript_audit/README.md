# Primary-panel transcript exposure

Builder and finder workspaces were per-run directories inside the harness
repository, and the agent CLIs ran without an OS-level sandbox.
`primary_panel_exposure.csv` lists the 62 of the 1,476 primary-panel
(compile-pass) find runs whose transcripts show reads, file listings, or git
commands that reach outside their workspace. The other files in this directory
are generated from that list, the released tables, and the released labels.

## Listed runs

| category | runs | what the transcript shows |
| --- | ---: | --- |
| `answer_files_in_search_output` | 1 | before answering, a recursive search rooted at the parent of the workspace returned lines from other find runs' answer files (`find_answers.json`) |
| `host_config_read` | 1 | after writing its answers, the finder read the host user's agent-CLI settings file; the CLI blocked its attempted edits |
| `git_status_listing` | 48 | `git status` run inside the workspace; the workspace has no `.git`, so git listed changed-file names of the enclosing harness repository |
| `workspace_listing` | 11 | a file-name search from the parent of the workspace (`find ..`) listed file paths in other find runs' workspaces |
| `git_checkout_attempt` | 1 | after writing its answers, the finder ran `git checkout` and `git merge` on the enclosing harness repository; the checkout was a no-op and the merge failed, leaving the repository unchanged |

The 60 runs in the last three categories read no file contents outside their
workspace. No primary-panel finder read the task specification or the
gold-annotated question files.

Columns of `primary_panel_exposure.csv`:

- `find_run_id`, `finder_id`, `task_id`: the run as recorded in
  `runs_release/tables/find_runs.csv`.
- `category`: one of the five categories above.
- `note`: what the run did outside its workspace.

## How the list was produced

The list comes from a structural audit of the agent transcripts of all 1,728
artifact-conditioned find runs (1,476 primary-panel, 252 compile-failed). The
audit read the local agent logs, which are not part of the release. Every tool
call was parsed, including subagent calls. A call was flagged when a path
argument, a shell command (an absolute or `..` path, a search rooted at a parent
directory, or a git subcommand), or the call's output reached a path outside the
run's workspace. Each flagged call was then classed as a content read or a
file-name listing. A second check matched every tool output against distinctive
lines from other build artifacts, other runs' answer files, the seed projects,
the specification and question files, and the harness source; in the primary
panel it flagged only the `answer_files_in_search_output` run. Builds and tests
that used the harness's shared Cargo target directory, and web searches, are not
counted as reaching outside the workspace.

This directory lists only primary-panel runs. Among the 252 compile-failed
diagnostic runs, the same audit found 34 runs that read file contents outside
their workspace, all on the MiMo 2.5-low `scratch_nanoweb` build without source
files (5 read answer-bearing files, 28 read the seed projects, and 1 read another
run's artifact copy), and 9 runs whose transcripts show only file-name listings
or `git status` output. These runs remain in the all-row columns of the
artifact-present and compile-failed rows of the paper's Table 1. The build without
source files is labeled absent on all of its questions, so its 36 runs contribute
no implemented-gold rows.

## Sensitivity analysis

The paper's main analyses keep these runs; excluding them is a sensitivity
analysis:

| primary-panel rows | all 1,476 runs | excluding the 62 listed runs |
| --- | ---: | ---: |
| all answer rows | 96.716% (21413/22140) | 96.822% (20536/21210) |
| implemented-gold rows | 98.930% (21084/21312) | 98.977% (20216/20425) |

Accuracy changes by at most 0.11 pp, and every R_b rank is unchanged in both
the high-effort and the low-effort panel. Implemented-gold rows use the v1.0
labels with `reports/artifact_conformance_consensus/final/label_corrections_v1_1.csv`
applied.

## Regenerating the outputs

```bash
BNF_RUNS_ROOT=runs_release uv run python scripts/analyze_transcript_exposure.py
```

The script writes:

- `summary.json`: inputs, input hashes, accuracy with and without the listed
  runs, and the R_b scores and ranks of both panels.
- `report.md`: the same values as tables.
