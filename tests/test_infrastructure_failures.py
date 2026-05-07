from bnf.runners.base import AgentResult, detect_infrastructure_failure
from bnf.schema.run import UsageMetrics


def _result(stdout: str, *, status: str = "error") -> AgentResult:
    return AgentResult(
        status=status,
        stdout=stdout,
        stderr="",
        usage=UsageMetrics(),
        error=None if status == "ok" else "exit code 1",
    )


def test_detects_claude_authentication_failure() -> None:
    result = _result(
        'Failed to authenticate. API Error: 401 {"type":"error","error":'
        '{"type":"authentication_error","message":"Invalid authentication credentials"}}'
    )

    assert detect_infrastructure_failure(result) == "agent authentication failed"


def test_detects_provider_rate_limit_failure() -> None:
    result = _result("API Error: rate_limit_error: request rate limit exceeded")

    assert detect_infrastructure_failure(result) == "provider rate limit reached"


def test_regular_agent_error_is_experimental_data() -> None:
    result = _result("cargo test failed: unresolved import crate::router")

    assert detect_infrastructure_failure(result) is None


def test_success_status_is_never_infrastructure_failure() -> None:
    result = _result("invalid authentication credentials", status="ok")

    assert detect_infrastructure_failure(result) is None
