"""Build-and-Find command-line interface (``bnf``).

Top-level commands::

    bnf specs list / show / validate / render-prompt
    bnf questions list / validate / audit
    bnf build run / list
    bnf find  run
    bnf run   all          # builds + finds + report end-to-end
    bnf analyze matrix / report
    bnf agents list
"""

from __future__ import annotations

import json
import logging
from pathlib import Path
from typing import Optional

import typer
from rich.console import Console
from rich.table import Table

from bnf.analysis import (
    DEFAULT_EFFICIENCY_AXES,
    build_builder_finder_matrix,
    export_analysis_tables,
    export_csv,
    export_json,
    export_markdown,
    load_build_runs,
    load_find_runs,
    pairwise_dominance_finders,
    scatter_rows,
)
from bnf.build import BuildRunner
from bnf.config import paths
from bnf.find import FindRunner
from bnf.orchestration import WorkItem, WorkResult, run_work_queue
from bnf.prompts import render_builder_prompt
from bnf.proxy import Gateway
from bnf.runners import get_agent, list_agents
from bnf.schema import BuildRun, BuildTaskSpec, FindRun, load_question_bank, load_specs_dir

logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(name)s %(message)s")
console = Console()

PRIMARY_TASK_FAMILY = "build_from_scratch"


def _start_proxy_if_enabled(enabled: bool) -> Optional[Gateway]:
    """Start an Observation-layer proxy if the caller opted in.

    The proxy captures per-run efficiency (paper §17.3). Returns
    ``None`` when disabled, in which case adapter runs fall back to the
    legacy session-log usage tracing path.
    """

    if not enabled:
        return None
    gw = Gateway.start()
    console.print(f"[dim]proxy listening on {gw.base_url}[/dim]")
    return gw


app = typer.Typer(no_args_is_help=True, help="Build-and-Find benchmark CLI")
specs_app = typer.Typer(no_args_is_help=True, help="Inspect Build specs")
questions_app = typer.Typer(no_args_is_help=True, help="Inspect Find question banks")
build_app = typer.Typer(no_args_is_help=True, help="Build-phase commands")
find_app = typer.Typer(no_args_is_help=True, help="Find-phase commands")
run_app = typer.Typer(no_args_is_help=True, help="Orchestration commands")
analyze_app = typer.Typer(no_args_is_help=True, help="Analysis and reporting")
agents_app = typer.Typer(no_args_is_help=True, help="List configured agents")

app.add_typer(specs_app, name="specs")
app.add_typer(questions_app, name="questions")
app.add_typer(build_app, name="build")
app.add_typer(find_app, name="find")
app.add_typer(run_app, name="run")
app.add_typer(analyze_app, name="analyze")
app.add_typer(agents_app, name="agents")


# ---------------------------------------------------------------------------
# specs
# ---------------------------------------------------------------------------


@specs_app.command("list")
def specs_list() -> None:
    specs = load_specs_dir(paths().specs_dir)
    t = Table(title="Build specs")
    t.add_column("task_id")
    t.add_column("family")
    t.add_column("title")
    t.add_column("#FR")
    t.add_column("#SR")
    t.add_column("#ER")
    t.add_column("#intent")
    for s in specs:
        t.add_row(
            s.task_id,
            s.task_family,
            s.title,
            str(len(s.functional_requirements)),
            str(len(s.structural_requirements)),
            str(len(s.edit_requirements)),
            str(len(s.intentions.all())),
        )
    console.print(t)


@specs_app.command("show")
def specs_show(task_id: str) -> None:
    s = _find_spec(task_id)
    console.print_json(s.model_dump_json())


@specs_app.command("validate")
def specs_validate() -> None:
    """Load every spec and check structural invariants + question cross-refs."""
    errors: list[str] = []
    p = paths()
    for s in load_specs_dir(p.specs_dir):
        q_path = p.questions_dir / f"{s.task_id}.yaml"
        if q_path.exists():
            bank = load_question_bank(q_path)
            errors.extend(bank.cross_validate(s))
        else:
            errors.append(f"{s.task_id}: no question bank at {q_path}")
    if errors:
        for e in errors:
            console.print(f"[red]✗[/red] {e}")
        raise typer.Exit(code=1)
    console.print("[green]All specs and question banks valid.[/green]")


@specs_app.command("render-prompt")
def specs_render_prompt(task_id: str) -> None:
    s = _find_spec(task_id)
    console.print(render_builder_prompt(s))


# ---------------------------------------------------------------------------
# questions
# ---------------------------------------------------------------------------


@questions_app.command("list")
def questions_list() -> None:
    p = paths()
    t = Table(title="Question banks")
    t.add_column("task_id")
    t.add_column("#questions")
    t.add_column("types")
    for q_path in sorted(p.questions_dir.glob("*.yaml")):
        bank = load_question_bank(q_path)
        types = sorted({q.question_type for q in bank.questions})
        t.add_row(bank.task_id, str(len(bank.questions)), ", ".join(types))
    console.print(t)


@questions_app.command("audit")
def questions_audit(task_id: str) -> None:
    """Detect obvious design defects in a question bank.

    Checks:
    - duplicate option texts
    - distractors that textually contain the correct-option text (potential
      leakage)
    - questions whose correct option is also the alphabetically first,
      which can bias weak finders
    """
    p = paths()
    import re

    bank = load_question_bank(p.questions_dir / f"{task_id}.yaml")
    issues = 0
    for q in bank.questions:
        correct_text = q.options[q.correct_option].strip().lower()
        # Use word-boundary match to avoid false positives like
        # "seconds" ⊂ "milliseconds".
        pattern = re.compile(rf"\b{re.escape(correct_text)}\b")
        for k, v in q.options.items():
            if k == q.correct_option:
                continue
            if correct_text and pattern.search(v.strip().lower()):
                console.print(
                    f"[yellow]warn[/yellow] {q.question_id}: distractor {k!r} "
                    f"contains correct-option text"
                )
                issues += 1
    if issues == 0:
        console.print("[green]No obvious issues.[/green]")


# ---------------------------------------------------------------------------
# agents
# ---------------------------------------------------------------------------


@agents_app.command("list")
def agents_list() -> None:
    t = Table(title="Registered agents")
    t.add_column("id")
    t.add_column("adapter")
    t.add_column("model_id")
    t.add_column("effort")
    t.add_column("cli_bin")
    for a in list_agents():
        t.add_row(
            a.id,
            a.family,
            a.model_id,
            a.reasoning_effort or "default",
            a.cli_bin,
        )
    console.print(t)


# ---------------------------------------------------------------------------
# build
# ---------------------------------------------------------------------------


@build_app.command("run")
def build_run(
    task: str = typer.Option(..., "--task", help="Build task id"),
    builder: str = typer.Option(..., "--builder", help="Agent id"),
    trials: int = typer.Option(1, "--trials", min=1),
    seed: int = typer.Option(0, "--seed"),
    proxy: bool = typer.Option(True, "--proxy/--no-proxy", help="Capture efficiency via proxy"),
    resume: bool = typer.Option(
        True,
        "--resume/--force",
        help="Skip runs that already have a successful record on disk",
    ),
) -> None:
    p = paths()
    spec = _find_spec(task)
    adapter = get_agent(builder)
    gw = _start_proxy_if_enabled(proxy)
    try:
        runner = BuildRunner(
            specs_dir=p.specs_dir,
            artifacts_dir=p.artifacts_dir,
            runs_dir=p.runs_dir,
            cargo_target_dir=p.cargo_target_dir,
            gateway=gw,
            resume=resume,
        )
        for i in range(trials):
            br = runner.run(spec, adapter, seed=seed + i, trial_id=i)
            _print_build_run(br)
    finally:
        if gw is not None:
            gw.stop()


@build_app.command("list")
def build_list() -> None:
    runs = load_build_runs(paths().runs_dir)
    t = Table(title="Build runs")
    t.add_column("build_run_id")
    t.add_column("task_id")
    t.add_column("builder")
    t.add_column("status")
    t.add_column("probe")
    for br in runs:
        t.add_row(
            br.build_run_id,
            br.task_id,
            br.manifest.agent.id,
            br.status,
            f"compile={br.validation.compile_pass} loc={br.validation.rust_loc}",
        )
    console.print(t)


# ---------------------------------------------------------------------------
# find
# ---------------------------------------------------------------------------


@find_app.command("run")
def find_run(
    codebase: str = typer.Option(..., "--codebase", help="build_run_id to consume"),
    finder: str = typer.Option(..., "--finder", help="Finder agent id"),
    trials: int = typer.Option(1, "--trials", min=1),
    seed: int = typer.Option(0, "--seed"),
    proxy: bool = typer.Option(True, "--proxy/--no-proxy", help="Capture efficiency via proxy"),
    resume: bool = typer.Option(
        True,
        "--resume/--force",
        help="Skip runs that already have a successful record on disk",
    ),
) -> None:
    p = paths()
    build_runs = {br.build_run_id: br for br in load_build_runs(p.runs_dir)}
    if codebase not in build_runs:
        raise typer.BadParameter(f"unknown build_run_id {codebase!r}")
    br = build_runs[codebase]
    spec = _find_spec(br.task_id)
    bank = load_question_bank(p.questions_dir / f"{spec.task_id}.yaml")
    gw = _start_proxy_if_enabled(proxy)
    try:
        runner = FindRunner(
            runs_dir=p.runs_dir,
            find_workspaces_dir=p.find_workspaces_dir,
            cargo_target_dir=p.cargo_target_dir,
            gateway=gw,
            resume=resume,
        )
        adapter = get_agent(finder)
        for i in range(trials):
            fr = runner.run(spec, bank, br, adapter, seed=seed + i, trial_id=i)
            acc = fr.accuracy
            correct = sum(a.is_correct for a in fr.answers)
            parts = [
                f"[bold]{fr.find_run_id}[/bold]",
                f"acc={acc:.2f} ({correct}/{len(fr.answers)})",
            ]
            eff = fr.usage.efficiency
            if eff is not None:
                parts.append(f"retrieval={eff.novel_retrieval_bytes / 1024:.1f}KiB")
                parts.append(f"tool_calls={eff.novel_tool_call_count}")
                if eff.generation_tok_per_s:
                    parts.append(f"tok/s={eff.generation_tok_per_s:.1f}")
            else:
                parts.append(f"retrieval={fr.usage.retrieval_payload_bytes / 1024:.1f}KiB")
            console.print(" ".join(parts))
    finally:
        if gw is not None:
            gw.stop()


# ---------------------------------------------------------------------------
# run all
# ---------------------------------------------------------------------------


@run_app.command("all")
def run_all(
    builders: Optional[str] = typer.Option(None, "--builders", help="Comma-separated agent ids"),
    finders: Optional[str] = typer.Option(None, "--finders", help="Comma-separated agent ids"),
    tasks: Optional[str] = typer.Option(
        None,
        "--tasks",
        help="Comma-separated task ids. Defaults to build_from_scratch tasks only.",
    ),
    include_edit: bool = typer.Option(
        False,
        "--include-edit",
        help="Include edit_existing_project tasks when --tasks is omitted.",
    ),
    trials: Optional[int] = typer.Option(
        None,
        "--trials",
        min=1,
        help="Set both build and find trial counts. Mutually exclusive with split trial flags.",
    ),
    build_trials: Optional[int] = typer.Option(None, "--build-trials", min=1),
    find_trials: Optional[int] = typer.Option(None, "--find-trials", min=1),
    seed: int = typer.Option(0, "--seed"),
    proxy: bool = typer.Option(True, "--proxy/--no-proxy", help="Capture efficiency via proxy"),
    resume: bool = typer.Option(
        True,
        "--resume/--force",
        help="Skip runs that already have a successful record on disk",
    ),
    build_workers: int = typer.Option(1, "--build-workers", min=1),
    find_workers: int = typer.Option(1, "--find-workers", min=1),
) -> None:
    p = paths()
    all_specs = load_specs_dir(p.specs_dir)
    task_ids = _resolve_task_ids(all_specs, tasks=tasks, include_edit=include_edit)
    all_specs = [s for s in all_specs if s.task_id in task_ids]
    builder_ids = builders.split(",") if builders else [a.id for a in list_agents()]
    finder_ids = finders.split(",") if finders else [a.id for a in list_agents()]
    build_trial_count, find_trial_count = _resolve_trial_counts(
        trials=trials,
        build_trials=build_trials,
        find_trials=find_trials,
    )

    gw = _start_proxy_if_enabled(proxy)
    try:
        br_runner = BuildRunner(
            specs_dir=p.specs_dir,
            artifacts_dir=p.artifacts_dir,
            runs_dir=p.runs_dir,
            cargo_target_dir=p.cargo_target_dir,
            gateway=gw,
            resume=resume,
        )
        fr_runner = FindRunner(
            runs_dir=p.runs_dir,
            find_workspaces_dir=p.find_workspaces_dir,
            cargo_target_dir=p.cargo_target_dir,
            gateway=gw,
            resume=resume,
        )

        build_items: list[WorkItem[tuple[BuildTaskSpec, BuildRun]]] = []
        for spec in all_specs:
            for builder_id in builder_ids:
                for trial_id in range(build_trial_count):
                    build_items.append(
                        WorkItem(
                            name=f"build {spec.task_id} {builder_id} t{trial_id}",
                            run=lambda spec=spec, builder_id=builder_id, trial_id=trial_id: (
                                spec,
                                br_runner.run(
                                    spec,
                                    get_agent(builder_id),
                                    seed=seed + trial_id,
                                    trial_id=trial_id,
                                ),
                            ),
                        )
                    )

        def _on_build_done(
            result: WorkResult[tuple[BuildTaskSpec, BuildRun]],
            completed: int,
            total: int,
        ) -> None:
            _spec, br = result.value
            console.print(
                f"[green]build[/green] {completed}/{total} {br.build_run_id} "
                f"status={br.status} elapsed={result.elapsed_seconds:.1f}s"
            )

        built = [
            result.value
            for result in run_work_queue(
                build_items,
                workers=build_workers,
                on_result=_on_build_done,
            )
        ]

        # Failed builds leave a partial artifact on disk. Running finders
        # against it produces noise — they answer questions about an
        # unfinished codebase that no human would call "the build". Skip
        # them; the next --resume cycle will retry the build itself.
        skipped = [(spec, br) for spec, br in built if br.status != "ok"]
        for spec, br in skipped:
            console.print(
                f"[yellow]skip finds[/yellow] for {br.build_run_id} "
                f"(status={br.status}); will retry on next --resume run"
            )
        runnable = [(spec, br) for spec, br in built if br.status == "ok"]

        banks = {
            spec.task_id: load_question_bank(p.questions_dir / f"{spec.task_id}.yaml")
            for spec, _br in runnable
        }
        find_items: list[WorkItem[FindRun]] = []
        for spec, br in runnable:
            bank = banks[spec.task_id]
            for finder_id in finder_ids:
                for trial_id in range(find_trial_count):
                    find_items.append(
                        WorkItem(
                            name=f"find {br.build_run_id} {finder_id} t{trial_id}",
                            run=lambda spec=spec,
                            bank=bank,
                            br=br,
                            finder_id=finder_id,
                            trial_id=trial_id: fr_runner.run(
                                spec,
                                bank,
                                br,
                                get_agent(finder_id),
                                seed=seed + trial_id,
                                trial_id=trial_id,
                            ),
                        )
                    )

        def _on_find_done(result: WorkResult[FindRun], completed: int, total: int) -> None:
            fr = result.value
            console.print(
                f"[green]find[/green] {completed}/{total} {fr.find_run_id} "
                f"status={fr.status} acc={fr.accuracy:.2f} elapsed={result.elapsed_seconds:.1f}s"
            )

        run_work_queue(find_items, workers=find_workers, on_result=_on_find_done)
    finally:
        if gw is not None:
            gw.stop()

    # Final report.
    build_runs, find_runs = _load_scoped_runs(task_ids=task_ids)
    export_markdown(build_runs, find_runs, p.reports_dir / "summary.md")
    console.print(f"[green]done.[/green] report -> {p.reports_dir / 'summary.md'}")


# ---------------------------------------------------------------------------
# analyze
# ---------------------------------------------------------------------------


@analyze_app.command("matrix")
def analyze_matrix(
    metric: str = typer.Option(
        "accuracy",
        "--metric",
        help="accuracy|answered|retrieval_payload_bytes|tool_calls|vendor_total_tokens",
    ),
    tasks: Optional[str] = typer.Option(
        None,
        "--tasks",
        help="Comma-separated task ids. Defaults to build_from_scratch tasks only.",
    ),
    include_edit: bool = typer.Option(
        False,
        "--include-edit",
        help="Include edit_existing_project tasks when --tasks is omitted.",
    ),
) -> None:
    task_ids = _resolve_task_ids(
        load_specs_dir(paths().specs_dir), tasks=tasks, include_edit=include_edit
    )
    br, fr = _load_scoped_runs(task_ids=task_ids)
    m = build_builder_finder_matrix(br, fr, metric=metric)
    console.print_json(json.dumps(m))


@analyze_app.command("dominance")
def analyze_dominance(
    axes: str = typer.Option(
        ",".join(DEFAULT_EFFICIENCY_AXES),
        "--axes",
        help="Comma-separated efficiency axes (all 'lower is better')",
    ),
    out: Optional[Path] = typer.Option(None, "--out"),
    tasks: Optional[str] = typer.Option(
        None,
        "--tasks",
        help="Comma-separated task ids. Defaults to build_from_scratch tasks only.",
    ),
    include_edit: bool = typer.Option(
        False,
        "--include-edit",
        help="Include edit_existing_project tasks when --tasks is omitted.",
    ),
) -> None:
    """Pairwise dominance matrix across finders (paper §17.6)."""

    task_ids = _resolve_task_ids(
        load_specs_dir(paths().specs_dir), tasks=tasks, include_edit=include_edit
    )
    br, fr = _load_scoped_runs(task_ids=task_ids)
    axis_tuple = tuple(a.strip() for a in axes.split(",") if a.strip())
    cells = pairwise_dominance_finders(br, fr, axes=axis_tuple)
    finders = sorted({c.row for c in cells.values()} | {c.col for c in cells.values()})

    t = Table(title="Pairwise dominance rate (row dominates column)")
    t.add_column("row \\ col")
    for f in finders:
        t.add_column(f)
    for a in finders:
        row_cells = [a]
        for b in finders:
            if a == b:
                row_cells.append("—")
                continue
            cell = cells.get((a, b))
            if cell is None or cell.matched_tasks == 0:
                row_cells.append("·")
            else:
                row_cells.append(f"{cell.dominance_rate:.2f} ({cell.wins}/{cell.matched_tasks})")
        t.add_row(*row_cells)
    console.print(t)

    if out:
        import json as _json

        payload = {
            "axes": list(axis_tuple),
            "finders": finders,
            "cells": {
                f"{k[0]}|{k[1]}": {
                    "wins": v.wins,
                    "ties": v.ties,
                    "losses": v.losses,
                    "incomparable": v.incomparable,
                    "matched_tasks": v.matched_tasks,
                    "dominance_rate": v.dominance_rate,
                }
                for k, v in cells.items()
            },
        }
        out.write_text(_json.dumps(payload, indent=2), encoding="utf-8")
        console.print(f"[green]wrote[/green] {out}")


@analyze_app.command("scatter")
def analyze_scatter(
    format: str = typer.Option("csv", "--format", help="csv|json"),
    out: Optional[Path] = typer.Option(None, "--out"),
    tasks: Optional[str] = typer.Option(
        None,
        "--tasks",
        help="Comma-separated task ids. Defaults to build_from_scratch tasks only.",
    ),
    include_edit: bool = typer.Option(
        False,
        "--include-edit",
        help="Include edit_existing_project tasks when --tasks is omitted.",
    ),
) -> None:
    """Per-run accuracy × efficiency points for plotting (paper §17.6)."""

    p = paths()
    task_ids = _resolve_task_ids(
        load_specs_dir(p.specs_dir), tasks=tasks, include_edit=include_edit
    )
    br, fr = _load_scoped_runs(task_ids=task_ids)
    rows = scatter_rows(br, fr)
    out = out or (p.reports_dir / f"scatter.{format}")
    out.parent.mkdir(parents=True, exist_ok=True)
    if format == "csv":
        import csv as _csv

        if not rows:
            out.write_text("", encoding="utf-8")
        else:
            keys = list(rows[0].keys())
            with out.open("w", encoding="utf-8", newline="") as f:
                writer = _csv.DictWriter(f, fieldnames=keys, lineterminator="\n")
                writer.writeheader()
                for row in rows:
                    writer.writerow(row)
    elif format == "json":
        import json as _json

        out.write_text(_json.dumps(rows, indent=2), encoding="utf-8")
    else:
        raise typer.BadParameter(f"unknown format {format!r}")
    console.print(f"[green]wrote[/green] {len(rows)} rows → {out}")


@analyze_app.command("report")
def analyze_report(
    format: str = typer.Option("markdown", "--format", help="markdown|json|csv"),
    out: Optional[Path] = typer.Option(None, "--out"),
    tasks: Optional[str] = typer.Option(
        None,
        "--tasks",
        help="Comma-separated task ids. Defaults to build_from_scratch tasks only.",
    ),
    include_edit: bool = typer.Option(
        False,
        "--include-edit",
        help="Include edit_existing_project tasks when --tasks is omitted.",
    ),
    consensus_threshold: float = typer.Option(
        0.5,
        "--consensus-threshold",
        min=0.0,
        max=1.0,
        help=(
            "Proportion of finders that must agree on the same wrong "
            "option to flag a (builder, codebase, question) cell as a "
            "convergent miss. Default 0.5 (strict majority)."
        ),
    ),
    legibility_threshold: float = typer.Option(
        2 / 3,
        "--legibility-threshold",
        min=0.0,
        max=1.0,
        help=(
            "Proportion of finders that must pick the same option "
            "(correct or not) for a cell to count toward inter-finder "
            "agreement. Default ~0.667 (≥ two-thirds)."
        ),
    ),
) -> None:
    p = paths()
    task_ids = _resolve_task_ids(
        load_specs_dir(p.specs_dir), tasks=tasks, include_edit=include_edit
    )
    br, fr = _load_scoped_runs(task_ids=task_ids)
    out = out or (p.reports_dir / f"report.{format}")
    if format == "markdown":
        export_markdown(
            br,
            fr,
            out,
            consensus_threshold=consensus_threshold,
            legibility_threshold=legibility_threshold,
        )
    elif format == "json":
        export_json(
            br,
            fr,
            out,
            consensus_threshold=consensus_threshold,
            legibility_threshold=legibility_threshold,
        )
    elif format == "csv":
        export_csv(br, fr, out)
    else:
        raise typer.BadParameter(f"unknown format {format!r}")
    console.print(f"[green]wrote[/green] {out}")


@analyze_app.command("tables")
def analyze_tables(
    out_dir: Optional[Path] = typer.Option(None, "--out-dir"),
) -> None:
    """Materialize canonical analysis tables from raw run records."""

    p = paths()
    dest = out_dir or p.tables_dir
    metadata = export_analysis_tables(
        records_dir=p.runs_dir,
        out_dir=dest,
        questions_dir=p.questions_dir,
    )
    console.print(f"[green]wrote[/green] canonical analysis tables -> {dest}")
    table = Table("panel", "builds", "find runs", "question rows")
    for panel_id, panel in sorted(metadata["panels"].items()):
        table.add_row(
            panel_id,
            str(panel["build_run_count"]),
            str(panel["find_run_count"]),
            str(panel["question_row_count"]),
        )
    console.print(table)


# ---------------------------------------------------------------------------
# helpers
# ---------------------------------------------------------------------------


def _find_spec(task_id: str):
    p = paths()
    for s in load_specs_dir(p.specs_dir):
        if s.task_id == task_id:
            return s
    raise typer.BadParameter(f"unknown task_id {task_id!r}")


def _resolve_task_ids(
    specs: list[BuildTaskSpec],
    *,
    tasks: str | None,
    include_edit: bool,
) -> set[str]:
    """Resolve the task scope used by run/analyze commands.

    The benchmark's default headline scope is from-scratch synthesis.
    Edit tasks are still available by naming them explicitly with
    ``--tasks`` or by opting into the full suite with ``--include-edit``.
    """

    if tasks:
        wanted = {t.strip() for t in tasks.split(",") if t.strip()}
        if not wanted:
            raise typer.BadParameter("no task ids supplied")
        known = {s.task_id for s in specs}
        unknown = sorted(wanted - known)
        if unknown:
            raise typer.BadParameter(f"unknown task_id(s): {', '.join(unknown)}")
        return wanted
    if include_edit:
        return {s.task_id for s in specs}
    return {s.task_id for s in specs if s.task_family == PRIMARY_TASK_FAMILY}


def _resolve_trial_counts(
    *,
    trials: int | None,
    build_trials: int | None,
    find_trials: int | None,
) -> tuple[int, int]:
    if trials is not None and (build_trials is not None or find_trials is not None):
        raise typer.BadParameter("--trials cannot be combined with --build-trials or --find-trials")
    if trials is not None:
        return trials, trials
    return build_trials or 1, find_trials or 1


def _load_scoped_runs(*, task_ids: set[str]) -> tuple[list[BuildRun], list[FindRun]]:
    p = paths()
    build_runs = [br for br in load_build_runs(p.runs_dir) if br.task_id in task_ids]
    build_run_ids = {br.build_run_id for br in build_runs}
    find_runs = [
        fr
        for fr in load_find_runs(p.runs_dir)
        if fr.task_id in task_ids and fr.build_run_id in build_run_ids
    ]
    return build_runs, find_runs


def _print_build_run(br) -> None:
    v = br.validation
    console.print(
        f"[bold]{br.build_run_id}[/bold] status={br.status} "
        f"compile={v.compile_pass} files={v.rust_files} loc={v.rust_loc}"
    )


if __name__ == "__main__":
    app()
