from __future__ import annotations

import time

import pytest

from bnf.orchestration import WorkItem, run_work_queue


def test_work_queue_dispatches_pending_items_to_available_workers() -> None:
    started: list[int] = []

    def item(i: int):
        def run() -> int:
            started.append(i)
            time.sleep(0.01 if i == 0 else 0.001)
            return i

        return run

    results = run_work_queue(
        [WorkItem(name=f"i{i}", run=item(i)) for i in range(5)],
        workers=2,
    )

    assert sorted(result.value for result in results) == [0, 1, 2, 3, 4]
    assert started[:2] == [0, 1]
    assert len(started) == 5


def test_work_queue_reraises_first_failure() -> None:
    def fail() -> int:
        raise RuntimeError("boom")

    with pytest.raises(RuntimeError, match="boom"):
        run_work_queue([WorkItem(name="bad", run=fail)], workers=1)
