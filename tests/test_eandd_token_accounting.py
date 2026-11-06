from pathlib import Path

import pytest

from bnf.schema.run import FindRun, VendorTokens
from scripts.analyze_eandd_final import _usage_total_tokens


ROOT = Path(__file__).resolve().parents[1]
SONNET_RUN = ROOT / "runs_release" / "records" / (
    "find-build-scratch_minidb-cla-claude_code_sonnet_4_6_low-t0-s20260501-0d3aba2e.json"
)


@pytest.fixture
def sonnet_run() -> FindRun:
    return FindRun.model_validate_json(SONNET_RUN.read_text(encoding="utf-8"))


def test_released_subagent_usage_is_included(sonnet_run: FindRun) -> None:
    assert sonnet_run.usage.vendor_usage.total_tokens == 113_506
    observed = sonnet_run.usage.efficiency.vendor_tokens
    assert observed.total_input_tokens == 885_582
    assert observed.cached_input_tokens + observed.uncached_input_tokens == 885_582
    assert observed.cache_creation_input_tokens == 106_340
    assert observed.non_reasoning_output_tokens + observed.reasoning_output_tokens == 11_440
    assert _usage_total_tokens(sonnet_run) == 897_022


@pytest.mark.parametrize("missing", ["efficiency", "vendor_tokens"])
def test_missing_observed_usage_does_not_use_main_session(
    sonnet_run: FindRun, missing: str
) -> None:
    if missing == "efficiency":
        sonnet_run.usage.efficiency = None
    else:
        sonnet_run.usage.efficiency.vendor_tokens = None
    with pytest.raises(ValueError, match="missing observed vendor tokens"):
        _usage_total_tokens(sonnet_run)


def test_observed_zero_does_not_use_main_session(sonnet_run: FindRun) -> None:
    sonnet_run.usage.efficiency.vendor_tokens = VendorTokens()
    assert _usage_total_tokens(sonnet_run) == 0
