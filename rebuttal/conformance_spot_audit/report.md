# Consensus-prelabel source spot audit — results

## Scope and method

This is a post-review diagnostic, not an independent human audit. A fresh
GPT-5.6 source-adjudication pass reviewed a fixed-seed sample of 24 compile-passing
artifact-question pairs labeled `implemented_gold` by the consensus-prelabel stage.
The sample excludes the two pilot pairs already known to be false positives and is
stratified by task × builder family, with four distinct artifacts and questions per
stratum. The artifact-question pair is the effective unit.

For each pair, the adjudicator read the canonical question/options and traced
specification item, then followed the artifact's live source path. README claims,
declarations, dead code, tests, and the original finder consensus were not sufficient
evidence. Every decision records concrete files, symbols, reachability, confidence,
and rationale.

The deterministic selector is
`scripts/select_rebuttal_conformance_spot_audit.py`; `--check` reproduces the frozen
`sample_manifest.csv`.

## Results

| Task | Pairs | Consensus confirmed | Disagreements |
|---|---:|---:|---:|
| scratch_minidb | 12 | 11 | 1 |
| scratch_nanoweb | 12 | 6 | 6 |
| **Total** | **24** | **17** | **7** |

All seven disagreements were reclassified from `implemented_gold` to
`absent_or_ambiguous`. Six have high confidence and one has medium confidence.
All 24 rows include a live-path check, and every recorded evidence path exists.

The disagreements include:

- an auto-transaction path whose state flag prevents the intended auto-commit;
- a graceful-shutdown path that waits on the accept loop but detaches in-flight
  handlers;
- three negative/forbidden-coupling questions for which the artifact makes more than
  one option valid;
- two trie implementations that match the high-level architecture but violate the
  gold option's allocation guarantee.

A second GPT-5.6 source pass reproduced all seven disagreement labels. It completed
its source judgments before an overly broad search exposed prior rationales, so this
is useful corroboration but not a formally blinded or model-independent replication.

## Interpretation

The 17/24 agreement is descriptive. The sample has only four pairs per stratum, the
adjudication is model-assisted rather than human, and it was not designed to estimate
a population error rate. It nevertheless establishes that stable finder consensus
cannot be treated as source-adjudicated ground truth.

The deterministic corrected-label sensitivity analysis
(`scripts/analyze_rebuttal_label_sensitivity.py`) overlays the two pilot
corrections and all seven spot-audit disagreements without modifying
`runs_release/`. The public outputs report it as the `all_observed_corrected`
scenario in `rebuttal/full_conformance_audit/label_sensitivity_adjudicated/`
(`make label-sensitivity`). Under all nine observed corrections:

- implemented-gold scoring pairs change from 592 to 583;
- conditioned recovery changes from 98.930% to 98.971%;
- the low-prior conditioned rate changes from 97.874% to 97.942%;
- per-builder implementation coverage changes from 85.0%–100.0% to
  83.3%–100.0%;
- high- and low-effort builder rank orders are unchanged;
- perfect-gate contributing cells remain 66/66 and 64/66.

This establishes robustness to the observed corrections only. It does not validate
the remaining consensus prelabels; complete source adjudication remains necessary
before treating implementation-conditioned scores or rankings as validated.
