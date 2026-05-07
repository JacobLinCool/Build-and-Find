"""Build task specification schema (Phase 1).

A ``BuildTaskSpec`` is the single source of truth for a benchmark task. It
carries functional, structural and edit requirements, together with a canonical
``intentions`` list that every Phase 2 question must trace back to.

Design notes
------------
* ``spec_version`` is mandatory so that future schema evolutions are detectable.
* Intentions are kept **separate** from raw requirements. Requirements describe
  *what the builder is told*; intentions describe *what a downstream finder
  should be able to recover*. These are not always one-to-one.
* ``intention_id`` must be globally unique across a spec; we validate this.
* ``constraints`` are intentionally strict so runs are comparable across agents.
"""

from __future__ import annotations

import hashlib
from pathlib import Path
from typing import Any, Literal

import yaml
from pydantic import BaseModel, Field, field_validator, model_validator

TaskFamily = Literal["build_from_scratch", "edit_existing_project"]
EditKind = Literal["add", "remove", "preserve"]


class FunctionalRequirement(BaseModel):
    id: str
    statement: str

    @field_validator("id")
    @classmethod
    def _fr_id(cls, v: str) -> str:
        if not v.startswith("FR"):
            raise ValueError(f"functional requirement id must start with 'FR', got {v!r}")
        return v


class StructuralRequirement(BaseModel):
    id: str
    statement: str

    @field_validator("id")
    @classmethod
    def _sr_id(cls, v: str) -> str:
        if not v.startswith("SR"):
            raise ValueError(f"structural requirement id must start with 'SR', got {v!r}")
        return v


class EditRequirement(BaseModel):
    id: str
    kind: EditKind
    statement: str

    @field_validator("id")
    @classmethod
    def _er_id(cls, v: str) -> str:
        if not v.startswith("ER"):
            raise ValueError(f"edit requirement id must start with 'ER', got {v!r}")
        return v


class Intention(BaseModel):
    id: str
    source: str  # source requirement id (FR*/SR*/ER*)
    kind: Literal["functional", "structural", "edit"]
    statement: str

    @field_validator("id")
    @classmethod
    def _i_id(cls, v: str) -> str:
        if not v.startswith("I-"):
            raise ValueError(f"intention id must start with 'I-', got {v!r}")
        return v


class IntentionSet(BaseModel):
    functional: list[Intention] = Field(default_factory=list)
    structural: list[Intention] = Field(default_factory=list)
    edit: list[Intention] = Field(default_factory=list)

    def all(self) -> list[Intention]:
        return [*self.functional, *self.structural, *self.edit]

    def by_id(self, intention_id: str) -> Intention | None:
        for it in self.all():
            if it.id == intention_id:
                return it
        return None


class Constraints(BaseModel):
    language: str = "Rust"
    frontend_allowed: bool = False
    terminal_allowed: bool = True
    network_allowed: bool = True
    external_crates_allowed: bool = True
    target_os: str = "linux"
    rust_toolchain: str = "stable"
    external_hosted_services_allowed: bool = False
    # Wall-clock budget (seconds) for the builder. None = no cap.
    build_time_budget_s: int | None = 1800


class Validation(BaseModel):
    """What to record about the produced codebase at the Build-phase gate.

    We intentionally do not validate correctness — the benchmark
    measures legibility indirectly via the Find phase. The only useful
    Build-phase signal is "did the agent leave us with something that
    at least compiles". Anything beyond that is recorded for audit,
    not for gating.
    """

    # Optional free-form notes/checks the spec author wants captured in
    # the run record; the runner does not interpret them.
    notes: list[str] = Field(default_factory=list)


class SeedProject(BaseModel):
    """Metadata pointer to a seed project (edit_existing_project only)."""

    path: str  # relative to the specs directory
    description: str | None = None


class BuildTaskSpec(BaseModel):
    spec_version: str
    task_id: str
    task_family: TaskFamily
    title: str
    objective: str
    functional_requirements: list[FunctionalRequirement] = Field(default_factory=list)
    structural_requirements: list[StructuralRequirement] = Field(default_factory=list)
    edit_requirements: list[EditRequirement] = Field(default_factory=list)
    constraints: Constraints = Field(default_factory=Constraints)
    deliverables: list[str] = Field(default_factory=list)
    validation: Validation = Field(default_factory=Validation)
    intentions: IntentionSet = Field(default_factory=IntentionSet)
    seed_project: SeedProject | None = None
    # Authoring metadata, not consumed by the builder prompt.
    tags: list[str] = Field(default_factory=list)
    notes: str | None = None

    # --- invariants --------------------------------------------------------

    @model_validator(mode="after")
    def _check_families(self) -> "BuildTaskSpec":
        if self.task_family == "edit_existing_project":
            if not self.edit_requirements:
                raise ValueError("edit_existing_project tasks must define edit_requirements")
            if self.seed_project is None:
                raise ValueError("edit_existing_project tasks must include seed_project")
        if self.task_family == "build_from_scratch" and self.edit_requirements:
            raise ValueError("build_from_scratch tasks must not have edit_requirements")
        # Intention id uniqueness
        seen: set[str] = set()
        for it in self.intentions.all():
            if it.id in seen:
                raise ValueError(f"duplicate intention id: {it.id}")
            seen.add(it.id)
        # Intention sources must reference declared requirements.
        fr_ids = {r.id for r in self.functional_requirements}
        sr_ids = {r.id for r in self.structural_requirements}
        er_ids = {r.id for r in self.edit_requirements}
        for it in self.intentions.functional:
            if it.source not in fr_ids:
                raise ValueError(f"intention {it.id} references missing FR {it.source}")
        for it in self.intentions.structural:
            if it.source not in sr_ids:
                raise ValueError(f"intention {it.id} references missing SR {it.source}")
        for it in self.intentions.edit:
            if it.source not in er_ids:
                raise ValueError(f"intention {it.id} references missing ER {it.source}")
        return self

    # --- helpers -----------------------------------------------------------

    def spec_hash(self) -> str:
        """Deterministic hash of the spec for reproducibility manifests."""
        payload = self.model_dump_json(exclude={"notes", "tags"}).encode("utf-8")
        return hashlib.sha256(payload).hexdigest()[:16]


def load_spec(path: str | Path) -> BuildTaskSpec:
    p = Path(path)
    with p.open("r", encoding="utf-8") as f:
        data: dict[str, Any] = yaml.safe_load(f)
    return BuildTaskSpec.model_validate(data)


def load_specs_dir(path: str | Path) -> list[BuildTaskSpec]:
    p = Path(path)
    specs: list[BuildTaskSpec] = []
    for f in sorted(p.glob("*.yaml")):
        specs.append(load_spec(f))
    return specs
