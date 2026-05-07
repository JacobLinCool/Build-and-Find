"""Builder prompt renderer.

Design notes
------------
The prompt has three fixed layers (see section 20 of the design doc):

1. Global benchmark instructions
2. Task-specific content (objective, FRs, SRs, ERs, constraints, deliverables)
3. Output contract

We deliberately do **not** include the ``intentions:`` block in the builder
prompt. Leaking canonical intention strings would make the Find phase trivial
(a builder could embed them verbatim in code comments).
"""

from __future__ import annotations

import hashlib
import textwrap

from bnf.schema import BuildTaskSpec

GLOBAL_PREAMBLE = textwrap.dedent(
    """\
    You are participating in the Build-and-Find coding benchmark. You will
    receive a structured specification and must produce a complete Rust
    project that satisfies it.

    Ground rules
    ------------
    - Terminal commands and network access are allowed under the benchmark
      policy stated in the constraints section. You SHOULD run `cargo build`
      and `cargo test` before finishing to validate your work.
    - Prioritise correctness, maintainability, and clear project structure.
      Downstream evaluators will inspect the codebase to recover the task
      intent, so legibility matters.
    - Do not hard-code benchmark identifiers (e.g., requirement IDs like FR1,
      intention IDs like I-FR1, or the literal task_id) in source files or
      comments. Implementation artefacts must stand on their own.
    - Do not leave required functionality as TODO/unimplemented.
    """
).strip()

OUTPUT_CONTRACT = textwrap.dedent(
    """\
    Output contract
    ---------------
    - Produce a complete `cargo` project at the workspace root.
    - The project must build with `cargo build --release` on stable Rust.
    - Tests (where applicable per the spec) must pass with `cargo test`.
    - Include a `README.md` with build/usage instructions.
    - Format the code with `cargo fmt` before finishing.
    """
).strip()


def _render_functional(spec: BuildTaskSpec) -> str:
    if not spec.functional_requirements:
        return ""
    lines = ["Functional requirements"]
    lines.append("-" * len("Functional requirements"))
    for fr in spec.functional_requirements:
        lines.append(f"- [{fr.id}] {fr.statement}")
    return "\n".join(lines)


def _render_structural(spec: BuildTaskSpec) -> str:
    if not spec.structural_requirements:
        return ""
    lines = ["Structural requirements"]
    lines.append("-" * len("Structural requirements"))
    for sr in spec.structural_requirements:
        lines.append(f"- [{sr.id}] {sr.statement}")
    return "\n".join(lines)


def _render_edits(spec: BuildTaskSpec) -> str:
    if not spec.edit_requirements:
        return ""
    lines = ["Edit requirements"]
    lines.append("-" * len("Edit requirements"))
    groups: dict[str, list[str]] = {"add": [], "remove": [], "preserve": []}
    for er in spec.edit_requirements:
        groups[er.kind].append(f"- [{er.id}] {er.statement}")
    for kind, label in (("add", "Add"), ("remove", "Remove"), ("preserve", "Preserve")):
        if groups[kind]:
            lines.append(f"\n{label}:")
            lines.extend(groups[kind])
    return "\n".join(lines)


def _render_constraints(spec: BuildTaskSpec) -> str:
    c = spec.constraints
    lines = ["Constraints"]
    lines.append("-" * len("Constraints"))
    lines.extend(
        [
            f"- Language: {c.language}",
            f"- Rust toolchain: {c.rust_toolchain}",
            f"- Target OS: {c.target_os}",
            f"- Terminal: {'allowed' if c.terminal_allowed else 'disallowed'}",
            f"- Network: {'allowed' if c.network_allowed else 'disallowed'}",
            f"- External crates: {'allowed' if c.external_crates_allowed else 'disallowed'}",
            f"- Frontend/UI work: {'allowed' if c.frontend_allowed else 'disallowed'}",
            f"- External hosted services: "
            f"{'allowed' if c.external_hosted_services_allowed else 'disallowed'}",
        ]
    )
    if c.build_time_budget_s is not None:
        lines.append(f"- Wall-clock budget: {c.build_time_budget_s}s")
    return "\n".join(lines)


def _render_deliverables(spec: BuildTaskSpec) -> str:
    if not spec.deliverables:
        return ""
    lines = ["Deliverables"]
    lines.append("-" * len("Deliverables"))
    for d in spec.deliverables:
        lines.append(f"- {d}")
    return "\n".join(lines)


def render_builder_prompt(spec: BuildTaskSpec) -> str:
    """Deterministically render the builder prompt for ``spec``."""

    header = f"Task: {spec.title}\nTask family: {spec.task_family}\n"
    objective = f"Objective\n---------\n{spec.objective.strip()}"
    sections = [
        GLOBAL_PREAMBLE,
        "",
        header,
        objective,
    ]
    for block in (
        _render_functional(spec),
        _render_structural(spec),
        _render_edits(spec),
        _render_constraints(spec),
        _render_deliverables(spec),
    ):
        if block:
            sections.extend(["", block])

    if spec.task_family == "edit_existing_project" and spec.seed_project is not None:
        sections.extend(
            [
                "",
                "Seed project",
                "------------",
                "A seed project has been placed in the workspace root. Modify it "
                "in place to satisfy the edit requirements above; do not create a "
                "parallel project.",
            ]
        )

    sections.extend(["", OUTPUT_CONTRACT])
    return "\n".join(sections).strip() + "\n"


def builder_prompt_hash(prompt: str) -> str:
    return hashlib.sha256(prompt.encode("utf-8")).hexdigest()[:16]
