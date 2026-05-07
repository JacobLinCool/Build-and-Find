"""In-process proxy gateway for tracking per-session LLM API usage.

Phase 1 implementation: pure Python (``aiohttp``). Phase 2 will swap the
transport layer for a Rust ``maturin`` extension while keeping this public
API stable.

Typical use::

    from bnf.proxy import Gateway

    with Gateway.start() as gw:
        session = gw.create_session(upstream="anthropic")
        env = {"ANTHROPIC_BASE_URL": session.base_url, **os.environ}
        # ... spawn agent subprocess with ``env`` ...
        stats = gw.get_stats(session.id)
"""

from bnf.proxy.gateway import (
    EventSlice,
    Gateway,
    ProxyEvent,
    SessionHandle,
    SessionStats,
    VendorCounter,
)
from bnf.proxy.parser import (
    ContentBreakdown,
    NovelTracker,
    StreamContentAnalyzer,
    StreamUsageParser,
    UsageSnapshot,
    analyze_request_body,
    analyze_request_body_novel,
    analyze_response_body,
    parse_json_usage,
)

__all__ = [
    "ContentBreakdown",
    "EventSlice",
    "Gateway",
    "NovelTracker",
    "ProxyEvent",
    "SessionHandle",
    "SessionStats",
    "StreamContentAnalyzer",
    "StreamUsageParser",
    "UsageSnapshot",
    "VendorCounter",
    "analyze_request_body",
    "analyze_request_body_novel",
    "analyze_response_body",
    "parse_json_usage",
]
