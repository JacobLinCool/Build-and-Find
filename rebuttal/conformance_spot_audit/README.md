# Consensus-prelabel source spot audit

This post-review diagnostic checks whether `implemented_gold` labels produced by
the finder-consensus triage stage survive a fresh source-level execution-path review.
It is not a substitute for a complete independent human audit.

## Sample

- Population: compile-passing artifact-question pairs labeled `implemented_gold` by
  `consensus_prelabel`.
- Exclusions: the two pilot pairs already identified as false positives
  (`scratch_minidb:q_db_05` on build `...51f333b3` and
  `scratch_nanoweb:q_nw_07` on build `...44ad1601`).
- Seed: `20260725`.
- Design: 24 pairs, stratified by task × builder family, four pairs per stratum.
  Within each stratum, the sampler selected distinct artifacts and distinct questions.
- Effective unit: artifact-question pair.

The frozen sample is in `sample_manifest.csv`.

## Adjudication contract

For each pair, the adjudicator must:

1. read the canonical question, all four options, and the traced specification item;
2. inspect the cited artifact's source, following the live execution path rather than
   accepting declarations, README claims, dead code, or tests alone;
3. assign one of `implemented_gold`, `implemented_non_gold`,
   `absent_or_ambiguous`, `documented_only`, or
   `not_applicable_or_invalid`;
4. record concrete files, symbols, a concise source-grounded rationale, confidence,
   and whether runtime reachability was checked.

Two separate task-level files are used so the adjudicators cannot overwrite one
another. The final report must disclose that these are fresh model-assisted source
reviews, not independent human annotations, and must not infer a population error
rate from 24 pairs without an uncertainty interval.
