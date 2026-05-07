"""Rerun one Find logical cell with an explicit seed.

This is for append-only recovery of a single raw record. It never
overwrites the original failed record because the caller supplies a new
seed, producing a distinct find_run_id for the same
``(build_run_id, finder_id, trial_id)`` logical cell.
"""

from __future__ import annotations

import argparse

from bnf.config import paths
from bnf.find.runner import FindRunner
from bnf.proxy import Gateway
from bnf.runners import get_agent
from bnf.schema import load_question_bank, load_specs_dir
from bnf.schema.run import BuildRun, load_run_json


def main() -> None:
    args = _parse_args()
    p = paths()

    payload = load_run_json(p.runs_dir / f"{args.build_run_id}.json")
    if payload is None:
        raise SystemExit(f"unknown build_run_id: {args.build_run_id}")
    build = BuildRun.model_validate(payload)
    if build.status != "ok":
        raise SystemExit(f"refusing non-ok build {build.build_run_id}: {build.status}")

    specs = {spec.task_id: spec for spec in load_specs_dir(p.specs_dir)}
    spec = specs.get(build.task_id)
    if spec is None:
        raise SystemExit(f"unknown task_id for build {build.build_run_id}: {build.task_id}")
    bank = load_question_bank(p.questions_dir / f"{spec.task_id}.yaml")

    gateway = None if args.no_proxy else Gateway.start()
    if gateway is not None:
        print(f"proxy listening on {gateway.base_url}")
    try:
        runner = FindRunner(
            runs_dir=p.runs_dir,
            find_workspaces_dir=p.find_workspaces_dir,
            cargo_target_dir=p.cargo_target_dir,
            gateway=gateway,
            resume=True,
        )
        result = runner.run(
            spec,
            bank,
            build,
            get_agent(args.finder),
            seed=args.seed,
            trial_id=args.trial_id,
        )
        print(f"{result.find_run_id} status={result.status} error={result.error}")
    finally:
        if gateway is not None:
            gateway.stop()


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--build-run-id", required=True)
    parser.add_argument("--finder", required=True)
    parser.add_argument("--trial-id", type=int, required=True)
    parser.add_argument("--seed", type=int, required=True)
    parser.add_argument("--no-proxy", action="store_true")
    return parser.parse_args()


if __name__ == "__main__":
    main()
