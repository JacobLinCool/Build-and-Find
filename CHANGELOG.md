# Changelog

## v1.1.0: camera-ready corrections (2026-10-16)

v1.0.0 is the release as submitted to the NeurIPS 2026 Evaluations & Datasets
track; the public v1.0.0 commit is that release without the manuscript sources.
v1.1.0 corrects errors found after submission. The released v1.0 records,
tables, artifacts, and implementation labels are unchanged; every correction is
an added file or a change to an analysis script, a derived report, a figure, or
the paper.

### Question-only control

- 11 of the 72 question-only control runs read files outside their workspace:
  3 read answer-bearing files (the specification, a gold-annotated question file,
  or another finder's answer file), 6 read the benchmark's seed projects, and 2
  read other runs' artifact copies. They are listed in
  `runs_release/reports/eandd_final/control_exclusions/excluded_question_only_runs.csv`
  and excluded from the question-only control. Question-only accuracy changes from
  94.5% (1021/1080) to 97.2% (889/915); excluding only the 3 answer-bearing runs
  gives 96.6%. No spec-only run read a file outside its workspace.
- The low-prior and implementation-signal question sets stay as selected on the
  released controls; 6 of the 10 low-prior questions still meet the selection
  rule after the exclusions. The core-finder selection, rerun on the corrected
  control, is unchanged.
- New script `scripts/analyze_control_exclusions.py` (run by
  `make paper-analysis`) writes every control-dependent value printed in the
  paper to `runs_release/reports/eandd_final/control_exclusions/`.

### Differences from the question-only control

- Δ is now the difference between all-row accuracy and the question-only control
  over matched finder, task, and trial strata, with a 95% task-question-cluster
  bootstrap interval (2,000 resamples). In v1.0, Δ was the slice's accuracy minus
  the unmatched question-only rate, without an interval, and artifact-conditioned
  slices used implemented-gold rows only. Implemented-gold accuracy is still
  reported, without Δ.
- Main changes: compile-pass +4.4 pp → −0.4 pp [−3.2, +2.6]; spec-only
  +5.4 pp → +2.7 pp [+1.0, +5.0]; low-prior subset +9.0 pp → +3.8 pp [−2.6, +10.6].
  With artifact access, all-row accuracy is therefore indistinguishable from the
  question-only control. The paper uses accuracy only as a gate and bases the
  artifact comparison on gated inspection effort; v1.0 stated that accuracy still
  verified recovery.
- Table 9 (finder calibration): Formal is now accuracy over all compile-pass
  answer rows, each Δ is relative to the finder's own question-only control on
  matched strata, and rows are re-sorted by Formal. The largest overall Δ,
  MiMo-v2.5-pro-low, changes from +34.3 pp to +12.9 pp.

### Implementation labels

- Added
  `runs_release/reports/artifact_conformance_consensus/final/label_corrections_v1_1.csv`.
  The MiMo 2.5-low `scratch_nanoweb` build `957fa423` has no source files, so its
  15 questions change from `implemented_gold` to `absent_or_ambiguous`
  (`label_source = rule_empty_artifact`). The corrections are applied as an
  overlay; `artifact_conformance_final.csv` stays as released (see
  `final/README.md`).
- Label sources change from 696 `consensus_prelabel` + 24 `manual_review` to
  681 + 24 + 15 `rule_empty_artifact`; implemented-gold pairs from 687 to 672.
  MiMo 2.5-low implementation coverage changes from 95.0% to 70.0% and its
  implementation-aware downstream recovery from 92.6% to 68.2% (Figure 2,
  Table 11). The 592 scored compile-pass pairs are unchanged.
- Paper Appendix H now states the label provenance of the 592 scored pairs
  (589 consensus, 3 manual), the 9 label errors found by post-submission source
  checks, and a model-assisted source trace that flagged 205 pairs, including
  all 9 errors. All 205 flags have decisions: 43 by the author (41 relabeled, 2
  kept) and 162 by model-assisted adjudication (two passes, the second without
  access to the audit verdicts, and arbitration where they disagreed) that the
  author reviewed and approved (`decision` values: 151 `absent_or_ambiguous`,
  3 `non_gold`, and 8 `keep_implemented_gold`), recorded in
  `rebuttal/full_conformance_audit/author_decisions.csv` and
  `rebuttal/full_conformance_audit/model_assisted_decisions.csv`. Appendix H and
  its new Table 12 report coverage, implemented-gold accuracy, and R_b under
  these adjudicated labels (`fully_adjudicated` in
  `rebuttal/full_conformance_audit/label_sensitivity_adjudicated/`,
  `make label-sensitivity`) as a sensitivity analysis; the paper's other
  analyses use the released labels with the v1.1 empty-artifact correction.
  Appendix I reports how many evidence-audit answers fall on relabeled pairs
  (`evidence_audit_overlap.csv` in the same directory).

### R_b intervals

- The finder-task cluster bootstrap for R_b merged a cell drawn twice into one
  cell. Repeated draws are now separate cells. Point estimates and ranks are
  unchanged; 12 interval rows each in `impact/conditioned_builder_scores.csv` and
  `impact/low_prior_conditioned_builder_scores.csv` become wider. Figures 1 and 6
  are regenerated.

### Execution environment

- Builder and finder workspaces were per-run directories inside the harness
  repository, and the agent CLIs ran without an OS-level sandbox. The paper no
  longer describes the runs as isolated; finders are "given" the artifact and the
  specification is "withheld", and Algorithm 3 uses `CloneWorkspace` (a fresh
  writable copy) in place of `CloneReadOnly`. Section 7 and the checklist
  recommend OS-level sandboxing.
- A transcript check of the 1,728 artifact-conditioned find runs found that no
  primary-panel finder read the specification or the gold-annotated question
  files. Excluding the 62 primary-panel runs whose transcripts reach outside their
  workspace changes accuracy by at most 0.11 pp and leaves every R_b rank
  unchanged; the main analyses keep these runs. New script
  `scripts/analyze_transcript_exposure.py`; outputs in
  `runs_release/reports/eandd_final/transcript_audit/`.

### Other paper corrections

- Algorithms 1 and 3: every build with status `ok` receives find runs, and the
  compile probe selects the primary panel. Table 6 adds the 252 compile-failed
  diagnostic find runs (3,780 answer rows).
- Removed the AELS aggregate score (Section 4.5 paragraph, Appendix F equation and
  table). Added tables renumber the appendices: the example item (Table 5) moves
  submission Tables 5–8 to Tables 6–9; submission Tables 10 and 11 keep their
  numbers; Table 12 (Appendix H) moves submission Tables 12–14 to Tables 13–15;
  Table 16 (per-task R_b) moves submission Tables 15 and 16 to Tables 17 and 18;
  Tables 19 and 20 (option-free check) move submission Table 17 to Table 21.
- Tables 1, 4, and 14 report all-row accuracy with matched Δ and interval, plus
  implemented-gold accuracy; Tables 2 and 15 report reliable questions as counts.
- Section 4.5: the two low-effort leaders (GPT-5.4-mini-low and GPT-5.5-low) are
  not separated by the bootstrap, and their order depends on the
  implementation-label set.
- Section 4.6 reports mean Kendall τ (0.49 / 0.61 high effort, 0.44 / 0.55 low
  effort); the Figure 4 caption states that it shows median τ.
- Appendix A: the seed reproduces harness-side randomization only, not
  provider-side sampling.
- The leakage audit and manual inspection cover the 47 generated artifacts with
  source files (v1.0 said 48).
- Appendix F adds an Uncertainty paragraph; the checklist answer on statistical
  significance changes from No to Yes.

### Added checks and text

- Option-free check (Section 4.2 sentence, Appendix L.1, Tables 19 and 20): eight
  of the ten low-prior questions are asked without answer choices on one artifact
  per task, by GPT-5.5-high and Opus 4.7-high over three trials. Free-text answers
  are correct in 38 of 48 responses with artifact access and 13 of 48 without it;
  the matched multiple-choice arms are correct in all 48 answer rows of each
  condition. Appendix L.1 states that the frozen pass-1 grading sheet includes the
  evidence citations that the rubric hides during pass 1. Sessions:
  `scripts/run_open_pilot.py`; analysis: `scripts/analyze_option_free_check.py`;
  records, rubric, grading sheets, and outputs:
  `runs_release/reports/option_free_check/`.
- Builder-written test suites (Section 6 sentence, Appendix G):
  `cargo build --release` and `cargo test` in each of the 48 released artifacts.
  The full suite passes in 40 of the 41 compile-pass builds, and 2,060 of 2,061
  tests pass.
  Scripts: `scripts/run_artifact_tests.py`, `scripts/summarize_runtime_tests.py`;
  outputs: `runs_release/reports/runtime_tests/`.
- Appendix J adds Table 16, R_b per task under the released labels
  (`impact/conditioned_builder_scores_by_task.csv`, written by
  `scripts/analyze_artifact_conformance_impact.py`).
- Section 3.1 states the build and find flow of the reported study and gives an
  example item; Appendix A lists the item with its specification trace (Table 5).
- Section 3.4 states that accuracy is reported on the audited scoring set and on
  all answer rows, and that only all-row accuracy is compared with the
  question-only control.
- Appendix F adds the paragraphs "Effort estimand" (R_b is finder-conditioned,
  panel-local, and conditional on the implementation labels) and "Answer
  correctness and evidence sufficiency". Appendix I ends with the condition under
  which a future pack can grade evidence sufficiency for every scored pair.
- Appendix N adds "Pack admission": admission rules for LLM-drafted items,
  question-only and option-free prior screening, gold counterbalancing in new
  packs, drafting and distractor-selection histories (not retained for the current
  pack), per-pack calibration, compact specifications, and evaluation cost.
- Section 6 adds three sentences: builder orderings are panel-local and
  conditional on the implementation labels, the builder-written test-suite
  result, and pack admission. The abstract states that the audits are designed to
  separate omitted claims from finder failures. The checklist LLM item declares
  the LLM use in the implementation-label audit and in the option-free
  consistency check.
- Appendix N (Quality control) and the checklist LLM item state that the
  question banks were written with LLM assistance and reviewed by the author.
- `rebuttal/full_conformance_audit/label_sensitivity_adjudicated/` adds
  `relabeled_pairs.csv` and `question_labels.csv`, the relabeled pairs by new
  label and by question that Appendix H reports.

### Release documentation

- `runs_release/reports/claim_evidence.md` is renamed
  `reproducibility_index.md`, written by `scripts/write_reproducibility_index.py`
  and `make reproducibility-index` (formerly `scripts/write_claim_evidence.py`
  and `make claim-evidence`). The index maps the paper's results to the scripts,
  tables, and reports that produce them; the new name separates it from the
  implementation-label audits.
- `final/README.md` documents the v1.1 label corrections; the reproducibility
  index adds entries for the control exclusions, the transcript audit, the v1.1
  labels, the label sensitivity, the per-task R_b, the option-free check, and
  the builder-written test suites; Croissant checksums are refreshed.
- `scripts/verify_release.py` requires the option-free check, runtime-test, and
  per-task R_b outputs.
- Release CSVs are written and stored with LF line endings, and recorded
  checksums and sizes are computed on those LF files, so a fresh checkout passes
  `scripts/verify_release.py`.

### Release packaging

- The code is published as a public GitHub repository (https://github.com/JacobLinCool/Build-and-Find) with two
  commits, tagged `v1.0.0` (the release as reviewed, without the manuscript
  sources) and `v1.1.0` (this version). The `v1.0.0` commit keeps the reviewed
  files unchanged: its table manifest and Croissant file record the sizes and
  hashes of CRLF copies of the four table CSVs, so `make reviewer-check-fast` or
  `make release-check` (which rewrite the tables and refresh the checksums) must
  run before `scripts/verify_release.py`; its `make paper` targets and `paper/`
  references need the manuscript sources, which are not published; and its
  Croissant `url` points to the anonymous mirror used during review.
- The data are published as a Hugging Face dataset (https://huggingface.co/datasets/JacobLinCool/Build-and-Find,
  revision tagged `v1.1.0`) under the same paths as in the repository,
  without the seed projects and the `edit_*` tasks, with `LICENSE-DATA`,
  `LICENSE-CODE`, and this changelog. `hf/README.md` is its dataset card, and
  `metadata/croissant.json` is stored at the dataset root as `croissant.json`.
- In `metadata/croissant.json`, each FileObject `contentUrl` is the URL of the
  file in the Hugging Face dataset at revision `v1.1.0`, and each FileSet is
  contained in the dataset's git repository, so the record sets load from the
  hosted file. `scripts/update_croissant_metadata.py` and
  `scripts/verify_release.py` map these URLs to the files of a checkout. The
  generated artifacts (`build-*`, Apache-2.0) and the control-build workspaces
  (`control-build-*`, CC BY 4.0) are separate file sets.
- The manuscript sources are not part of the public repository; the paper is at
  https://openreview.net/forum?id=y48mj8HiWu. The public post-review audit records are the audit protocol,
  audit tables, evidence excerpts with source-file hashes, manifests, the
  core-generation manifest, `author_decisions.csv`,
  `model_assisted_decisions.csv`, and the adjudicated-label sensitivity outputs;
  the raw audit sessions, the author's adjudication worksheet, and the review
  report of the model-assisted decisions are not included. As recorded in
  `rebuttal/full_conformance_audit/execution_manifest.json`, the source-trace
  audit ran a primary pass with `gpt-5.6-sol` at reasoning effort `ultra` on all
  592 pairs and a cross-check pass with `gpt-5.6-terra` at `ultra` on 242 pairs;
  the passes were blinded procedurally, by the inputs each prompt received.
- `LICENSE` and `LICENSE-CODE` contain the full Apache License 2.0 text, and
  `LICENSE` states which files each license covers. `LICENSE-DATA` is the CC BY
  4.0 notice with the license deed and legal code URLs, and its scope now names
  the control-build workspaces, the metadata, and the audit records. License
  fields that the builder agents wrote into the artifacts (for example the
  `license` entries of some `Cargo.toml` files) are generated content kept as
  produced, not license grants; the release licenses every generated artifact
  under Apache-2.0. `CITATION.cff` is added.
- `README.md` is rewritten for the public release: versions, installation and
  dataset download, release verification, figure regeneration with
  `FIGURES_DIR`, the repository layout, the post-review audit records, a safety
  note on running the agent CLIs in an OS-level sandbox, licenses, and citation.
- The manuscript build targets move to `paper/paper.mk`, which the `Makefile`
  includes only when the manuscript sources are present.
  `scripts/paper_figures.py` writes to `BNF_FIGURES_DIR`, which
  `make release-figures FIGURES_DIR=...` sets, and otherwise to
  `runs_release/reports/figures/` when the manuscript directory is absent;
  `scripts/plot_affinity_residuals.py` takes `--output-dir` and writes its PDFs
  without a creation date, so regenerated plots are byte-identical to the stored
  ones; the three stored plot PDFs differ from v1.0.0 only in that date. Make
  target descriptions and the `insights.md` heading
  "Interpretation Notes" (formerly "Reviewer-Risk Notes") use release wording.
- `metadata/croissant.json` is version 1.1.0. It gives the dataset name,
  description, creator, citation, URL, and DOI; adds file entries for the run
  records, the generated artifacts, the v1.1.0 report layers, and the public
  audit records; describes every column of the canonical tables and record sets
  for the new CSV files; and updates the Responsible AI fields: data collection,
  the annotation protocol (consensus prelabels, manual review, the model-assisted
  source trace, author adjudication including the model-assisted decisions the
  author approved, and option-free grading), the excluded control runs and the
  unsandboxed runs among the limitations, and personal and sensitive
  information.
