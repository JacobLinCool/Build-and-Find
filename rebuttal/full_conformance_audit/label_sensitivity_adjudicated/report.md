# Corrected-label sensitivity

This deterministic, post-hoc rebuttal sensitivity overlays cumulative
source-audited corrections in memory and does not modify `runs_release/`.

## Scenarios

| scenario | corrections | definition |
| --- | ---: | --- |
| baseline | 0 | released consensus labels with `runs_release/reports/artifact_conformance_consensus/final/label_corrections_v1_1.csv` applied |
| all_observed_corrected | 9 | 9 observed source-audit errors: 2 pilot corrections + 7 spot-audit disagreements |
| author_adjudicated | 48 | observed errors + 41 author-confirmed flags (2 already observed) = 48 distinct pairs; the 2 author-kept flags stay implemented_gold |
| fully_adjudicated | 195 | author-adjudicated pairs + 154 model-assisted relabels of the flags without an author decision (7 already observed) = 195 distinct pairs; the 8 model-kept flags stay implemented_gold. The model-assisted decisions were reviewed and approved by the author on 2026-10-16 |
| author_adjudicated_plus_unreviewed_flags | 203 | worst case: author-adjudicated pairs + all 162 flags without an author decision, each relabeled with its proposed label (7 already observed) = 203 distinct pairs |

The full-coverage audit flagged 205 of the 592 scored compile-pass pairs.
`rebuttal/full_conformance_audit/author_decisions.csv` records author decisions on the 43 flags for `codex_gpt5_4_mini_low` (25), `codex_gpt5_5_low` (18):
41 confirmed (39 absent_or_ambiguous, 2 implemented_non_gold) and 2 kept as implemented_gold (#7, #11).
The other 162 flags have no author decision;
the worst-case scenario relabels all of them as an extreme case, not as an
adjudication. No scenario modifies the released labels.

`rebuttal/full_conformance_audit/model_assisted_decisions.csv` records model-assisted decisions on those 162 flags
(model-assisted adjudication (Claude), two passes with arbitration; review status: author_approved, reviewed on 2026-10-16): 151 absent_or_ambiguous, 3 implemented_non_gold, 8 kept as implemented_gold.
The fully_adjudicated scenario applies them on top of the author-adjudicated set.
2 model-assisted labels differ from the proposed sensitivity label
that the worst case uses for the same pair; both labels exclude the pair from
the implemented_gold set.

## Author decisions

| item | task | artifact | question | flag basis | decision | decided | rationale |
| --- | --- | --- | --- | --- | --- | --- | --- |
| #1 | scratch_minidb | `build-scratch_minidb-codex_gpt5_4_mini_low-t1-s20260502-c57ac23c` | q_db_02 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #2 | scratch_minidb | `build-scratch_minidb-codex_gpt5_4_mini_low-t1-s20260502-c57ac23c` | q_db_03 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #3 | scratch_minidb | `build-scratch_minidb-codex_gpt5_4_mini_low-t1-s20260502-c57ac23c` | q_db_05 | both_passes | non_gold | author, 2026-07-31 | not recorded at decision time |
| #4 | scratch_minidb | `build-scratch_minidb-codex_gpt5_4_mini_low-t1-s20260502-c57ac23c` | q_db_06 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #5 | scratch_minidb | `build-scratch_minidb-codex_gpt5_4_mini_low-t1-s20260502-c57ac23c` | q_db_07 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #6 | scratch_minidb | `build-scratch_minidb-codex_gpt5_4_mini_low-t1-s20260502-c57ac23c` | q_db_08 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #7 | scratch_minidb | `build-scratch_minidb-codex_gpt5_4_mini_low-t1-s20260502-c57ac23c` | q_db_10 | primary_only | keep_implemented_gold | author, 2026-07-31 | not recorded at decision time |
| #8 | scratch_minidb | `build-scratch_minidb-codex_gpt5_4_mini_low-t1-s20260502-c57ac23c` | q_db_12 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #9 | scratch_minidb | `build-scratch_minidb-codex_gpt5_5_low-t0-s20260501-d0068f8c` | q_db_07 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #10 | scratch_minidb | `build-scratch_minidb-codex_gpt5_5_low-t0-s20260501-d0068f8c` | q_db_12 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #11 | scratch_minidb | `build-scratch_minidb-codex_gpt5_5_low-t0-s20260501-d0068f8c` | q_db_13 | primary_only | keep_implemented_gold | author, 2026-07-31 | not recorded at decision time |
| #12 | scratch_minidb | `build-scratch_minidb-codex_gpt5_5_low-t1-s20260502-4c36b45f` | q_db_03 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #13 | scratch_minidb | `build-scratch_minidb-codex_gpt5_5_low-t1-s20260502-4c36b45f` | q_db_05 | both_passes | non_gold | author, 2026-07-31 | not recorded at decision time |
| #14 | scratch_minidb | `build-scratch_minidb-codex_gpt5_5_low-t1-s20260502-4c36b45f` | q_db_07 | primary_only | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #15 | scratch_minidb | `build-scratch_minidb-codex_gpt5_5_low-t1-s20260502-4c36b45f` | q_db_08 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #16 | scratch_minidb | `build-scratch_minidb-codex_gpt5_5_low-t1-s20260502-4c36b45f` | q_db_12 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #17 | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t0-s20260501-1a57cf95` | q_nw_02 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #18 | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t0-s20260501-1a57cf95` | q_nw_03 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #19 | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t0-s20260501-1a57cf95` | q_nw_05 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #20 | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t0-s20260501-1a57cf95` | q_nw_07 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #21 | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t0-s20260501-1a57cf95` | q_nw_08 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #22 | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t0-s20260501-1a57cf95` | q_nw_09 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #23 | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t0-s20260501-1a57cf95` | q_nw_11 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #24 | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t0-s20260501-1a57cf95` | q_nw_14 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #25 | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t1-s20260502-8f79ca6d` | q_nw_02 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #26 | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t1-s20260502-8f79ca6d` | q_nw_03 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #27 | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t1-s20260502-8f79ca6d` | q_nw_05 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #28 | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t1-s20260502-8f79ca6d` | q_nw_07 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #29 | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t1-s20260502-8f79ca6d` | q_nw_08 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #30 | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t1-s20260502-8f79ca6d` | q_nw_10 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #31 | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t1-s20260502-8f79ca6d` | q_nw_11 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #32 | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t1-s20260502-8f79ca6d` | q_nw_12 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #33 | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t1-s20260502-8f79ca6d` | q_nw_14 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #34 | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_5_low-t0-s20260501-6a64601d` | q_nw_02 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #35 | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_5_low-t0-s20260501-6a64601d` | q_nw_06 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #36 | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_5_low-t0-s20260501-6a64601d` | q_nw_11 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #37 | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_5_low-t0-s20260501-6a64601d` | q_nw_12 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #38 | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_5_low-t0-s20260501-6a64601d` | q_nw_13 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #39 | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_5_low-t0-s20260501-6a64601d` | q_nw_14 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #40 | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_5_low-t1-s20260502-ce7e6480` | q_nw_02 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #41 | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_5_low-t1-s20260502-ce7e6480` | q_nw_07 | primary_only | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #42 | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_5_low-t1-s20260502-ce7e6480` | q_nw_11 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |
| #43 | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_5_low-t1-s20260502-ce7e6480` | q_nw_14 | both_passes | absent_or_ambiguous | author, 2026-07-31 | not recorded at decision time |

## Model-assisted decisions

Per-pair decisions, evidence, and rationales are in `rebuttal/full_conformance_audit/model_assisted_decisions.csv`; relabeled pairs also appear in
`corrections.csv`. Review status: `author_approved` (reviewed on 2026-10-16).

| decision | confidence | pass agreement | pairs |
| --- | --- | --- | ---: |
| absent_or_ambiguous | high | agree | 112 |
| absent_or_ambiguous | high | arbitrated | 3 |
| absent_or_ambiguous | medium | agree | 30 |
| absent_or_ambiguous | medium | arbitrated | 3 |
| absent_or_ambiguous | low | agree | 3 |
| keep_implemented_gold | medium | agree | 7 |
| keep_implemented_gold | medium | arbitrated | 1 |
| non_gold | high | agree | 1 |
| non_gold | medium | agree | 2 |

Model-kept pairs (stay implemented_gold in every scenario except the worst case):

| task | artifact | question | option | confidence | pass agreement |
| --- | --- | --- | --- | --- | --- |
| scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_pro_high-t0-s20260501-291ba58c` | q_db_01 | B | medium | agree |
| scratch_minidb | `build-scratch_minidb-codex_gpt5_4_mini_high-t1-s20260502-05500a40` | q_db_01 | B | medium | agree |
| scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_low-t1-s20260502-07943f95` | q_db_08 | B | medium | agree |
| scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_low-t1-s20260502-07943f95` | q_db_09 | B | medium | arbitrated |
| scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_high-t0-s20260501-f80bd4f0` | q_nw_01 | B | medium | agree |
| scratch_nanoweb | `build-scratch_nanoweb-claude_code_opus_4_7_high-t0-s20260501-c329ce51` | q_nw_03 | C | medium | agree |
| scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_5_high-t1-s20260502-a5df8a82` | q_nw_07 | B | medium | agree |
| scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_pro_low-t1-s20260502-e516fe14` | q_nw_08 | A | medium | agree |

## Correction validation

| provenance | confidence | task | artifact | question | old → new | primary correct / rows | low-prior |
| --- | --- | --- | --- | --- | --- | ---: | ---: |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_high-t0-s20260501-acb184a1` | q_db_01 | implemented_gold → absent_or_ambiguous | 27 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_high-t0-s20260501-acb184a1` | q_db_03 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_high-t0-s20260501-acb184a1` | q_db_06 | implemented_gold → absent_or_ambiguous | 36 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_high-t0-s20260501-acb184a1` | q_db_07 | implemented_gold → absent_or_ambiguous | 35 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_high-t0-s20260501-acb184a1` | q_db_08 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_medium | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_high-t0-s20260501-acb184a1` | q_db_09 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_high-t0-s20260501-acb184a1` | q_db_12 | implemented_gold → absent_or_ambiguous | 34 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_high-t1-s20260502-550d8811` | q_db_02 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_high-t1-s20260502-550d8811` | q_db_03 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_high-t1-s20260502-550d8811` | q_db_05 | implemented_gold → implemented_non_gold | 34 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_high-t1-s20260502-550d8811` | q_db_06 | implemented_gold → absent_or_ambiguous | 22 / 36 | 1 |
| full_conformance_audit_crosscheck_pass|model_assisted_adjudication | model_assisted_medium | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_high-t1-s20260502-550d8811` | q_db_09 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_medium | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_low-t0-s20260501-51f333b3` | q_db_01 | implemented_gold → absent_or_ambiguous | 35 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_medium | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_low-t0-s20260501-51f333b3` | q_db_02 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_low-t0-s20260501-51f333b3` | q_db_03 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| pilot_source_audit|full_conformance_audit_both_passes|model_assisted_adjudication | source_confirmed|model_assisted_medium | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_low-t0-s20260501-51f333b3` | q_db_05 | implemented_gold → implemented_non_gold | 36 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_low-t0-s20260501-51f333b3` | q_db_08 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_low-t0-s20260501-51f333b3` | q_db_13 | implemented_gold → absent_or_ambiguous | 33 / 36 | 0 |
| spot_audit_sample_09|full_conformance_audit_both_passes|model_assisted_adjudication | medium|model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_low-t1-s20260502-07943f95` | q_db_02 | implemented_gold → absent_or_ambiguous | 32 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_low-t1-s20260502-07943f95` | q_db_03 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_low-t1-s20260502-07943f95` | q_db_06 | implemented_gold → absent_or_ambiguous | 25 / 36 | 1 |
| full_conformance_audit_primary_pass|model_assisted_adjudication | model_assisted_medium | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_low-t1-s20260502-07943f95` | q_db_07 | implemented_gold → absent_or_ambiguous | 36 / 36 | 1 |
| full_conformance_audit_primary_pass | high | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_low-t1-s20260502-07943f95` | q_db_08 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_primary_pass | high | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_low-t1-s20260502-07943f95` | q_db_09 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_primary_pass|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_low-t1-s20260502-07943f95` | q_db_10 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_low-t1-s20260502-07943f95` | q_db_12 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_low-t1-s20260502-07943f95` | q_db_13 | implemented_gold → absent_or_ambiguous | 35 / 36 | 0 |
| full_conformance_audit_primary_pass | high | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_pro_high-t0-s20260501-291ba58c` | q_db_01 | implemented_gold → absent_or_ambiguous | 35 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_pro_high-t0-s20260501-291ba58c` | q_db_02 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_pro_high-t0-s20260501-291ba58c` | q_db_03 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_medium | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_pro_high-t0-s20260501-291ba58c` | q_db_06 | implemented_gold → absent_or_ambiguous | 36 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_pro_high-t0-s20260501-291ba58c` | q_db_10 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_pro_high-t0-s20260501-291ba58c` | q_db_13 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_pro_high-t1-s20260502-b6e82435` | q_db_03 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_medium | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_pro_high-t1-s20260502-b6e82435` | q_db_06 | implemented_gold → absent_or_ambiguous | 36 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_pro_high-t1-s20260502-b6e82435` | q_db_07 | implemented_gold → absent_or_ambiguous | 36 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_medium | scratch_minidb | `build-scratch_minidb-claude_code_mimo_v2_5_pro_high-t1-s20260502-b6e82435` | q_db_13 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_medium | scratch_minidb | `build-scratch_minidb-claude_code_opus_4_7_high-t1-s20260502-83f0c8f0` | q_db_02 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_primary_pass|model_assisted_adjudication | model_assisted_medium | scratch_minidb | `build-scratch_minidb-claude_code_opus_4_7_high-t1-s20260502-83f0c8f0` | q_db_06 | implemented_gold → absent_or_ambiguous | 36 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_opus_4_7_low-t0-s20260501-cdbba75d` | q_db_06 | implemented_gold → absent_or_ambiguous | 30 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_opus_4_7_low-t0-s20260501-cdbba75d` | q_db_07 | implemented_gold → absent_or_ambiguous | 35 / 36 | 1 |
| full_conformance_audit_primary_pass|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_opus_4_7_low-t1-s20260502-5846c7eb` | q_db_07 | implemented_gold → absent_or_ambiguous | 35 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_medium | scratch_minidb | `build-scratch_minidb-claude_code_sonnet_4_6_high-t0-s20260501-aa19a4dc` | q_db_01 | implemented_gold → absent_or_ambiguous | 35 / 36 | 1 |
| full_conformance_audit_primary_pass|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_sonnet_4_6_high-t0-s20260501-aa19a4dc` | q_db_07 | implemented_gold → absent_or_ambiguous | 33 / 36 | 1 |
| full_conformance_audit_primary_pass|model_assisted_adjudication | model_assisted_medium | scratch_minidb | `build-scratch_minidb-claude_code_sonnet_4_6_high-t0-s20260501-aa19a4dc` | q_db_13 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_medium | scratch_minidb | `build-scratch_minidb-claude_code_sonnet_4_6_high-t1-s20260502-bd0b6695` | q_db_01 | implemented_gold → absent_or_ambiguous | 36 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_medium | scratch_minidb | `build-scratch_minidb-claude_code_sonnet_4_6_high-t1-s20260502-bd0b6695` | q_db_02 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_sonnet_4_6_high-t1-s20260502-bd0b6695` | q_db_03 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_medium | scratch_minidb | `build-scratch_minidb-claude_code_sonnet_4_6_high-t1-s20260502-bd0b6695` | q_db_05 | implemented_gold → absent_or_ambiguous | 29 / 36 | 1 |
| full_conformance_audit_both_passes | high | scratch_minidb | `build-scratch_minidb-claude_code_sonnet_4_6_high-t1-s20260502-bd0b6695` | q_db_05 | implemented_gold → implemented_non_gold | 29 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_sonnet_4_6_high-t1-s20260502-bd0b6695` | q_db_13 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_sonnet_4_6_low-t0-s20260501-dd444f70` | q_db_03 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_sonnet_4_6_low-t0-s20260501-dd444f70` | q_db_13 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_sonnet_4_6_low-t1-s20260502-0690460b` | q_db_07 | implemented_gold → absent_or_ambiguous | 36 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-claude_code_sonnet_4_6_low-t1-s20260502-0690460b` | q_db_12 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_medium | scratch_minidb | `build-scratch_minidb-codex_gpt5_4_mini_high-t0-s20260501-7ba82072` | q_db_05 | implemented_gold → absent_or_ambiguous | 34 / 36 | 1 |
| full_conformance_audit_both_passes | high | scratch_minidb | `build-scratch_minidb-codex_gpt5_4_mini_high-t0-s20260501-7ba82072` | q_db_05 | implemented_gold → implemented_non_gold | 34 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_medium | scratch_minidb | `build-scratch_minidb-codex_gpt5_4_mini_high-t0-s20260501-7ba82072` | q_db_06 | implemented_gold → absent_or_ambiguous | 33 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-codex_gpt5_4_mini_high-t0-s20260501-7ba82072` | q_db_07 | implemented_gold → absent_or_ambiguous | 30 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_medium | scratch_minidb | `build-scratch_minidb-codex_gpt5_4_mini_high-t0-s20260501-7ba82072` | q_db_12 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_crosscheck_pass | high | scratch_minidb | `build-scratch_minidb-codex_gpt5_4_mini_high-t1-s20260502-05500a40` | q_db_01 | implemented_gold → absent_or_ambiguous | 34 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-codex_gpt5_4_mini_high-t1-s20260502-05500a40` | q_db_12 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_minidb | `build-scratch_minidb-codex_gpt5_4_mini_low-t1-s20260502-c57ac23c` | q_db_02 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_minidb | `build-scratch_minidb-codex_gpt5_4_mini_low-t1-s20260502-c57ac23c` | q_db_03 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_minidb | `build-scratch_minidb-codex_gpt5_4_mini_low-t1-s20260502-c57ac23c` | q_db_05 | implemented_gold → implemented_non_gold | 36 / 36 | 1 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_minidb | `build-scratch_minidb-codex_gpt5_4_mini_low-t1-s20260502-c57ac23c` | q_db_06 | implemented_gold → absent_or_ambiguous | 36 / 36 | 1 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_minidb | `build-scratch_minidb-codex_gpt5_4_mini_low-t1-s20260502-c57ac23c` | q_db_07 | implemented_gold → absent_or_ambiguous | 36 / 36 | 1 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_minidb | `build-scratch_minidb-codex_gpt5_4_mini_low-t1-s20260502-c57ac23c` | q_db_08 | implemented_gold → absent_or_ambiguous | 28 / 36 | 0 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_minidb | `build-scratch_minidb-codex_gpt5_4_mini_low-t1-s20260502-c57ac23c` | q_db_12 | implemented_gold → absent_or_ambiguous | 35 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-codex_gpt5_5_high-t0-s20260501-1924cec4` | q_db_06 | implemented_gold → absent_or_ambiguous | 22 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-codex_gpt5_5_high-t0-s20260501-1924cec4` | q_db_12 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_medium | scratch_minidb | `build-scratch_minidb-codex_gpt5_5_high-t1-s20260502-0c54a173` | q_db_07 | implemented_gold → absent_or_ambiguous | 34 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-codex_gpt5_5_high-t1-s20260502-0c54a173` | q_db_08 | implemented_gold → absent_or_ambiguous | 32 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_minidb | `build-scratch_minidb-codex_gpt5_5_high-t1-s20260502-0c54a173` | q_db_12 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_minidb | `build-scratch_minidb-codex_gpt5_5_low-t0-s20260501-d0068f8c` | q_db_07 | implemented_gold → absent_or_ambiguous | 35 / 36 | 1 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_minidb | `build-scratch_minidb-codex_gpt5_5_low-t0-s20260501-d0068f8c` | q_db_12 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_minidb | `build-scratch_minidb-codex_gpt5_5_low-t1-s20260502-4c36b45f` | q_db_03 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_minidb | `build-scratch_minidb-codex_gpt5_5_low-t1-s20260502-4c36b45f` | q_db_05 | implemented_gold → implemented_non_gold | 35 / 36 | 1 |
| full_conformance_audit_primary_pass|author_adjudication | author_confirmed | scratch_minidb | `build-scratch_minidb-codex_gpt5_5_low-t1-s20260502-4c36b45f` | q_db_07 | implemented_gold → absent_or_ambiguous | 36 / 36 | 1 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_minidb | `build-scratch_minidb-codex_gpt5_5_low-t1-s20260502-4c36b45f` | q_db_08 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_minidb | `build-scratch_minidb-codex_gpt5_5_low-t1-s20260502-4c36b45f` | q_db_12 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_primary_pass | high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_high-t0-s20260501-f80bd4f0` | q_nw_01 | implemented_gold → absent_or_ambiguous | 36 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_high-t0-s20260501-f80bd4f0` | q_nw_02 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_primary_pass|model_assisted_adjudication | model_assisted_medium | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_high-t0-s20260501-f80bd4f0` | q_nw_03 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_primary_pass|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_high-t0-s20260501-f80bd4f0` | q_nw_05 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_medium | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_high-t0-s20260501-f80bd4f0` | q_nw_06 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_high-t0-s20260501-f80bd4f0` | q_nw_07 | implemented_gold → absent_or_ambiguous | 36 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_high-t0-s20260501-f80bd4f0` | q_nw_11 | implemented_gold → absent_or_ambiguous | 34 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_low | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_high-t0-s20260501-f80bd4f0` | q_nw_12 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_medium | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_high-t0-s20260501-f80bd4f0` | q_nw_13 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_high-t0-s20260501-f80bd4f0` | q_nw_14 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_high-t1-s20260502-44ad1601` | q_nw_02 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_primary_pass|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_high-t1-s20260502-44ad1601` | q_nw_03 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_medium | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_high-t1-s20260502-44ad1601` | q_nw_05 | implemented_gold → absent_or_ambiguous | 29 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_high-t1-s20260502-44ad1601` | q_nw_06 | implemented_gold → absent_or_ambiguous | 35 / 36 | 0 |
| pilot_source_audit|full_conformance_audit_both_passes|model_assisted_adjudication | source_confirmed|model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_high-t1-s20260502-44ad1601` | q_nw_07 | implemented_gold → absent_or_ambiguous | 36 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_high-t1-s20260502-44ad1601` | q_nw_08 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_high-t1-s20260502-44ad1601` | q_nw_11 | implemented_gold → absent_or_ambiguous | 29 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_high-t1-s20260502-44ad1601` | q_nw_13 | implemented_gold → absent_or_ambiguous | 34 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_high-t1-s20260502-44ad1601` | q_nw_14 | implemented_gold → absent_or_ambiguous | 35 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_high-t1-s20260502-44ad1601` | q_nw_15 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| spot_audit_sample_22|full_conformance_audit_both_passes|model_assisted_adjudication | high|model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_low-t1-s20260502-0ab2ca9f` | q_nw_02 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_low-t1-s20260502-0ab2ca9f` | q_nw_03 | implemented_gold → absent_or_ambiguous | 34 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_low-t1-s20260502-0ab2ca9f` | q_nw_05 | implemented_gold → absent_or_ambiguous | 30 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_low-t1-s20260502-0ab2ca9f` | q_nw_06 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_low-t1-s20260502-0ab2ca9f` | q_nw_07 | implemented_gold → absent_or_ambiguous | 36 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_low-t1-s20260502-0ab2ca9f` | q_nw_11 | implemented_gold → absent_or_ambiguous | 30 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_low-t1-s20260502-0ab2ca9f` | q_nw_13 | implemented_gold → absent_or_ambiguous | 34 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_low-t1-s20260502-0ab2ca9f` | q_nw_14 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_pro_low-t1-s20260502-e516fe14` | q_nw_02 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_medium | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_pro_low-t1-s20260502-e516fe14` | q_nw_06 | implemented_gold → absent_or_ambiguous | 32 / 36 | 0 |
| full_conformance_audit_primary_pass | high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_pro_low-t1-s20260502-e516fe14` | q_nw_08 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_primary_pass|model_assisted_adjudication | model_assisted_medium | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_pro_low-t1-s20260502-e516fe14` | q_nw_09 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_pro_low-t1-s20260502-e516fe14` | q_nw_11 | implemented_gold → absent_or_ambiguous | 36 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_pro_low-t1-s20260502-e516fe14` | q_nw_12 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_pro_low-t1-s20260502-e516fe14` | q_nw_13 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_mimo_v2_5_pro_low-t1-s20260502-e516fe14` | q_nw_14 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_opus_4_7_high-t0-s20260501-c329ce51` | q_nw_02 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_primary_pass | high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_opus_4_7_high-t0-s20260501-c329ce51` | q_nw_03 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_medium | scratch_nanoweb | `build-scratch_nanoweb-claude_code_opus_4_7_high-t0-s20260501-c329ce51` | q_nw_06 | implemented_gold → implemented_non_gold | 33 / 36 | 0 |
| full_conformance_audit_primary_pass|model_assisted_adjudication | model_assisted_medium | scratch_nanoweb | `build-scratch_nanoweb-claude_code_opus_4_7_high-t0-s20260501-c329ce51` | q_nw_07 | implemented_gold → absent_or_ambiguous | 36 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_medium | scratch_nanoweb | `build-scratch_nanoweb-claude_code_opus_4_7_high-t0-s20260501-c329ce51` | q_nw_08 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_medium | scratch_nanoweb | `build-scratch_nanoweb-claude_code_opus_4_7_high-t0-s20260501-c329ce51` | q_nw_11 | implemented_gold → absent_or_ambiguous | 32 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_medium | scratch_nanoweb | `build-scratch_nanoweb-claude_code_opus_4_7_high-t0-s20260501-c329ce51` | q_nw_12 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| spot_audit_sample_14|full_conformance_audit_both_passes|model_assisted_adjudication | high|model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_opus_4_7_high-t0-s20260501-c329ce51` | q_nw_13 | implemented_gold → absent_or_ambiguous | 35 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_opus_4_7_high-t0-s20260501-c329ce51` | q_nw_14 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_opus_4_7_high-t1-s20261502-374bc791` | q_nw_02 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| spot_audit_sample_15|full_conformance_audit_both_passes|model_assisted_adjudication | high|model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_opus_4_7_high-t1-s20261502-374bc791` | q_nw_14 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_opus_4_7_low-t0-s20261501-636b758c` | q_nw_02 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_opus_4_7_low-t0-s20261501-636b758c` | q_nw_11 | implemented_gold → absent_or_ambiguous | 36 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_opus_4_7_low-t0-s20261501-636b758c` | q_nw_14 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_opus_4_7_low-t0-s20261501-636b758c` | q_nw_15 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_opus_4_7_low-t1-s20261502-ae88af88` | q_nw_02 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| spot_audit_sample_16|full_conformance_audit_both_passes|model_assisted_adjudication | high|model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_opus_4_7_low-t1-s20261502-ae88af88` | q_nw_11 | implemented_gold → absent_or_ambiguous | 29 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_opus_4_7_low-t1-s20261502-ae88af88` | q_nw_14 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_sonnet_4_6_high-t0-s20261501-05d5b610` | q_nw_02 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_sonnet_4_6_high-t0-s20261501-05d5b610` | q_nw_05 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_sonnet_4_6_high-t0-s20261501-05d5b610` | q_nw_11 | implemented_gold → absent_or_ambiguous | 35 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_sonnet_4_6_high-t0-s20261501-05d5b610` | q_nw_14 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_sonnet_4_6_high-t1-s20261502-f19379b3` | q_nw_02 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_sonnet_4_6_high-t1-s20261502-f19379b3` | q_nw_05 | implemented_gold → absent_or_ambiguous | 35 / 36 | 0 |
| full_conformance_audit_primary_pass|model_assisted_adjudication | model_assisted_medium | scratch_nanoweb | `build-scratch_nanoweb-claude_code_sonnet_4_6_high-t1-s20261502-f19379b3` | q_nw_07 | implemented_gold → absent_or_ambiguous | 36 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_sonnet_4_6_high-t1-s20261502-f19379b3` | q_nw_11 | implemented_gold → absent_or_ambiguous | 34 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_sonnet_4_6_high-t1-s20261502-f19379b3` | q_nw_14 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_sonnet_4_6_low-t0-s20261501-e6047538` | q_nw_02 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_sonnet_4_6_low-t0-s20261501-e6047538` | q_nw_05 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_sonnet_4_6_low-t0-s20261501-e6047538` | q_nw_06 | implemented_gold → absent_or_ambiguous | 33 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_sonnet_4_6_low-t0-s20261501-e6047538` | q_nw_11 | implemented_gold → absent_or_ambiguous | 35 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_sonnet_4_6_low-t0-s20261501-e6047538` | q_nw_12 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_sonnet_4_6_low-t0-s20261501-e6047538` | q_nw_13 | implemented_gold → absent_or_ambiguous | 32 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_sonnet_4_6_low-t0-s20261501-e6047538` | q_nw_14 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_sonnet_4_6_low-t1-s20261502-8328c0da` | q_nw_02 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_sonnet_4_6_low-t1-s20261502-8328c0da` | q_nw_03 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_sonnet_4_6_low-t1-s20261502-8328c0da` | q_nw_07 | implemented_gold → absent_or_ambiguous | 36 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_sonnet_4_6_low-t1-s20261502-8328c0da` | q_nw_08 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_sonnet_4_6_low-t1-s20261502-8328c0da` | q_nw_11 | implemented_gold → absent_or_ambiguous | 34 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_medium | scratch_nanoweb | `build-scratch_nanoweb-claude_code_sonnet_4_6_low-t1-s20261502-8328c0da` | q_nw_12 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-claude_code_sonnet_4_6_low-t1-s20261502-8328c0da` | q_nw_14 | implemented_gold → absent_or_ambiguous | 35 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_medium | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_high-t0-s20260501-aa87757d` | q_nw_02 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_high-t0-s20260501-aa87757d` | q_nw_07 | implemented_gold → absent_or_ambiguous | 36 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_high-t0-s20260501-aa87757d` | q_nw_11 | implemented_gold → absent_or_ambiguous | 35 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_low | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_high-t0-s20260501-aa87757d` | q_nw_12 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_high-t0-s20260501-aa87757d` | q_nw_14 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_high-t0-s20260501-aa87757d` | q_nw_15 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_low | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_high-t1-s20260502-e25932eb` | q_nw_02 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_medium | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_high-t1-s20260502-e25932eb` | q_nw_05 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_high-t1-s20260502-e25932eb` | q_nw_11 | implemented_gold → absent_or_ambiguous | 36 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_high-t1-s20260502-e25932eb` | q_nw_14 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_high-t1-s20260502-e25932eb` | q_nw_15 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t0-s20260501-1a57cf95` | q_nw_02 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| spot_audit_sample_17|full_conformance_audit_both_passes|author_adjudication | high|author_confirmed | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t0-s20260501-1a57cf95` | q_nw_03 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t0-s20260501-1a57cf95` | q_nw_05 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t0-s20260501-1a57cf95` | q_nw_07 | implemented_gold → absent_or_ambiguous | 36 / 36 | 1 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t0-s20260501-1a57cf95` | q_nw_08 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t0-s20260501-1a57cf95` | q_nw_09 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t0-s20260501-1a57cf95` | q_nw_11 | implemented_gold → absent_or_ambiguous | 36 / 36 | 1 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t0-s20260501-1a57cf95` | q_nw_14 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t1-s20260502-8f79ca6d` | q_nw_02 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t1-s20260502-8f79ca6d` | q_nw_03 | implemented_gold → absent_or_ambiguous | 34 / 36 | 0 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t1-s20260502-8f79ca6d` | q_nw_05 | implemented_gold → absent_or_ambiguous | 35 / 36 | 0 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t1-s20260502-8f79ca6d` | q_nw_07 | implemented_gold → absent_or_ambiguous | 36 / 36 | 1 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t1-s20260502-8f79ca6d` | q_nw_08 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t1-s20260502-8f79ca6d` | q_nw_10 | implemented_gold → absent_or_ambiguous | 35 / 36 | 1 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t1-s20260502-8f79ca6d` | q_nw_11 | implemented_gold → absent_or_ambiguous | 34 / 36 | 1 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t1-s20260502-8f79ca6d` | q_nw_12 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t1-s20260502-8f79ca6d` | q_nw_14 | implemented_gold → absent_or_ambiguous | 35 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_5_high-t0-s20260501-b54f0634` | q_nw_02 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_5_high-t0-s20260501-b54f0634` | q_nw_11 | implemented_gold → absent_or_ambiguous | 36 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_5_high-t0-s20260501-b54f0634` | q_nw_14 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_5_high-t0-s20260501-b54f0634` | q_nw_15 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_5_high-t1-s20260502-a5df8a82` | q_nw_02 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_primary_pass | high | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_5_high-t1-s20260502-a5df8a82` | q_nw_07 | implemented_gold → absent_or_ambiguous | 36 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_5_high-t1-s20260502-a5df8a82` | q_nw_11 | implemented_gold → absent_or_ambiguous | 36 / 36 | 1 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_5_high-t1-s20260502-a5df8a82` | q_nw_14 | implemented_gold → absent_or_ambiguous | 35 / 36 | 0 |
| full_conformance_audit_both_passes|model_assisted_adjudication | model_assisted_high | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_5_high-t1-s20260502-a5df8a82` | q_nw_15 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_5_low-t0-s20260501-6a64601d` | q_nw_02 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_5_low-t0-s20260501-6a64601d` | q_nw_06 | implemented_gold → absent_or_ambiguous | 33 / 36 | 0 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_5_low-t0-s20260501-6a64601d` | q_nw_11 | implemented_gold → absent_or_ambiguous | 36 / 36 | 1 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_5_low-t0-s20260501-6a64601d` | q_nw_12 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_5_low-t0-s20260501-6a64601d` | q_nw_13 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_5_low-t0-s20260501-6a64601d` | q_nw_14 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| spot_audit_sample_20|full_conformance_audit_both_passes|author_adjudication | high|author_confirmed | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_5_low-t1-s20260502-ce7e6480` | q_nw_02 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |
| full_conformance_audit_primary_pass|author_adjudication | author_confirmed | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_5_low-t1-s20260502-ce7e6480` | q_nw_07 | implemented_gold → absent_or_ambiguous | 36 / 36 | 1 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_5_low-t1-s20260502-ce7e6480` | q_nw_11 | implemented_gold → absent_or_ambiguous | 36 / 36 | 1 |
| full_conformance_audit_both_passes|author_adjudication | author_confirmed | scratch_nanoweb | `build-scratch_nanoweb-codex_gpt5_5_low-t1-s20260502-ce7e6480` | q_nw_14 | implemented_gold → absent_or_ambiguous | 36 / 36 | 0 |

Every key resolves uniquely to one released `implemented_gold` label and
exactly 36 primary rows (12 finders × 3 trials). The 24 legacy spot-audit
rows match their sample manifest exactly; full-audit flags have unique
keys, pass provenance, rationales, and validated sensitivity labels.
Each author decision resolves to exactly one full-audit flag and
repeats that flag's basis; author-kept pairs appear in no scenario.
Each model-assisted decision resolves to exactly one flag without an
author decision and repeats its basis; with the author decisions they
cover every flag. A pair whose fully-adjudicated label differs from its
worst-case label has one validation row per label.

## Label and answer counts

`artifact_present_all` covers all 720 released artifact-question pairs;
`compile_pass_primary` is the 615-pair primary analysis subset before
correction. Its 592 released `implemented_gold` pairs are the scored
pairs; their released label sources are:

| label source | scored pairs |
| --- | ---: |
| consensus_prelabel | 589 |
| manual_review | 3 |

| scenario | scope | label | pairs | answer rows | correct | answer accuracy |
| --- | --- | --- | ---: | ---: | ---: | ---: |
| baseline | artifact_present_all | implemented_gold | 672 | 24192 | 23928 | 98.909% |
| baseline | artifact_present_all | implemented_non_gold | 28 | 1008 | 399 | 39.583% |
| baseline | artifact_present_all | absent_or_ambiguous | 20 | 720 | 654 | 90.833% |
| baseline | compile_pass_primary | implemented_gold | 592 | 21312 | 21084 | 98.930% |
| baseline | compile_pass_primary | implemented_non_gold | 19 | 684 | 232 | 33.918% |
| baseline | compile_pass_primary | absent_or_ambiguous | 4 | 144 | 97 | 67.361% |
| all_observed_corrected | artifact_present_all | implemented_gold | 663 | 23868 | 23616 | 98.944% |
| all_observed_corrected | artifact_present_all | implemented_non_gold | 29 | 1044 | 435 | 41.667% |
| all_observed_corrected | artifact_present_all | absent_or_ambiguous | 28 | 1008 | 930 | 92.262% |
| all_observed_corrected | compile_pass_primary | implemented_gold | 583 | 20988 | 20772 | 98.971% |
| all_observed_corrected | compile_pass_primary | implemented_non_gold | 20 | 720 | 268 | 37.222% |
| all_observed_corrected | compile_pass_primary | absent_or_ambiguous | 12 | 432 | 373 | 86.343% |
| author_adjudicated | artifact_present_all | implemented_gold | 624 | 22464 | 22233 | 98.972% |
| author_adjudicated | artifact_present_all | implemented_non_gold | 31 | 1116 | 506 | 45.341% |
| author_adjudicated | artifact_present_all | absent_or_ambiguous | 65 | 2340 | 2242 | 95.812% |
| author_adjudicated | compile_pass_primary | implemented_gold | 544 | 19584 | 19389 | 99.004% |
| author_adjudicated | compile_pass_primary | implemented_non_gold | 22 | 792 | 339 | 42.803% |
| author_adjudicated | compile_pass_primary | absent_or_ambiguous | 49 | 1764 | 1685 | 95.522% |
| fully_adjudicated | artifact_present_all | implemented_gold | 477 | 17172 | 17099 | 99.575% |
| fully_adjudicated | artifact_present_all | implemented_non_gold | 33 | 1188 | 573 | 48.232% |
| fully_adjudicated | artifact_present_all | absent_or_ambiguous | 210 | 7560 | 7309 | 96.680% |
| fully_adjudicated | compile_pass_primary | implemented_gold | 397 | 14292 | 14255 | 99.741% |
| fully_adjudicated | compile_pass_primary | implemented_non_gold | 24 | 864 | 406 | 46.991% |
| fully_adjudicated | compile_pass_primary | absent_or_ambiguous | 194 | 6984 | 6752 | 96.678% |
| author_adjudicated_plus_unreviewed_flags | artifact_present_all | implemented_gold | 469 | 16884 | 16814 | 99.585% |
| author_adjudicated_plus_unreviewed_flags | artifact_present_all | implemented_non_gold | 35 | 1260 | 636 | 50.476% |
| author_adjudicated_plus_unreviewed_flags | artifact_present_all | absent_or_ambiguous | 216 | 7776 | 7531 | 96.849% |
| author_adjudicated_plus_unreviewed_flags | compile_pass_primary | implemented_gold | 389 | 14004 | 13970 | 99.757% |
| author_adjudicated_plus_unreviewed_flags | compile_pass_primary | implemented_non_gold | 26 | 936 | 469 | 50.107% |
| author_adjudicated_plus_unreviewed_flags | compile_pass_primary | absent_or_ambiguous | 200 | 7200 | 6974 | 96.861% |

## Relabeled scored pairs

Scored compile-pass pairs that each scenario relabels away from
`implemented_gold`, with the primary answer rows on those pairs and the rows
whose selected option is the specification gold option.

| scenario | relabeled pairs | absent_or_ambiguous | implemented_non_gold | answer rows | gold option chosen |
| --- | ---: | ---: | ---: | ---: | ---: |
| all_observed_corrected | 9 | 8 | 1 | 324 | 312 (96.3%) |
| author_adjudicated | 48 | 45 | 3 | 1728 | 1695 (98.1%) |
| fully_adjudicated | 195 | 190 | 5 | 7020 | 6829 (97.3%) |
| author_adjudicated_plus_unreviewed_flags | 203 | 196 | 7 | 7308 | 7114 (97.3%) |

Questions with relabeled pairs under `fully_adjudicated`, most relabeled first.
Cumulative counts the relabeled pairs of a row and every row above it;
`question_labels.csv` has every question in every scenario.

| task | question | compile-pass artifacts | implemented_gold (baseline → scenario) | relabeled | cumulative |
| --- | --- | ---: | ---: | ---: | ---: |
| scratch_nanoweb | q_nw_02 | 20 | 20 → 0 | 20 | 20 |
| scratch_nanoweb | q_nw_14 | 20 | 20 → 0 | 20 | 40 |
| scratch_nanoweb | q_nw_11 | 20 | 20 → 1 | 19 | 59 |
| scratch_minidb | q_db_07 | 21 | 20 → 8 | 12 | 71 |
| scratch_minidb | q_db_03 | 21 | 21 → 11 | 10 | 81 |
| scratch_minidb | q_db_06 | 21 | 20 → 10 | 10 | 91 |
| scratch_minidb | q_db_12 | 21 | 21 → 11 | 10 | 101 |
| scratch_nanoweb | q_nw_07 | 20 | 20 → 10 | 10 | 111 |
| scratch_nanoweb | q_nw_05 | 20 | 20 → 11 | 9 | 120 |
| scratch_nanoweb | q_nw_12 | 20 | 20 → 12 | 8 | 128 |
| scratch_minidb | q_db_02 | 21 | 21 → 14 | 7 | 135 |
| scratch_minidb | q_db_13 | 21 | 18 → 11 | 7 | 142 |
| scratch_nanoweb | q_nw_06 | 20 | 19 → 12 | 7 | 149 |
| scratch_nanoweb | q_nw_13 | 20 | 19 → 12 | 7 | 156 |
| scratch_minidb | q_db_05 | 21 | 18 → 12 | 6 | 162 |
| scratch_nanoweb | q_nw_03 | 20 | 20 → 14 | 6 | 168 |
| scratch_nanoweb | q_nw_15 | 20 | 13 → 7 | 6 | 174 |
| scratch_minidb | q_db_08 | 21 | 20 → 15 | 5 | 179 |
| scratch_nanoweb | q_nw_08 | 20 | 19 → 14 | 5 | 184 |
| scratch_minidb | q_db_01 | 21 | 20 → 16 | 4 | 188 |
| scratch_minidb | q_db_09 | 21 | 21 → 19 | 2 | 190 |
| scratch_minidb | q_db_10 | 21 | 21 → 19 | 2 | 192 |
| scratch_nanoweb | q_nw_09 | 20 | 20 → 18 | 2 | 194 |
| scratch_nanoweb | q_nw_10 | 20 | 20 → 19 | 1 | 195 |

## Implementation coverage

Only entities whose artifact-present coverage differs from baseline are
shown below; the full artifact and builder tables for both scopes are in
`implementation_coverage.csv`.

| scenario | grain | entity | gold (baseline → scenario) | coverage (baseline → scenario) | Δ (pp) |
| --- | --- | --- | ---: | ---: | ---: |
| all_observed_corrected | artifact | `build-scratch_minidb-claude_code_mimo_v2_5_low-t0-s20260501-51f333b3` | 15 → 14 | 100.0% → 93.3% | -6.667 |
| all_observed_corrected | artifact | `build-scratch_minidb-claude_code_mimo_v2_5_low-t1-s20260502-07943f95` | 13 → 12 | 86.7% → 80.0% | -6.667 |
| all_observed_corrected | artifact | `build-scratch_nanoweb-claude_code_mimo_v2_5_high-t1-s20260502-44ad1601` | 15 → 14 | 100.0% → 93.3% | -6.667 |
| all_observed_corrected | artifact | `build-scratch_nanoweb-claude_code_mimo_v2_5_low-t1-s20260502-0ab2ca9f` | 14 → 13 | 93.3% → 86.7% | -6.667 |
| all_observed_corrected | artifact | `build-scratch_nanoweb-claude_code_opus_4_7_high-t0-s20260501-c329ce51` | 15 → 14 | 100.0% → 93.3% | -6.667 |
| all_observed_corrected | artifact | `build-scratch_nanoweb-claude_code_opus_4_7_high-t1-s20261502-374bc791` | 15 → 14 | 100.0% → 93.3% | -6.667 |
| all_observed_corrected | artifact | `build-scratch_nanoweb-claude_code_opus_4_7_low-t1-s20261502-ae88af88` | 15 → 14 | 100.0% → 93.3% | -6.667 |
| all_observed_corrected | artifact | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t0-s20260501-1a57cf95` | 12 → 11 | 80.0% → 73.3% | -6.667 |
| all_observed_corrected | artifact | `build-scratch_nanoweb-codex_gpt5_5_low-t1-s20260502-ce7e6480` | 15 → 14 | 100.0% → 93.3% | -6.667 |
| all_observed_corrected | builder | `claude_code_mimo_v2_5_high` | 53 → 52 | 88.3% → 86.7% | -1.667 |
| all_observed_corrected | builder | `claude_code_mimo_v2_5_low` | 42 → 39 | 70.0% → 65.0% | -5.000 |
| all_observed_corrected | builder | `claude_code_opus_4_7_high` | 60 → 58 | 100.0% → 96.7% | -3.333 |
| all_observed_corrected | builder | `claude_code_opus_4_7_low` | 60 → 59 | 100.0% → 98.3% | -1.667 |
| all_observed_corrected | builder | `codex_gpt5_4_mini_low` | 51 → 50 | 85.0% → 83.3% | -1.667 |
| all_observed_corrected | builder | `codex_gpt5_5_low` | 59 → 58 | 98.3% → 96.7% | -1.667 |
| author_adjudicated | artifact | `build-scratch_minidb-claude_code_mimo_v2_5_low-t0-s20260501-51f333b3` | 15 → 14 | 100.0% → 93.3% | -6.667 |
| author_adjudicated | artifact | `build-scratch_minidb-claude_code_mimo_v2_5_low-t1-s20260502-07943f95` | 13 → 12 | 86.7% → 80.0% | -6.667 |
| author_adjudicated | artifact | `build-scratch_minidb-codex_gpt5_4_mini_low-t1-s20260502-c57ac23c` | 14 → 7 | 93.3% → 46.7% | -46.667 |
| author_adjudicated | artifact | `build-scratch_minidb-codex_gpt5_5_low-t0-s20260501-d0068f8c` | 14 → 12 | 93.3% → 80.0% | -13.333 |
| author_adjudicated | artifact | `build-scratch_minidb-codex_gpt5_5_low-t1-s20260502-4c36b45f` | 15 → 10 | 100.0% → 66.7% | -33.333 |
| author_adjudicated | artifact | `build-scratch_nanoweb-claude_code_mimo_v2_5_high-t1-s20260502-44ad1601` | 15 → 14 | 100.0% → 93.3% | -6.667 |
| author_adjudicated | artifact | `build-scratch_nanoweb-claude_code_mimo_v2_5_low-t1-s20260502-0ab2ca9f` | 14 → 13 | 93.3% → 86.7% | -6.667 |
| author_adjudicated | artifact | `build-scratch_nanoweb-claude_code_opus_4_7_high-t0-s20260501-c329ce51` | 15 → 14 | 100.0% → 93.3% | -6.667 |
| author_adjudicated | artifact | `build-scratch_nanoweb-claude_code_opus_4_7_high-t1-s20261502-374bc791` | 15 → 14 | 100.0% → 93.3% | -6.667 |
| author_adjudicated | artifact | `build-scratch_nanoweb-claude_code_opus_4_7_low-t1-s20261502-ae88af88` | 15 → 14 | 100.0% → 93.3% | -6.667 |
| author_adjudicated | artifact | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t0-s20260501-1a57cf95` | 12 → 4 | 80.0% → 26.7% | -53.333 |
| author_adjudicated | artifact | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t1-s20260502-8f79ca6d` | 14 → 5 | 93.3% → 33.3% | -60.000 |
| author_adjudicated | artifact | `build-scratch_nanoweb-codex_gpt5_5_low-t0-s20260501-6a64601d` | 15 → 9 | 100.0% → 60.0% | -40.000 |
| author_adjudicated | artifact | `build-scratch_nanoweb-codex_gpt5_5_low-t1-s20260502-ce7e6480` | 15 → 11 | 100.0% → 73.3% | -26.667 |
| author_adjudicated | builder | `claude_code_mimo_v2_5_high` | 53 → 52 | 88.3% → 86.7% | -1.667 |
| author_adjudicated | builder | `claude_code_mimo_v2_5_low` | 42 → 39 | 70.0% → 65.0% | -5.000 |
| author_adjudicated | builder | `claude_code_opus_4_7_high` | 60 → 58 | 100.0% → 96.7% | -3.333 |
| author_adjudicated | builder | `claude_code_opus_4_7_low` | 60 → 59 | 100.0% → 98.3% | -1.667 |
| author_adjudicated | builder | `codex_gpt5_4_mini_low` | 51 → 27 | 85.0% → 45.0% | -40.000 |
| author_adjudicated | builder | `codex_gpt5_5_low` | 59 → 42 | 98.3% → 70.0% | -28.333 |
| fully_adjudicated | artifact | `build-scratch_minidb-claude_code_mimo_v2_5_high-t0-s20260501-acb184a1` | 12 → 5 | 80.0% → 33.3% | -46.667 |
| fully_adjudicated | artifact | `build-scratch_minidb-claude_code_mimo_v2_5_high-t1-s20260502-550d8811` | 12 → 7 | 80.0% → 46.7% | -33.333 |
| fully_adjudicated | artifact | `build-scratch_minidb-claude_code_mimo_v2_5_low-t0-s20260501-51f333b3` | 15 → 9 | 100.0% → 60.0% | -40.000 |
| fully_adjudicated | artifact | `build-scratch_minidb-claude_code_mimo_v2_5_low-t1-s20260502-07943f95` | 13 → 6 | 86.7% → 40.0% | -46.667 |
| fully_adjudicated | artifact | `build-scratch_minidb-claude_code_mimo_v2_5_pro_high-t0-s20260501-291ba58c` | 14 → 9 | 93.3% → 60.0% | -33.333 |
| fully_adjudicated | artifact | `build-scratch_minidb-claude_code_mimo_v2_5_pro_high-t1-s20260502-b6e82435` | 14 → 10 | 93.3% → 66.7% | -26.667 |
| fully_adjudicated | artifact | `build-scratch_minidb-claude_code_opus_4_7_high-t1-s20260502-83f0c8f0` | 15 → 13 | 100.0% → 86.7% | -13.333 |
| fully_adjudicated | artifact | `build-scratch_minidb-claude_code_opus_4_7_low-t0-s20260501-cdbba75d` | 15 → 13 | 100.0% → 86.7% | -13.333 |
| fully_adjudicated | artifact | `build-scratch_minidb-claude_code_opus_4_7_low-t1-s20260502-5846c7eb` | 15 → 14 | 100.0% → 93.3% | -6.667 |
| fully_adjudicated | artifact | `build-scratch_minidb-claude_code_sonnet_4_6_high-t0-s20260501-aa19a4dc` | 15 → 12 | 100.0% → 80.0% | -20.000 |
| fully_adjudicated | artifact | `build-scratch_minidb-claude_code_sonnet_4_6_high-t1-s20260502-bd0b6695` | 15 → 10 | 100.0% → 66.7% | -33.333 |
| fully_adjudicated | artifact | `build-scratch_minidb-claude_code_sonnet_4_6_low-t0-s20260501-dd444f70` | 14 → 12 | 93.3% → 80.0% | -13.333 |
| fully_adjudicated | artifact | `build-scratch_minidb-claude_code_sonnet_4_6_low-t1-s20260502-0690460b` | 15 → 13 | 100.0% → 86.7% | -13.333 |
| fully_adjudicated | artifact | `build-scratch_minidb-codex_gpt5_4_mini_high-t0-s20260501-7ba82072` | 15 → 11 | 100.0% → 73.3% | -26.667 |
| fully_adjudicated | artifact | `build-scratch_minidb-codex_gpt5_4_mini_high-t1-s20260502-05500a40` | 15 → 14 | 100.0% → 93.3% | -6.667 |
| fully_adjudicated | artifact | `build-scratch_minidb-codex_gpt5_4_mini_low-t1-s20260502-c57ac23c` | 14 → 7 | 93.3% → 46.7% | -46.667 |
| fully_adjudicated | artifact | `build-scratch_minidb-codex_gpt5_5_high-t0-s20260501-1924cec4` | 15 → 13 | 100.0% → 86.7% | -13.333 |
| fully_adjudicated | artifact | `build-scratch_minidb-codex_gpt5_5_high-t1-s20260502-0c54a173` | 15 → 12 | 100.0% → 80.0% | -20.000 |
| fully_adjudicated | artifact | `build-scratch_minidb-codex_gpt5_5_low-t0-s20260501-d0068f8c` | 14 → 12 | 93.3% → 80.0% | -13.333 |
| fully_adjudicated | artifact | `build-scratch_minidb-codex_gpt5_5_low-t1-s20260502-4c36b45f` | 15 → 10 | 100.0% → 66.7% | -33.333 |
| fully_adjudicated | artifact | `build-scratch_nanoweb-claude_code_mimo_v2_5_high-t0-s20260501-f80bd4f0` | 14 → 5 | 93.3% → 33.3% | -60.000 |
| fully_adjudicated | artifact | `build-scratch_nanoweb-claude_code_mimo_v2_5_high-t1-s20260502-44ad1601` | 15 → 5 | 100.0% → 33.3% | -66.667 |
| fully_adjudicated | artifact | `build-scratch_nanoweb-claude_code_mimo_v2_5_low-t1-s20260502-0ab2ca9f` | 14 → 6 | 93.3% → 40.0% | -53.333 |
| fully_adjudicated | artifact | `build-scratch_nanoweb-claude_code_mimo_v2_5_pro_low-t1-s20260502-e516fe14` | 15 → 8 | 100.0% → 53.3% | -46.667 |
| fully_adjudicated | artifact | `build-scratch_nanoweb-claude_code_opus_4_7_high-t0-s20260501-c329ce51` | 15 → 7 | 100.0% → 46.7% | -53.333 |
| fully_adjudicated | artifact | `build-scratch_nanoweb-claude_code_opus_4_7_high-t1-s20261502-374bc791` | 15 → 13 | 100.0% → 86.7% | -13.333 |
| fully_adjudicated | artifact | `build-scratch_nanoweb-claude_code_opus_4_7_low-t0-s20261501-636b758c` | 15 → 11 | 100.0% → 73.3% | -26.667 |
| fully_adjudicated | artifact | `build-scratch_nanoweb-claude_code_opus_4_7_low-t1-s20261502-ae88af88` | 15 → 12 | 100.0% → 80.0% | -20.000 |
| fully_adjudicated | artifact | `build-scratch_nanoweb-claude_code_sonnet_4_6_high-t0-s20261501-05d5b610` | 14 → 10 | 93.3% → 66.7% | -26.667 |
| fully_adjudicated | artifact | `build-scratch_nanoweb-claude_code_sonnet_4_6_high-t1-s20261502-f19379b3` | 14 → 9 | 93.3% → 60.0% | -33.333 |
| fully_adjudicated | artifact | `build-scratch_nanoweb-claude_code_sonnet_4_6_low-t0-s20261501-e6047538` | 13 → 6 | 86.7% → 40.0% | -46.667 |
| fully_adjudicated | artifact | `build-scratch_nanoweb-claude_code_sonnet_4_6_low-t1-s20261502-8328c0da` | 15 → 8 | 100.0% → 53.3% | -46.667 |
| fully_adjudicated | artifact | `build-scratch_nanoweb-codex_gpt5_4_mini_high-t0-s20260501-aa87757d` | 15 → 9 | 100.0% → 60.0% | -40.000 |
| fully_adjudicated | artifact | `build-scratch_nanoweb-codex_gpt5_4_mini_high-t1-s20260502-e25932eb` | 15 → 10 | 100.0% → 66.7% | -33.333 |
| fully_adjudicated | artifact | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t0-s20260501-1a57cf95` | 12 → 4 | 80.0% → 26.7% | -53.333 |
| fully_adjudicated | artifact | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t1-s20260502-8f79ca6d` | 14 → 5 | 93.3% → 33.3% | -60.000 |
| fully_adjudicated | artifact | `build-scratch_nanoweb-codex_gpt5_5_high-t0-s20260501-b54f0634` | 15 → 11 | 100.0% → 73.3% | -26.667 |
| fully_adjudicated | artifact | `build-scratch_nanoweb-codex_gpt5_5_high-t1-s20260502-a5df8a82` | 15 → 11 | 100.0% → 73.3% | -26.667 |
| fully_adjudicated | artifact | `build-scratch_nanoweb-codex_gpt5_5_low-t0-s20260501-6a64601d` | 15 → 9 | 100.0% → 60.0% | -40.000 |
| fully_adjudicated | artifact | `build-scratch_nanoweb-codex_gpt5_5_low-t1-s20260502-ce7e6480` | 15 → 11 | 100.0% → 73.3% | -26.667 |
| fully_adjudicated | builder | `claude_code_mimo_v2_5_high` | 53 → 22 | 88.3% → 36.7% | -51.667 |
| fully_adjudicated | builder | `claude_code_mimo_v2_5_low` | 42 → 21 | 70.0% → 35.0% | -35.000 |
| fully_adjudicated | builder | `claude_code_mimo_v2_5_pro_high` | 56 → 47 | 93.3% → 78.3% | -15.000 |
| fully_adjudicated | builder | `claude_code_mimo_v2_5_pro_low` | 56 → 49 | 93.3% → 81.7% | -11.667 |
| fully_adjudicated | builder | `claude_code_opus_4_7_high` | 60 → 48 | 100.0% → 80.0% | -20.000 |
| fully_adjudicated | builder | `claude_code_opus_4_7_low` | 60 → 50 | 100.0% → 83.3% | -16.667 |
| fully_adjudicated | builder | `claude_code_sonnet_4_6_high` | 58 → 41 | 96.7% → 68.3% | -28.333 |
| fully_adjudicated | builder | `claude_code_sonnet_4_6_low` | 57 → 39 | 95.0% → 65.0% | -30.000 |
| fully_adjudicated | builder | `codex_gpt5_4_mini_high` | 60 → 44 | 100.0% → 73.3% | -26.667 |
| fully_adjudicated | builder | `codex_gpt5_4_mini_low` | 51 → 27 | 85.0% → 45.0% | -40.000 |
| fully_adjudicated | builder | `codex_gpt5_5_high` | 60 → 47 | 100.0% → 78.3% | -21.667 |
| fully_adjudicated | builder | `codex_gpt5_5_low` | 59 → 42 | 98.3% → 70.0% | -28.333 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_minidb-claude_code_mimo_v2_5_high-t0-s20260501-acb184a1` | 12 → 5 | 80.0% → 33.3% | -46.667 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_minidb-claude_code_mimo_v2_5_high-t1-s20260502-550d8811` | 12 → 7 | 80.0% → 46.7% | -33.333 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_minidb-claude_code_mimo_v2_5_low-t0-s20260501-51f333b3` | 15 → 9 | 100.0% → 60.0% | -40.000 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_minidb-claude_code_mimo_v2_5_low-t1-s20260502-07943f95` | 13 → 4 | 86.7% → 26.7% | -60.000 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_minidb-claude_code_mimo_v2_5_pro_high-t0-s20260501-291ba58c` | 14 → 8 | 93.3% → 53.3% | -40.000 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_minidb-claude_code_mimo_v2_5_pro_high-t1-s20260502-b6e82435` | 14 → 10 | 93.3% → 66.7% | -26.667 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_minidb-claude_code_opus_4_7_high-t1-s20260502-83f0c8f0` | 15 → 13 | 100.0% → 86.7% | -13.333 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_minidb-claude_code_opus_4_7_low-t0-s20260501-cdbba75d` | 15 → 13 | 100.0% → 86.7% | -13.333 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_minidb-claude_code_opus_4_7_low-t1-s20260502-5846c7eb` | 15 → 14 | 100.0% → 93.3% | -6.667 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_minidb-claude_code_sonnet_4_6_high-t0-s20260501-aa19a4dc` | 15 → 12 | 100.0% → 80.0% | -20.000 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_minidb-claude_code_sonnet_4_6_high-t1-s20260502-bd0b6695` | 15 → 10 | 100.0% → 66.7% | -33.333 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_minidb-claude_code_sonnet_4_6_low-t0-s20260501-dd444f70` | 14 → 12 | 93.3% → 80.0% | -13.333 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_minidb-claude_code_sonnet_4_6_low-t1-s20260502-0690460b` | 15 → 13 | 100.0% → 86.7% | -13.333 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_minidb-codex_gpt5_4_mini_high-t0-s20260501-7ba82072` | 15 → 11 | 100.0% → 73.3% | -26.667 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_minidb-codex_gpt5_4_mini_high-t1-s20260502-05500a40` | 15 → 13 | 100.0% → 86.7% | -13.333 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_minidb-codex_gpt5_4_mini_low-t1-s20260502-c57ac23c` | 14 → 7 | 93.3% → 46.7% | -46.667 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_minidb-codex_gpt5_5_high-t0-s20260501-1924cec4` | 15 → 13 | 100.0% → 86.7% | -13.333 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_minidb-codex_gpt5_5_high-t1-s20260502-0c54a173` | 15 → 12 | 100.0% → 80.0% | -20.000 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_minidb-codex_gpt5_5_low-t0-s20260501-d0068f8c` | 14 → 12 | 93.3% → 80.0% | -13.333 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_minidb-codex_gpt5_5_low-t1-s20260502-4c36b45f` | 15 → 10 | 100.0% → 66.7% | -33.333 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_nanoweb-claude_code_mimo_v2_5_high-t0-s20260501-f80bd4f0` | 14 → 4 | 93.3% → 26.7% | -66.667 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_nanoweb-claude_code_mimo_v2_5_high-t1-s20260502-44ad1601` | 15 → 5 | 100.0% → 33.3% | -66.667 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_nanoweb-claude_code_mimo_v2_5_low-t1-s20260502-0ab2ca9f` | 14 → 6 | 93.3% → 40.0% | -53.333 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_nanoweb-claude_code_mimo_v2_5_pro_low-t1-s20260502-e516fe14` | 15 → 7 | 100.0% → 46.7% | -53.333 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_nanoweb-claude_code_opus_4_7_high-t0-s20260501-c329ce51` | 15 → 6 | 100.0% → 40.0% | -60.000 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_nanoweb-claude_code_opus_4_7_high-t1-s20261502-374bc791` | 15 → 13 | 100.0% → 86.7% | -13.333 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_nanoweb-claude_code_opus_4_7_low-t0-s20261501-636b758c` | 15 → 11 | 100.0% → 73.3% | -26.667 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_nanoweb-claude_code_opus_4_7_low-t1-s20261502-ae88af88` | 15 → 12 | 100.0% → 80.0% | -20.000 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_nanoweb-claude_code_sonnet_4_6_high-t0-s20261501-05d5b610` | 14 → 10 | 93.3% → 66.7% | -26.667 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_nanoweb-claude_code_sonnet_4_6_high-t1-s20261502-f19379b3` | 14 → 9 | 93.3% → 60.0% | -33.333 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_nanoweb-claude_code_sonnet_4_6_low-t0-s20261501-e6047538` | 13 → 6 | 86.7% → 40.0% | -46.667 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_nanoweb-claude_code_sonnet_4_6_low-t1-s20261502-8328c0da` | 15 → 8 | 100.0% → 53.3% | -46.667 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_nanoweb-codex_gpt5_4_mini_high-t0-s20260501-aa87757d` | 15 → 9 | 100.0% → 60.0% | -40.000 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_nanoweb-codex_gpt5_4_mini_high-t1-s20260502-e25932eb` | 15 → 10 | 100.0% → 66.7% | -33.333 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t0-s20260501-1a57cf95` | 12 → 4 | 80.0% → 26.7% | -53.333 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_nanoweb-codex_gpt5_4_mini_low-t1-s20260502-8f79ca6d` | 14 → 5 | 93.3% → 33.3% | -60.000 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_nanoweb-codex_gpt5_5_high-t0-s20260501-b54f0634` | 15 → 11 | 100.0% → 73.3% | -26.667 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_nanoweb-codex_gpt5_5_high-t1-s20260502-a5df8a82` | 15 → 10 | 100.0% → 66.7% | -33.333 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_nanoweb-codex_gpt5_5_low-t0-s20260501-6a64601d` | 15 → 9 | 100.0% → 60.0% | -40.000 |
| author_adjudicated_plus_unreviewed_flags | artifact | `build-scratch_nanoweb-codex_gpt5_5_low-t1-s20260502-ce7e6480` | 15 → 11 | 100.0% → 73.3% | -26.667 |
| author_adjudicated_plus_unreviewed_flags | builder | `claude_code_mimo_v2_5_high` | 53 → 21 | 88.3% → 35.0% | -53.333 |
| author_adjudicated_plus_unreviewed_flags | builder | `claude_code_mimo_v2_5_low` | 42 → 19 | 70.0% → 31.7% | -38.333 |
| author_adjudicated_plus_unreviewed_flags | builder | `claude_code_mimo_v2_5_pro_high` | 56 → 46 | 93.3% → 76.7% | -16.667 |
| author_adjudicated_plus_unreviewed_flags | builder | `claude_code_mimo_v2_5_pro_low` | 56 → 48 | 93.3% → 80.0% | -13.333 |
| author_adjudicated_plus_unreviewed_flags | builder | `claude_code_opus_4_7_high` | 60 → 47 | 100.0% → 78.3% | -21.667 |
| author_adjudicated_plus_unreviewed_flags | builder | `claude_code_opus_4_7_low` | 60 → 50 | 100.0% → 83.3% | -16.667 |
| author_adjudicated_plus_unreviewed_flags | builder | `claude_code_sonnet_4_6_high` | 58 → 41 | 96.7% → 68.3% | -28.333 |
| author_adjudicated_plus_unreviewed_flags | builder | `claude_code_sonnet_4_6_low` | 57 → 39 | 95.0% → 65.0% | -30.000 |
| author_adjudicated_plus_unreviewed_flags | builder | `codex_gpt5_4_mini_high` | 60 → 43 | 100.0% → 71.7% | -28.333 |
| author_adjudicated_plus_unreviewed_flags | builder | `codex_gpt5_4_mini_low` | 51 → 27 | 85.0% → 45.0% | -40.000 |
| author_adjudicated_plus_unreviewed_flags | builder | `codex_gpt5_5_high` | 60 → 46 | 100.0% → 76.7% | -23.333 |
| author_adjudicated_plus_unreviewed_flags | builder | `codex_gpt5_5_low` | 59 → 42 | 98.3% → 70.0% | -28.333 |

| scenario | artifact-present per-builder coverage range |
| --- | ---: |
| baseline | 70.0%–100.0% |
| all_observed_corrected | 65.0%–100.0% |
| author_adjudicated | 45.0%–100.0% |
| fully_adjudicated | 35.0%–83.3% |
| author_adjudicated_plus_unreviewed_flags | 31.7%–83.3% |

## Conditioned recovery

| scenario | slice | pairs | answer rows | correct | accuracy | Δ accuracy (pp) |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| baseline | overall | 592 | 21312 | 21084 | 98.930% | — |
| baseline | low_prior | 196 | 7056 | 6906 | 97.874% | — |
| all_observed_corrected | overall | 583 | 20988 | 20772 | 98.971% | +0.04066 |
| all_observed_corrected | low_prior | 193 | 6948 | 6805 | 97.942% | +0.06770 |
| author_adjudicated | overall | 544 | 19584 | 19389 | 99.004% | +0.07411 |
| author_adjudicated | low_prior | 179 | 6444 | 6306 | 97.858% | -0.01568 |
| fully_adjudicated | overall | 397 | 14292 | 14255 | 99.741% | +0.81093 |
| fully_adjudicated | low_prior | 134 | 4824 | 4796 | 99.420% | +1.54542 |
| author_adjudicated_plus_unreviewed_flags | overall | 389 | 14004 | 13970 | 99.757% | +0.82703 |
| author_adjudicated_plus_unreviewed_flags | low_prior | 130 | 4680 | 4655 | 99.466% | +1.59166 |

The numerator deltas are recomputed from the released answer rows for
each corrected key; they are not assumed to be all correct.

## Evidence-audit overlap

Sampled answers of the post-hoc evidence-citation audit
(`runs_release/reports/evidence_citation_audit_sample.csv`) that fall on pairs the
scenario relabels. The audit labels were assigned under the released labels.

| scenario | slice | Supported answers on relabeled pairs | sampled answers on relabeled pairs |
| --- | --- | ---: | ---: |
| baseline | all | 0/65 | 0/72 |
| baseline | low_prior | 0/35 | 0/36 |
| baseline | high_prior | 0/30 | 0/36 |
| all_observed_corrected | all | 1/65 | 1/72 |
| all_observed_corrected | low_prior | 1/35 | 1/36 |
| all_observed_corrected | high_prior | 0/30 | 0/36 |
| author_adjudicated | all | 7/65 | 7/72 |
| author_adjudicated | low_prior | 5/35 | 5/36 |
| author_adjudicated | high_prior | 2/30 | 2/36 |
| fully_adjudicated | all | 23/65 | 23/72 |
| fully_adjudicated | low_prior | 10/35 | 10/36 |
| fully_adjudicated | high_prior | 13/30 | 13/36 |
| author_adjudicated_plus_unreviewed_flags | all | 26/65 | 26/72 |
| author_adjudicated_plus_unreviewed_flags | low_prior | 11/35 | 11/36 |
| author_adjudicated_plus_unreviewed_flags | high_prior | 15/30 | 15/36 |

## Primary perfect-run total-byte R_b

| scenario | panel | rank | builder | R_b | cells | coverage |
| --- | --- | ---: | --- | ---: | ---: | ---: |
| baseline | high_effort | 1 | codex_gpt5_5_high | 1.032701 | 12/12 | 100.0% |
| baseline | high_effort | 2 | codex_gpt5_4_mini_high | 1.320039 | 12/12 | 100.0% |
| baseline | low_effort | 1 | codex_gpt5_5_low | 1.150991 | 12/12 | 100.0% |
| baseline | low_effort | 2 | codex_gpt5_4_mini_low | 1.151405 | 11/12 | 91.7% |
| all_observed_corrected | high_effort | 1 | codex_gpt5_5_high | 1.032701 | 12/12 | 100.0% |
| all_observed_corrected | high_effort | 2 | codex_gpt5_4_mini_high | 1.320039 | 12/12 | 100.0% |
| all_observed_corrected | low_effort | 1 | codex_gpt5_5_low | 1.150991 | 12/12 | 100.0% |
| all_observed_corrected | low_effort | 2 | codex_gpt5_4_mini_low | 1.151405 | 11/12 | 91.7% |
| author_adjudicated | high_effort | 1 | codex_gpt5_5_high | 1.032701 | 12/12 | 100.0% |
| author_adjudicated | high_effort | 2 | codex_gpt5_4_mini_high | 1.320039 | 12/12 | 100.0% |
| author_adjudicated | low_effort | 1 | codex_gpt5_4_mini_low | 1.128049 | 12/12 | 100.0% |
| author_adjudicated | low_effort | 2 | codex_gpt5_5_low | 1.168112 | 12/12 | 100.0% |
| fully_adjudicated | high_effort | 1 | codex_gpt5_5_high | 1.032701 | 12/12 | 100.0% |
| fully_adjudicated | high_effort | 2 | codex_gpt5_4_mini_high | 1.313516 | 12/12 | 100.0% |
| fully_adjudicated | low_effort | 1 | codex_gpt5_4_mini_low | 1.128049 | 12/12 | 100.0% |
| fully_adjudicated | low_effort | 2 | codex_gpt5_5_low | 1.168112 | 12/12 | 100.0% |
| author_adjudicated_plus_unreviewed_flags | high_effort | 1 | codex_gpt5_5_high | 1.032701 | 12/12 | 100.0% |
| author_adjudicated_plus_unreviewed_flags | high_effort | 2 | codex_gpt5_4_mini_high | 1.309950 | 12/12 | 100.0% |
| author_adjudicated_plus_unreviewed_flags | low_effort | 1 | codex_gpt5_4_mini_low | 1.128049 | 12/12 | 100.0% |
| author_adjudicated_plus_unreviewed_flags | low_effort | 2 | codex_gpt5_5_low | 1.168112 | 12/12 | 100.0% |

| scenario | panel | builder order (rank 1 → last) | order vs. baseline |
| --- | --- | --- | --- |
| baseline | high_effort | codex_gpt5_5_high > codex_gpt5_4_mini_high > claude_code_opus_4_7_high > claude_code_sonnet_4_6_high > claude_code_mimo_v2_5_high > claude_code_mimo_v2_5_pro_high | — |
| baseline | low_effort | codex_gpt5_5_low > codex_gpt5_4_mini_low > claude_code_opus_4_7_low > claude_code_mimo_v2_5_pro_low > claude_code_sonnet_4_6_low > claude_code_mimo_v2_5_low | — |
| all_observed_corrected | high_effort | codex_gpt5_5_high > codex_gpt5_4_mini_high > claude_code_opus_4_7_high > claude_code_sonnet_4_6_high > claude_code_mimo_v2_5_high > claude_code_mimo_v2_5_pro_high | unchanged |
| all_observed_corrected | low_effort | codex_gpt5_5_low > codex_gpt5_4_mini_low > claude_code_opus_4_7_low > claude_code_mimo_v2_5_pro_low > claude_code_sonnet_4_6_low > claude_code_mimo_v2_5_low | unchanged |
| author_adjudicated | high_effort | codex_gpt5_5_high > codex_gpt5_4_mini_high > claude_code_opus_4_7_high > claude_code_sonnet_4_6_high > claude_code_mimo_v2_5_high > claude_code_mimo_v2_5_pro_high | unchanged |
| author_adjudicated | low_effort | codex_gpt5_4_mini_low > codex_gpt5_5_low > claude_code_opus_4_7_low > claude_code_mimo_v2_5_pro_low > claude_code_sonnet_4_6_low > claude_code_mimo_v2_5_low | changed |
| fully_adjudicated | high_effort | codex_gpt5_5_high > codex_gpt5_4_mini_high > claude_code_opus_4_7_high > claude_code_sonnet_4_6_high > claude_code_mimo_v2_5_high > claude_code_mimo_v2_5_pro_high | unchanged |
| fully_adjudicated | low_effort | codex_gpt5_4_mini_low > codex_gpt5_5_low > claude_code_opus_4_7_low > claude_code_mimo_v2_5_pro_low > claude_code_sonnet_4_6_low > claude_code_mimo_v2_5_low | changed |
| author_adjudicated_plus_unreviewed_flags | high_effort | codex_gpt5_5_high > codex_gpt5_4_mini_high > claude_code_opus_4_7_high > claude_code_sonnet_4_6_high > claude_code_mimo_v2_5_high > claude_code_mimo_v2_5_pro_high | unchanged |
| author_adjudicated_plus_unreviewed_flags | low_effort | codex_gpt5_4_mini_low > codex_gpt5_5_low > claude_code_opus_4_7_low > claude_code_mimo_v2_5_pro_low > claude_code_sonnet_4_6_low > claude_code_mimo_v2_5_low | changed |

| scenario | builder ranks changed | R_b scores changed |
| --- | ---: | ---: |
| all_observed_corrected | 0 | 2 |
| author_adjudicated | 2 | 6 |
| fully_adjudicated | 2 | 11 |
| author_adjudicated_plus_unreviewed_flags | 2 | 11 |

## Perfect-run and cell effects

| scenario | panel | perfect runs / runs | Δ perfect runs | contributing cells / observed | Δ cells |
| --- | --- | ---: | ---: | ---: | ---: |
| baseline | high_effort | 343/396 | +0 | 66/66 | +0 |
| baseline | low_effort | 296/342 | +0 | 64/66 | +0 |
| all_observed_corrected | high_effort | 343/396 | +0 | 66/66 | +0 |
| all_observed_corrected | low_effort | 301/342 | +5 | 64/66 | +0 |
| author_adjudicated | high_effort | 343/396 | +0 | 66/66 | +0 |
| author_adjudicated | low_effort | 312/342 | +16 | 65/66 | +1 |
| fully_adjudicated | high_effort | 388/396 | +45 | 66/66 | +0 |
| fully_adjudicated | low_effort | 332/342 | +36 | 66/66 | +2 |
| author_adjudicated_plus_unreviewed_flags | high_effort | 390/396 | +47 | 66/66 | +0 |
| author_adjudicated_plus_unreviewed_flags | low_effort | 332/342 | +36 | 66/66 | +2 |

Run-change counts below cover all primary find runs, including cross-effort
diagnostics; perfect-cell counts and R_b use only the matched high/high
and low/low panels.

| scenario | changed run metrics | changed perfect statuses | changed perfect-count cells | changed cell contribution |
| --- | ---: | ---: | ---: | ---: |
| all_observed_corrected | 324 | 9 | 3 | 0 |
| author_adjudicated | 504 | 27 | 11 | 1 |
| fully_adjudicated | 1440 | 165 | 51 | 2 |
| author_adjudicated_plus_unreviewed_flags | 1440 | 167 | 51 | 2 |

## Scope

This sensitivity establishes the effect of the observed source-audit errors
and the author-confirmed flags, of the model-assisted decisions on the other
flags, and of additionally relabeling every flag without an author decision.
Author decisions cover only the flags on the builders listed above.
The model-assisted decisions used by the fully_adjudicated scenario
were reviewed and approved by the author on 2026-10-16;
`decided_by` records them as model-assisted adjudication.
Full run-level inspection bytes are retained, matching the released
conditioned-effort contract.

Regenerate with:

```sh
uv run python scripts/analyze_rebuttal_label_sensitivity.py --runs-root runs_release --full-audit-disagreements rebuttal/full_conformance_audit/disagreements.csv --author-decisions rebuttal/full_conformance_audit/author_decisions.csv --model-assisted-decisions rebuttal/full_conformance_audit/model_assisted_decisions.csv --label-corrections runs_release/reports/artifact_conformance_consensus/final/label_corrections_v1_1.csv --output-dir rebuttal/full_conformance_audit/label_sensitivity_adjudicated
```
