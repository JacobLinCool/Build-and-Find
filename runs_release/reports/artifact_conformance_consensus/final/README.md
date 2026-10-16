# Final Artifact Conformance

This table merges stable consensus prelabels with manual review annotations.
`artifact_conformance_final.csv`, `summary.csv`, and `manifest.json` are
the v1.0 label release and stay unchanged; the counts in the next two
sections are v1.0. The v1.1 corrections are listed in
`label_corrections_v1_1.csv` (see below).

## Overall Counts

| label | count | rate |
| --- | ---: | ---: |
| `absent_or_ambiguous` | 5 | 0.006944 |
| `documented_only` | 0 | 0.000000 |
| `implemented_gold` | 687 | 0.954167 |
| `implemented_non_gold` | 28 | 0.038889 |
| `not_applicable_or_invalid` | 0 | 0.000000 |

## Source Counts

| source | rows |
| --- | ---: |
| `consensus_prelabel` | 696 |
| `manual_review` | 24 |

## v1.1 Label Corrections

`label_corrections_v1_1.csv` lists labels corrected after the v1.0 release.
It is applied as an overlay on top of `artifact_conformance_final.csv`: for each
listed `(task_id, artifact_id, question_id)` pair, the analysis scripts replace
`final_label` and `label_source` with the correction. A correction whose
`old_label` differs from the v1.0 label, a repeated pair, or an unknown pair or
label stops the analysis.

Columns:

- `task_id`, `artifact_id`, `question_id`: the artifact-question pair.
- `old_label`: the v1.0 `final_label` of the pair.
- `new_label`: the v1.1 `final_label`.
- `label_source`: the rule that produced the correction.
- `rationale`: why the label changed.

| label_source | rule | artifacts | rows |
| --- | --- | ---: | ---: |
| `rule_empty_artifact` | the build artifact contains no source files, so it implements no specified claim and every one of its questions is labeled `absent_or_ambiguous` | 1 | 15 |

Counts after the overlay (v1.1):

| label | count | rate |
| --- | ---: | ---: |
| `absent_or_ambiguous` | 20 | 0.027778 |
| `documented_only` | 0 | 0.000000 |
| `implemented_gold` | 672 | 0.933333 |
| `implemented_non_gold` | 28 | 0.038889 |
| `not_applicable_or_invalid` | 0 | 0.000000 |

| source | rows |
| --- | ---: |
| `consensus_prelabel` | 681 |
| `manual_review` | 24 |
| `rule_empty_artifact` | 15 |

The overlay's hash and the counts after it are also recorded in
`../impact/manifest.json` (`label_corrections`, `final_labels`, `label_sources`).
