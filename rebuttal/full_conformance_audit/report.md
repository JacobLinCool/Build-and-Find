# Full-coverage source-trace conformance audit

> Written by the July 2026 finalize step of `scripts/run_full_conformance_audit.py`
> and reworded on 2026-10-16 with the numbers unchanged (see `README.md`). The
> flags that this report lists as pending author adjudication have since received
> decisions, recorded in `author_decisions.csv` and `model_assisted_decisions.csv`.

## Outcome

The primary pass covered all **592 / 592** compile-passing pairs currently labeled `implemented_gold`, across **41 / 41** artifacts. Every primary row has a live-path check, concrete file/symbol evidence, validated line spans, and exact model/prompt/session provenance. Every selected result is bound by an append-only integrity attestation.

These are source reviews by two frontier-model passes, not human annotations. Every flag from either pass remains pending author adjudication; the audit does not modify `runs_release/` or self-resolve review conflicts.

## Models and procedural blinding

| pass | model | effort | prompt version | unit |
| --- | --- | --- | --- | --- |
| primary | `gpt-5.6-sol` | `ultra` | `full_conformance_live_path_v1` | one fresh session per artifact |
| cross-check | `gpt-5.6-terra` | `ultra` | `full_conformance_live_path_v1` | one fresh session per selected artifact |

Primary and cross-check prompts contain the canonical question/options, gold option, spec trace, and artifact path, but no current label, finder vote, prior verdict, prior rationale, or cross-check selection reason. Labels are joined only after source judgment. The blinding is procedural; its limits are stated under Limitations.

## Coverage and primary verdicts

| verdict | pairs | rate |
| --- | ---: | ---: |
| confirmed_gold | 389 | 65.709% |
| non_gold | 9 | 1.520% |
| absent | 21 | 3.547% |
| ambiguous | 173 | 29.223% |

| task | pairs | primary confirmed | primary agreement | flagged by either pass | union agreement |
| --- | ---: | ---: | ---: | ---: | ---: |
| scratch_minidb | 302 | 223 | 73.841% | 81 | 73.179% |
| scratch_nanoweb | 290 | 166 | 57.241% | 124 | 57.241% |

## Agreement by task and builder family

| task | builder family | pairs | primary confirmed | primary agreement | flagged by either pass | union agreement |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| scratch_minidb | claude | 119 | 102 | 85.714% | 17 | 85.714% |
| scratch_minidb | codex | 103 | 77 | 74.757% | 27 | 73.786% |
| scratch_minidb | mimo | 80 | 44 | 55.000% | 37 | 53.750% |
| scratch_nanoweb | claude | 116 | 75 | 64.655% | 41 | 64.655% |
| scratch_nanoweb | codex | 116 | 69 | 59.483% | 47 | 59.483% |
| scratch_nanoweb | mimo | 58 | 22 | 37.931% | 36 | 37.931% |

## Cross-check pass

The second pass reviewed **242** pairs: every primary flag, an exact 10% task × builder-family stratified sample of primary `confirmed_gold` pairs (rounded up once at the population level), and the frozen nine-key regression set. The fresh `gpt-5.6-terra` sessions received neither primary decisions nor selection reasons.

| selected pairs | same verdict as primary | verdict agreement |
| ---: | ---: | ---: |
| 242 | 155 | 64.050% |

`disagreements.csv` contains **205** unique pairs flagged by at least one pass. A cross-pass conflict remains a flag and is marked `author_adjudication_status=pending`.

## Known-error regression check

| task | artifact | question | expected mapped label | primary | cross-check | exact either | exact both |
| --- | --- | --- | --- | --- | --- | ---: | ---: |
| scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_low-t0-s20260501-51f333b3` | q_db_05 | implemented_non_gold | non_gold | non_gold | True | True |
| scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_low-t1-s20260502-07943f95` | q_db_02 | absent_or_ambiguous | ambiguous | absent | True | True |
| scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_high-t1-s20260502-44ad1601` | q_nw_07 | absent_or_ambiguous | ambiguous | absent | True | True |
| scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_low-t1-s20260502-0ab2ca9f` | q_nw_02 | absent_or_ambiguous | ambiguous | absent | True | True |
| scratch_nanoweb | `build-scratch_nanoweb-claude_code_opus_4_7_high-t0-s20260501-c329ce51` | q_nw_13 | absent_or_ambiguous | absent | absent | True | True |
| scratch_nanoweb | `build-scratch_nanoweb-claude_code_opus_4_7_high-t1-s20261502-374bc791` | q_nw_14 | absent_or_ambiguous | ambiguous | ambiguous | True | True |
| scratch_nanoweb | `build-scratch_nanoweb-claude_code_opus_4_7_low-t1-s20261502-ae88af88` | q_nw_11 | absent_or_ambiguous | ambiguous | ambiguous | True | True |
| scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t0-s20260501-1a57cf95` | q_nw_03 | absent_or_ambiguous | ambiguous | ambiguous | True | True |
| scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_5_low-t1-s20260502-ce7e6480` | q_nw_02 | absent_or_ambiguous | ambiguous | ambiguous | True | True |

All **9 / 9** known label errors were reproduced at the expected mapped-label level. The table shows, for each case, whether both model passes reproduced it.

## Label-sensitivity analysis

For sensitivity only, every unresolved full-audit flag is overlaid as a relabeling away from `implemented_gold`; this does not adjudicate the flags. These results are the July outputs in `label_sensitivity/`, which are not part of the public release; the paper reports the adjudicated-label analysis in `label_sensitivity_adjudicated/`.

| panel | baseline builder order | full-audit order | changed |
| --- | --- | --- | ---: |
| high_effort | codex_gpt5_5_high > codex_gpt5_4_mini_high > claude_code_opus_4_7_high > claude_code_sonnet_4_6_high > claude_code_mimo_v2_5_high > claude_code_mimo_v2_5_pro_high | codex_gpt5_5_high > codex_gpt5_4_mini_high > claude_code_opus_4_7_high > claude_code_sonnet_4_6_high > claude_code_mimo_v2_5_high > claude_code_mimo_v2_5_pro_high | false |
| low_effort | codex_gpt5_5_low > codex_gpt5_4_mini_low > claude_code_opus_4_7_low > claude_code_mimo_v2_5_pro_low > claude_code_sonnet_4_6_low > claude_code_mimo_v2_5_low | codex_gpt5_4_mini_low > codex_gpt5_5_low > claude_code_opus_4_7_low > claude_code_mimo_v2_5_pro_low > claude_code_sonnet_4_6_low > claude_code_mimo_v2_5_low | true |

| scenario | per-builder implementation coverage range |
| --- | ---: |
| baseline | 85.000%–100.000% |
| full-audit flags corrected | 35.000%–83.333% |

## Traceability and files

- `minidb_adjudication.csv` and `nanoweb_adjudication.csv`: one canonical row per pair, mirroring the spot-audit fields and adding spans plus primary/cross-check model and prompt provenance.
- `disagreements.csv`: union of all pairs flagged by either pass; no released label is changed.
- `evidence_excerpts.jsonl`: frozen line-numbered text for every cited span with source-file hashes.
- `raw/` (not part of the public release): append-only request, prompt, assignment, result, completion, and integrity-attestation records per session attempt.
- `integrity_attestation_manifest.json` (the core generation manifest records its hash): closed ledger of all 82 successful attempts, explicitly distinguishing completion-time from post-hoc result hashes.
- `CORE_GENERATION.json`: pointer to the stable canonical-data generation consumed by the sensitivity analysis; it excludes the report and sensitivity outputs. Its generation manifest is included.
- `generations/<content-hash>/` plus `CURRENT_GENERATION.json` (not part of the public release, except the core generation manifest): the immutable release bundle and its atomic publication pointer. Root-level CSV/report files are convenience aliases.
- `execution_manifest.json`, `crosscheck_sampling_manifest.json`, `validation_manifest.json`, and `output_manifest.json`: commands, hashes, row counts, selection rules, and validation results.

## Limitations

This is full coverage of the scoring pairs, but still model-assisted static source tracing. It does not estimate an error rate from the older 24-pair spot sample, and it does not elevate unresolved model flags to human-adjudicated ground truth. Dynamic dispatch or runtime configuration can still be misread; file/symbol/span records are provided so the author can adjudicate every flagged pair directly. The blinding is procedural: fresh no-history agents were instructed not to inspect labels or prior results, but the shared workspace remained technically readable and the collaboration runtime exposes no filesystem-access trace. Model ids are the exact parent dispatch requests, not separately attested runtime tokens. For 43 historical sessions, the result hash was appended and verified after completion rather than recorded at completion time; this provenance is explicit in every affected row.
