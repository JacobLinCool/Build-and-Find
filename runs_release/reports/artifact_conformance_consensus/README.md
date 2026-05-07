# Artifact Conformance Consensus Triage

This directory is a derived pre-audit layer. It uses selected finder answer consensus to queue artifact-question pairs for conformance annotation; it is not an independent correctness oracle.

## Scope

- Base models: `codex_gpt5_5, claude_code_opus_4_7`
- Finder efforts: `high`
- Consensus unit: `base_model`
- Minimum within-model agreement: `0.6666666666666666`
- Include compile-failed artifacts: `True`

## Overall Counts

| pairs | implemented_gold | implemented_non_gold | manual_review_required |
| ---: | ---: | ---: | ---: |
| 720 | 681 | 15 | 24 |

## Files

- `artifact_question_consensus.csv`: one row per artifact-question pair.
- `manual_review_queue.csv`: rows without selected-model consensus.
- `manual_review_queue.md`: human-readable list of rows to inspect.
- `manual_review_annotations.csv`: fill this file with final manual labels.
- `manual_review_annotations.template.csv`: regenerated blank annotation template.
- `model_votes.csv`: one row per artifact-question-base-model vote.
- `finder_votes.csv`: one row per artifact-question-finder-id vote.
- `summary.csv`: aggregate counts by task, compile status, and builder.
- `manifest.json`: inputs, hashes, command, and classification rule.

Final manual labels should use:

`implemented_gold`, `implemented_non_gold`, `absent_or_ambiguous`, `documented_only`, `not_applicable_or_invalid`
