# Validity Stress Diagnostics

This report separates the compile-pass primary panel from prior-only and stress diagnostics.

## Main Stress Table

| slice | task | builds | find runs | answer rows | accuracy | perfect-run rate | delta vs question-only |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| question_only_control | scratch_minidb | 0 | 0 | 540 | 94.6% | -- | +0.0 pp |
| question_only_control | scratch_nanoweb | 0 | 0 | 540 | 94.4% | -- | +0.0 pp |
| question_only_control | all | 0 | 0 | 1080 | 94.5% | -- | +0.0 pp |
| artifact_present_all | scratch_minidb | 24 | 864 | 12960 | 95.8% | 58.0% | +1.2 pp |
| artifact_present_all | scratch_nanoweb | 24 | 864 | 12960 | 96.9% | 66.8% | +2.5 pp |
| artifact_present_all | all | 48 | 1728 | 25920 | 96.4% | 62.4% | +1.8 pp |
| compile_pass_primary | scratch_minidb | 21 | 756 | 11340 | 96.4% | 61.4% | +1.8 pp |
| compile_pass_primary | scratch_nanoweb | 20 | 720 | 10800 | 97.0% | 68.8% | +2.6 pp |
| compile_pass_primary | all | 41 | 1476 | 22140 | 96.7% | 65.0% | +2.2 pp |
| compile_failed_excluded | scratch_minidb | 3 | 108 | 1620 | 91.7% | 34.3% | -2.9 pp |
| compile_failed_excluded | scratch_nanoweb | 4 | 144 | 2160 | 96.4% | 56.9% | +1.9 pp |
| compile_failed_excluded | all | 7 | 252 | 3780 | 94.4% | 47.2% | -0.1 pp |
| empty_artifact_excluded | scratch_nanoweb | 1 | 36 | 540 | 98.9% | 83.3% | +4.4 pp |
| empty_artifact_excluded | all | 1 | 36 | 540 | 98.9% | 83.3% | +4.4 pp |

## Excluded Builds

| task | builder | trial | rust files | rust LOC | empty | build run |
| --- | --- | ---: | ---: | ---: | ---: | --- |
| scratch_minidb | claude_code_mimo_v2_5_pro_low | 0 | 18 | 7037 | 0 | build-scratch_minidb-claude_code_mimo_v2_5_pro_low-t0-s20260501-f53e2357 |
| scratch_minidb | claude_code_mimo_v2_5_pro_low | 1 | 20 | 7374 | 0 | build-scratch_minidb-claude_code_mimo_v2_5_pro_low-t1-s20260502-e387a96b |
| scratch_minidb | codex_gpt5_4_mini_low | 0 | 16 | 2157 | 0 | build-scratch_minidb-codex_gpt5_4_mini_low-t0-s20260501-6923533c |
| scratch_nanoweb | claude_code_mimo_v2_5_low | 0 | 0 | 0 | 1 | build-scratch_nanoweb-claude_code_mimo_v2_5_low-t0-s20260501-957fa423 |
| scratch_nanoweb | claude_code_mimo_v2_5_pro_high | 0 | 28 | 3559 | 0 | build-scratch_nanoweb-claude_code_mimo_v2_5_pro_high-t0-s20260501-a6d76cc1 |
| scratch_nanoweb | claude_code_mimo_v2_5_pro_high | 1 | 28 | 3907 | 0 | build-scratch_nanoweb-claude_code_mimo_v2_5_pro_high-t1-s20260502-beb8c87d |
| scratch_nanoweb | claude_code_mimo_v2_5_pro_low | 0 | 28 | 3803 | 0 | build-scratch_nanoweb-claude_code_mimo_v2_5_pro_low-t0-s20260501-d22d724b |
