"""Load run records and export aggregated reports."""

from __future__ import annotations

import csv
import json
import statistics
from pathlib import Path

from bnf.analysis.legibility import (
    DEFAULT_CONSENSUS_THRESHOLD,
    DEFAULT_LEGIBILITY_THRESHOLD,
    compute_legibility_per_builder,
)
from bnf.analysis.matrix import (
    bootstrap_ci,
    build_builder_finder_matrix,
    iter_observations,
)
from bnf.schema.run import BuildRun, FindRun


def _load_run_file(path: Path, cls):
    return cls.model_validate_json(path.read_text(encoding="utf-8"))


def load_build_runs(runs_dir: Path) -> list[BuildRun]:
    out: list[BuildRun] = []
    for f in sorted(runs_dir.glob("build-*.json")):
        try:
            out.append(_load_run_file(f, BuildRun))
        except Exception:
            continue
    return out


def load_find_runs(runs_dir: Path) -> list[FindRun]:
    out: list[FindRun] = []
    for f in sorted(runs_dir.glob("find-*.json")):
        try:
            out.append(_load_run_file(f, FindRun))
        except Exception:
            continue
    return out


def export_csv(build_runs: list[BuildRun], find_runs: list[FindRun], out_path: Path) -> Path:
    out_path.parent.mkdir(parents=True, exist_ok=True)
    obs = iter_observations(build_runs, find_runs)
    find_by_id = {fr.find_run_id: fr for fr in find_runs}
    with out_path.open("w", newline="", encoding="utf-8") as f:
        w = csv.writer(f, lineterminator="\n")
        w.writerow(
            [
                "task_id",
                "builder_id",
                "build_run_id",
                "finder_id",
                "find_run_id",
                "question_id",
                "is_correct",
                "answered",
                "retrieval_payload_bytes",
                "tool_calls",
                "vendor_total_tokens",
                "usage_source",
                "session_id",
            ]
        )
        for o in obs:
            fr = find_by_id[o.find_run_id]
            w.writerow(
                [
                    o.task_id,
                    o.builder_id,
                    o.build_run_id,
                    o.finder_id,
                    o.find_run_id,
                    o.question_id,
                    o.is_correct,
                    o.answered,
                    fr.usage.retrieval_payload_bytes,
                    fr.usage.tool_calls,
                    fr.usage.vendor_usage.total_tokens if fr.usage.vendor_usage else "",
                    fr.usage.usage_source or "",
                    fr.usage.session_id or "",
                ]
            )
    return out_path


def export_json(
    build_runs: list[BuildRun],
    find_runs: list[FindRun],
    out_path: Path,
    *,
    consensus_threshold: float = DEFAULT_CONSENSUS_THRESHOLD,
    legibility_threshold: float = DEFAULT_LEGIBILITY_THRESHOLD,
) -> Path:
    out_path.parent.mkdir(parents=True, exist_ok=True)
    matrix = build_builder_finder_matrix(build_runs, find_runs)
    # Per-builder and per-finder marginal accuracy with bootstrap CI.
    obs = iter_observations(build_runs, find_runs)
    legibility = compute_legibility_per_builder(
        build_runs,
        find_runs,
        consensus_threshold=consensus_threshold,
        legibility_threshold=legibility_threshold,
    )
    per_builder: dict[str, dict] = {}
    for b in matrix["builders"]:
        samples = [o.is_correct for o in obs if o.builder_id == b]
        lo, hi = bootstrap_ci(samples)
        leg = legibility.get(b)
        per_builder[b] = {
            "accuracy": (sum(samples) / len(samples)) if samples else None,
            "ci95": [lo, hi],
            "n": len(samples),
            "convergent_miss_rate": leg.convergent_miss_rate if leg else None,
            "convergent_miss_cells": leg.convergent_miss_cells if leg else 0,
            "inter_finder_agreement": leg.inter_finder_agreement if leg else None,
            "high_agreement_cells": leg.high_agreement_cells if leg else 0,
            "n_question_cells": leg.n_question_cells if leg else 0,
            "convergent_miss_events": [
                {
                    "task_id": ev[0],
                    "question_id": ev[1],
                    "picked_option": ev[2],
                    "n_picked": ev[3],
                    "n_finders": ev[4],
                }
                for ev in (leg.convergent_miss_events if leg else ())
            ],
        }
    per_finder: dict[str, dict] = {}
    for f in matrix["finders"]:
        samples = [o.is_correct for o in obs if o.finder_id == f]
        lo, hi = bootstrap_ci(samples)
        usage = _summarize_finder_usage([fr for fr in find_runs if fr.manifest.agent.id == f])
        per_finder[f] = {
            "accuracy": (sum(samples) / len(samples)) if samples else None,
            "ci95": [lo, hi],
            "n": len(samples),
            **usage,
        }
    payload = {
        "matrix": matrix,
        "retrieval_payload_matrix": build_builder_finder_matrix(
            build_runs, find_runs, metric="retrieval_payload_bytes"
        ),
        "vendor_total_tokens_matrix": build_builder_finder_matrix(
            build_runs, find_runs, metric="vendor_total_tokens"
        ),
        "per_builder": per_builder,
        "per_finder": per_finder,
        "n_builds": len(build_runs),
        "n_finds": len(find_runs),
        "thresholds": {
            "consensus": consensus_threshold,
            "legibility": legibility_threshold,
        },
    }
    out_path.write_text(json.dumps(payload, indent=2), encoding="utf-8")
    return out_path


def export_markdown(
    build_runs: list[BuildRun],
    find_runs: list[FindRun],
    out_path: Path,
    *,
    consensus_threshold: float = DEFAULT_CONSENSUS_THRESHOLD,
    legibility_threshold: float = DEFAULT_LEGIBILITY_THRESHOLD,
) -> Path:
    out_path.parent.mkdir(parents=True, exist_ok=True)
    matrix = build_builder_finder_matrix(build_runs, find_runs)
    builders = matrix["builders"]
    finders = matrix["finders"]
    lines: list[str] = []
    lines.append("# Build-and-Find results\n")
    lines.append(f"- builds: {len(build_runs)}  finds: {len(find_runs)}\n")
    lines.append("\n## Builder × Finder accuracy\n")
    header = ["Builder \\ Finder", *finders]
    lines.append("| " + " | ".join(header) + " |")
    lines.append("|" + "|".join(["---"] * len(header)) + "|")
    for b in builders:
        row = [b]
        for f in finders:
            v = matrix["values"][b][f]
            row.append("--" if v != v else f"{v:.2f}")  # NaN check
        lines.append("| " + " | ".join(row) + " |")

    legibility = compute_legibility_per_builder(
        build_runs,
        find_runs,
        consensus_threshold=consensus_threshold,
        legibility_threshold=legibility_threshold,
    )
    if legibility:
        lines.append("\n## Builder fidelity vs legibility\n")
        lines.append(
            f"- *convergent miss* = ≥ {consensus_threshold:.0%} of finders all picked the same wrong option "
            f"(strong evidence the builder diverged from spec)."
        )
        lines.append(
            f"- *inter-finder agreement* = ≥ {legibility_threshold:.0%} of finders picked the same option "
            f"(any), measuring how unambiguously the codebase reads."
        )
        lines.append("")
        lines.append("| Builder | Cells | Convergent-miss rate | Inter-finder agreement |")
        lines.append("|---|---|---|---|")
        for b in builders:
            leg = legibility.get(b)
            if leg is None or leg.n_question_cells == 0:
                lines.append(f"| {b} | 0 | -- | -- |")
                continue
            lines.append(
                f"| {b} | {leg.n_question_cells} "
                f"| {leg.convergent_miss_rate:.2%} ({leg.convergent_miss_cells}) "
                f"| {leg.inter_finder_agreement:.2%} ({leg.high_agreement_cells}) |"
            )
        # Surface every individual convergent-miss event so a builder
        # ranking that depends on the rate is auditable: the reader can
        # see exactly which spec point each builder flunked.
        any_events = any(leg.convergent_miss_events for leg in legibility.values())
        if any_events:
            lines.append("\n### Convergent-miss events (builder spec-divergence audit)\n")
            lines.append("| Builder | Task | Question | Finders agreeing on wrong option |")
            lines.append("|---|---|---|---|")
            for b in builders:
                leg = legibility.get(b)
                if leg is None:
                    continue
                for task_id, qid, picked, n_picked, n_total in leg.convergent_miss_events:
                    lines.append(f"| {b} | {task_id} | {qid} | {picked} ({n_picked}/{n_total}) |")

    lines.append("\n## Finder efficiency\n")
    lines.append(
        "| Finder | Accuracy | Retrieval KiB / run | Tool calls / run | Vendor tokens / run | Correct / MiB |"
    )
    lines.append("|---|---|---|---|---|---|")
    for finder in finders:
        summary = _summarize_finder_usage(
            [fr for fr in find_runs if fr.manifest.agent.id == finder]
        )
        acc = summary["accuracy"]
        retrieval_kib = summary["mean_retrieval_payload_bytes"]
        vendor_tokens = summary["mean_vendor_total_tokens"]
        correct_per_mib = summary["correct_per_mib_retrieval"]
        lines.append(
            "| "
            + " | ".join(
                [
                    finder,
                    "--" if acc is None else f"{acc:.2f}",
                    "--" if retrieval_kib is None else f"{retrieval_kib / 1024:.1f}",
                    "--"
                    if summary["mean_tool_calls"] is None
                    else f"{summary['mean_tool_calls']:.1f}",
                    "--" if vendor_tokens is None else f"{vendor_tokens:.0f}",
                    "--" if correct_per_mib is None else f"{correct_per_mib:.2f}",
                ]
            )
            + " |"
        )
    out_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return out_path


def _summarize_finder_usage(find_runs: list[FindRun]) -> dict[str, float | int | None]:
    if not find_runs:
        return {
            "accuracy": None,
            "n_runs": 0,
            "mean_retrieval_payload_bytes": None,
            "median_retrieval_payload_bytes": None,
            "mean_tool_calls": None,
            "mean_vendor_total_tokens": None,
            "correct_per_mib_retrieval": None,
            "correct_per_1k_vendor_tokens": None,
        }

    retrieval = [fr.usage.retrieval_payload_bytes for fr in find_runs]
    tool_calls = [fr.usage.tool_calls for fr in find_runs]
    vendor_tokens = [
        fr.usage.vendor_usage.total_tokens
        for fr in find_runs
        if fr.usage.vendor_usage is not None and fr.usage.vendor_usage.total_tokens > 0
    ]
    correct = sum(sum(1 for ans in fr.answers if ans.is_correct) for fr in find_runs)
    total_questions = sum(len(fr.answers) for fr in find_runs)
    total_retrieval = sum(retrieval)
    total_vendor_tokens = sum(vendor_tokens)

    return {
        "accuracy": (correct / total_questions) if total_questions else None,
        "n_runs": len(find_runs),
        "mean_retrieval_payload_bytes": statistics.fmean(retrieval),
        "median_retrieval_payload_bytes": statistics.median(retrieval),
        "mean_tool_calls": statistics.fmean(tool_calls),
        "mean_vendor_total_tokens": statistics.fmean(vendor_tokens) if vendor_tokens else None,
        "correct_per_mib_retrieval": (
            correct / (total_retrieval / (1024 * 1024)) if total_retrieval > 0 else None
        ),
        "correct_per_1k_vendor_tokens": (
            correct / (total_vendor_tokens / 1000) if total_vendor_tokens > 0 else None
        ),
    }
