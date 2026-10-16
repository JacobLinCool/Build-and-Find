"""Continue the E&D final run without overwriting raw evidence.

Successful records keep their original run ids and are skipped by the
normal runner resume path. Failed infrastructure records, such as a
provider auth outage, are preserved and replaced by deterministic
recovery-seed attempts with new run ids.
"""

from __future__ import annotations

import argparse
import subprocess
from pathlib import Path

from bnf.build.runner import BuildRunner
from bnf.build.runner import _mk_id as _mk_build_id
from bnf.config import paths
from bnf.find.runner import FindRunner
from bnf.find.runner import _mk_id as _mk_find_id
from bnf.orchestration import WorkItem, WorkResult, run_work_queue
from bnf.proxy import Gateway
from bnf.runners import AgentResult, get_agent
from bnf.runners.base import detect_infrastructure_failure
from bnf.schema import load_question_bank, load_specs_dir
from bnf.schema.run import BuildRun, FindRun, UsageMetrics, load_run_json

DEFAULT_TASKS = "scratch_minidb,scratch_nanoweb"
DEFAULT_AGENTS = ",".join(
    [
        "claude_code_opus_4_7_high",
        "claude_code_opus_4_7_low",
        "claude_code_sonnet_4_6_high",
        "claude_code_sonnet_4_6_low",
        "codex_gpt5_5_high",
        "codex_gpt5_5_low",
        "codex_gpt5_4_mini_high",
        "codex_gpt5_4_mini_low",
        "claude_code_mimo_v2_5_pro_high",
        "claude_code_mimo_v2_5_pro_low",
        "claude_code_mimo_v2_5_high",
        "claude_code_mimo_v2_5_low",
    ]
)


def main() -> None:
    args = _parse_args()
    p = paths()
    task_ids = _split_csv(args.tasks)
    builder_ids = _split_csv(args.builders)
    finder_ids = _split_csv(args.finders)

    specs = [spec for spec in load_specs_dir(p.specs_dir) if spec.task_id in task_ids]
    missing = sorted(set(task_ids) - {spec.task_id for spec in specs})
    if missing:
        raise SystemExit(f"unknown task id(s): {', '.join(missing)}")

    if args.smoke_claude:
        _smoke_claude()

    gateway = None if args.no_proxy else Gateway.start()
    if gateway is not None:
        print(f"proxy listening on {gateway.base_url}")

    try:
        br_runner = BuildRunner(
            specs_dir=p.specs_dir,
            artifacts_dir=p.artifacts_dir,
            runs_dir=p.runs_dir,
            cargo_target_dir=p.cargo_target_dir,
            gateway=gateway,
            resume=True,
        )
        fr_runner = FindRunner(
            runs_dir=p.runs_dir,
            find_workspaces_dir=p.find_workspaces_dir,
            cargo_target_dir=p.cargo_target_dir,
            gateway=gateway,
            resume=True,
        )

        build_items: list[WorkItem[tuple[object, BuildRun]]] = []
        for spec in specs:
            for builder_id in builder_ids:
                for trial_id in range(args.build_trials):
                    build_seed = _select_build_seed(
                        p.runs_dir,
                        task_id=spec.task_id,
                        builder_id=builder_id,
                        trial_id=trial_id,
                        planned_seed=args.seed + trial_id,
                        recovery_seed=args.recovery_seed + trial_id,
                    )
                    build_items.append(
                        WorkItem(
                            name=f"build {spec.task_id} {builder_id} t{trial_id}",
                            run=lambda spec=spec,
                            builder_id=builder_id,
                            build_seed=build_seed,
                            trial_id=trial_id: _run_build_cell(
                                br_runner,
                                spec,
                                builder_id,
                                build_seed,
                                trial_id,
                            ),
                        )
                    )

        built = [
            result.value
            for result in run_work_queue(
                build_items,
                workers=args.build_workers,
                on_result=_print_build_progress,
            )
        ]

        banks = {
            spec.task_id: load_question_bank(p.questions_dir / f"{spec.task_id}.yaml")
            for spec, _build in built
        }
        find_items: list[WorkItem[FindRun]] = []
        for spec, build in built:
            bank = banks[spec.task_id]
            for finder_id in finder_ids:
                for trial_id in range(args.find_trials):
                    find_seed = _select_find_seed(
                        p.runs_dir,
                        build_run_id=build.build_run_id,
                        finder_id=finder_id,
                        trial_id=trial_id,
                        planned_seed=args.seed + trial_id,
                        recovery_seed=args.recovery_seed + trial_id,
                        keep_terminal_failure=args.keep_terminal_find_failures,
                    )
                    if find_seed is None:
                        continue
                    find_items.append(
                        WorkItem(
                            name=f"find {build.build_run_id} {finder_id} t{trial_id}",
                            run=lambda spec=spec,
                            bank=bank,
                            build=build,
                            finder_id=finder_id,
                            find_seed=find_seed,
                            trial_id=trial_id: _run_find_cell(
                                fr_runner,
                                spec,
                                bank,
                                build,
                                finder_id,
                                find_seed,
                                trial_id,
                            ),
                        )
                    )
        run_work_queue(find_items, workers=args.find_workers, on_result=_print_find_progress)
    finally:
        if gateway is not None:
            gateway.stop()

    if args.run_controls:
        _run_controls(args)

    if args.run_analysis:
        _run_analysis_outputs()


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tasks", default=DEFAULT_TASKS)
    parser.add_argument("--builders", default=DEFAULT_AGENTS)
    parser.add_argument("--finders", default=DEFAULT_AGENTS)
    parser.add_argument("--build-trials", type=int, default=2)
    parser.add_argument("--find-trials", type=int, default=3)
    parser.add_argument("--seed", type=int, default=20260501)
    parser.add_argument("--recovery-seed", type=int, default=20261501)
    parser.add_argument("--no-proxy", action="store_true")
    parser.add_argument("--no-smoke-claude", dest="smoke_claude", action="store_false")
    parser.add_argument("--no-controls", dest="run_controls", action="store_false")
    parser.add_argument("--no-analysis", dest="run_analysis", action="store_false")
    parser.add_argument("--build-workers", type=int, default=1)
    parser.add_argument("--find-workers", type=int, default=1)
    parser.add_argument(
        "--keep-terminal-find-failures",
        action="store_true",
        help=(
            "Treat existing non-infrastructure failed find records, such as model timeouts, "
            "as terminal evidence and continue later cells instead of retrying them."
        ),
    )
    parser.set_defaults(smoke_claude=True, run_controls=True, run_analysis=True)
    return parser.parse_args()


def _run_build_cell(
    runner: BuildRunner,
    spec,
    builder_id: str,
    seed: int,
    trial_id: int,
) -> tuple[object, BuildRun]:
    br = runner.run(spec, get_agent(builder_id), seed=seed, trial_id=trial_id)
    if br.status != "ok":
        raise RuntimeError(f"build {br.build_run_id} ended with status={br.status}")
    return spec, br


def _run_find_cell(
    runner: FindRunner,
    spec,
    bank,
    build: BuildRun,
    finder_id: str,
    seed: int,
    trial_id: int,
) -> FindRun:
    fr = runner.run(spec, bank, build, get_agent(finder_id), seed=seed, trial_id=trial_id)
    if fr.status != "ok":
        raise RuntimeError(f"find {fr.find_run_id} ended with status={fr.status}")
    return fr


def _print_build_progress(
    result: WorkResult[tuple[object, BuildRun]],
    completed: int,
    total: int,
) -> None:
    _spec, br = result.value
    print(
        f"build {completed}/{total} {br.build_run_id} "
        f"status={br.status} elapsed={result.elapsed_seconds:.1f}s"
    )


def _print_find_progress(result: WorkResult[FindRun], completed: int, total: int) -> None:
    fr = result.value
    print(
        f"find {completed}/{total} {fr.find_run_id} "
        f"status={fr.status} acc={fr.accuracy:.2f} elapsed={result.elapsed_seconds:.1f}s"
    )


def _split_csv(value: str) -> list[str]:
    return [part.strip() for part in value.split(",") if part.strip()]


def _select_build_seed(
    records_dir: Path,
    *,
    task_id: str,
    builder_id: str,
    trial_id: int,
    planned_seed: int,
    recovery_seed: int,
) -> int:
    planned_id = _mk_build_id(task_id, builder_id, trial_id, planned_seed)
    planned = _load_build(records_dir, planned_id)
    if planned is None:
        return planned_seed
    if planned.status == "ok":
        return planned_seed
    _require_infrastructure_failure(records_dir, planned_id, planned.status, planned.error)
    return _first_available_recovery_seed(
        records_dir,
        lambda seed: _mk_build_id(task_id, builder_id, trial_id, seed),
        recovery_seed,
    )


def _select_find_seed(
    records_dir: Path,
    *,
    build_run_id: str,
    finder_id: str,
    trial_id: int,
    planned_seed: int,
    recovery_seed: int,
    keep_terminal_failure: bool,
) -> int | None:
    planned_id = _mk_find_id(build_run_id, finder_id, trial_id, planned_seed)
    planned = _load_find(records_dir, planned_id)
    if planned is None:
        return planned_seed
    if planned.status == "ok":
        return None
    infra_reason = _detect_record_infrastructure_failure(
        records_dir,
        planned_id,
        planned.status,
        planned.error,
    )
    if infra_reason is None:
        if keep_terminal_failure:
            print(
                f"preserve {planned_id} (terminal find status={planned.status}); "
                "continuing without retry"
            )
            return None
        _raise_non_infrastructure_failure(planned_id, planned.status)
    print(f"preserve {planned_id} ({infra_reason}); using recovery seed")
    return _first_available_recovery_seed(
        records_dir,
        lambda seed: _mk_find_id(build_run_id, finder_id, trial_id, seed),
        recovery_seed,
    )


def _first_available_recovery_seed(records_dir: Path, mk_run_id, recovery_seed: int) -> int:
    for attempt in range(100):
        seed = recovery_seed + attempt * 1000
        run_id = mk_run_id(seed)
        path = records_dir / f"{run_id}.json"
        payload = load_run_json(path)
        if payload is None:
            if path.exists():
                raise RuntimeError(f"record exists but is not valid JSON: {path}")
            return seed
        status = payload.get("status")
        if status == "ok":
            return seed
        _require_infrastructure_failure(records_dir, run_id, str(status), payload.get("error"))
    raise RuntimeError(f"no available recovery seed near {recovery_seed}")


def _load_build(records_dir: Path, run_id: str) -> BuildRun | None:
    path = records_dir / f"{run_id}.json"
    payload = load_run_json(path)
    if payload is None:
        if path.exists():
            raise RuntimeError(f"build record exists but is not valid JSON: {path}")
        return None
    return BuildRun.model_validate(payload)


def _load_find(records_dir: Path, run_id: str) -> FindRun | None:
    path = records_dir / f"{run_id}.json"
    payload = load_run_json(path)
    if payload is None:
        if path.exists():
            raise RuntimeError(f"find record exists but is not valid JSON: {path}")
        return None
    return FindRun.model_validate(payload)


def _require_infrastructure_failure(
    records_dir: Path,
    run_id: str,
    status: str,
    error: str | None,
) -> None:
    reason = _detect_record_infrastructure_failure(records_dir, run_id, status, error)
    if reason is None:
        _raise_non_infrastructure_failure(run_id, status)
    print(f"preserve {run_id} ({reason}); using recovery seed")


def _detect_record_infrastructure_failure(
    records_dir: Path,
    run_id: str,
    status: str,
    error: str | None,
) -> str | None:
    logs_dir = records_dir.parent / "artifacts" / f"{run_id}.logs"
    stdout = _read_if_exists(logs_dir / "stdout.txt")
    stderr = _read_if_exists(logs_dir / "stderr.txt")
    if not stdout and not stderr:
        logs_dir = records_dir / f"{run_id}.logs"
        stdout = _read_if_exists(logs_dir / "stdout.txt")
        stderr = _read_if_exists(logs_dir / "stderr.txt")
    result = AgentResult(
        status=status,
        stdout=stdout,
        stderr=stderr,
        usage=UsageMetrics(),
        error=error,
    )
    return detect_infrastructure_failure(result)


def _raise_non_infrastructure_failure(run_id: str, status: str) -> None:
    raise RuntimeError(
        f"record {run_id} has status={status!r}, but is not a recognized "
        "infrastructure failure; refusing automatic replacement"
    )


def _read_if_exists(path: Path) -> str:
    if not path.exists():
        return ""
    return path.read_text(encoding="utf-8", errors="replace")


def _smoke_claude() -> None:
    proc = subprocess.run(
        [
            "claude",
            "--print",
            "--model",
            "claude-sonnet-4-6",
            "--output-format",
            "stream-json",
            "--verbose",
            "--dangerously-skip-permissions",
            "--effort",
            "low",
        ],
        input="Reply with OK only.\n",
        text=True,
        capture_output=True,
        check=False,
        timeout=60,
    )
    result = AgentResult(
        status="ok" if proc.returncode == 0 else "error",
        stdout=proc.stdout,
        stderr=proc.stderr,
        usage=UsageMetrics(),
        error=None if proc.returncode == 0 else f"exit code {proc.returncode}",
    )
    reason = detect_infrastructure_failure(result)
    if proc.returncode != 0 or reason is not None:
        raise RuntimeError(
            "Claude smoke failed; refusing to continue formal run. "
            f"reason={reason or result.error}; stdout={proc.stdout[-1000:]}"
        )
    print("claude smoke ok")


def _run_controls(args: argparse.Namespace) -> None:
    cmd = [
        "uv",
        "run",
        "python",
        "scripts/run_controls.py",
        "--tasks",
        args.tasks,
        "--finders",
        args.finders,
        "--trials",
        str(args.find_trials),
        "--seed",
        str(args.seed),
        "--recovery-seed",
        str(args.recovery_seed),
        "--conditions",
        "spec_only,question_only",
        "--workers",
        str(args.find_workers),
    ]
    if args.no_proxy:
        cmd.append("--no-proxy")
    subprocess.run(cmd, check=True)


def _run_analysis_outputs() -> None:
    for cmd in (
        ["uv", "run", "bnf", "analyze", "tables"],
        ["uv", "run", "python", "scripts/analyze_eandd_final.py"],
        ["uv", "run", "python", "scripts/analyze_eandd_pivots.py"],
        ["uv", "run", "python", "scripts/analyze_eandd_within_finder_efficiency.py"],
        ["uv", "run", "python", "scripts/write_reproducibility_index.py"],
    ):
        subprocess.run(cmd, check=True)


if __name__ == "__main__":
    main()
