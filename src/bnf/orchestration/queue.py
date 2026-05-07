"""Small pending-queue worker pool for benchmark runs.

The benchmark has heterogeneous run durations. Static shards therefore
leave capacity idle while the slowest shard drains. This helper keeps a
single pending queue and dispatches the next logical cell whenever any
worker finishes.
"""

from __future__ import annotations

from collections.abc import Callable, Iterable
from concurrent.futures import FIRST_COMPLETED, Future, ThreadPoolExecutor, wait
from dataclasses import dataclass
from time import monotonic
from typing import Generic, TypeVar

T = TypeVar("T")


@dataclass(frozen=True)
class WorkItem(Generic[T]):
    """One logical unit of experiment work."""

    name: str
    run: Callable[[], T]


@dataclass(frozen=True)
class WorkResult(Generic[T]):
    """Completed work item plus timing metadata."""

    name: str
    value: T
    elapsed_seconds: float


def run_work_queue[T](
    items: Iterable[WorkItem[T]],
    *,
    workers: int,
    on_result: Callable[[WorkResult[T], int, int], None] | None = None,
) -> list[WorkResult[T]]:
    """Run items through a dynamic pending queue.

    ``workers=1`` is intentionally valid and preserves the same failure
    behavior as a serial loop. For ``workers>1``, the queue submits at most
    ``workers`` items at a time and starts another item as each future
    completes. The first exception aborts pending work and is re-raised.
    """

    pending = list(items)
    total = len(pending)
    if total == 0:
        return []
    if workers < 1:
        raise ValueError("workers must be >= 1")

    active_limit = min(workers, total)
    results: list[WorkResult[T]] = []

    def submit(executor: ThreadPoolExecutor, item: WorkItem[T]) -> Future[WorkResult[T]]:
        started = monotonic()

        def invoke() -> WorkResult[T]:
            value = item.run()
            return WorkResult(
                name=item.name,
                value=value,
                elapsed_seconds=monotonic() - started,
            )

        return executor.submit(invoke)

    with ThreadPoolExecutor(max_workers=active_limit, thread_name_prefix="bnf-worker") as executor:
        in_flight: dict[Future[WorkResult[T]], WorkItem[T]] = {}
        next_index = 0
        for _ in range(active_limit):
            item = pending[next_index]
            next_index += 1
            in_flight[submit(executor, item)] = item

        while in_flight:
            done, _ = wait(in_flight, return_when=FIRST_COMPLETED)
            for future in done:
                in_flight.pop(future)
                result = future.result()
                results.append(result)
                if on_result is not None:
                    on_result(result, len(results), total)
                if next_index < total:
                    item = pending[next_index]
                    next_index += 1
                    in_flight[submit(executor, item)] = item
    return results
