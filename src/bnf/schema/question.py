"""Find question schema (Phase 2).

Design notes
------------
* Every question carries a ``source_trace`` back to a spec intention id. This
  is the contract that makes ground truth defensible.
* Options are a dict keyed ``A``, ``B``, ``C``, ``D``; ``correct_option`` is a
  single key. A ``shuffled_options`` helper produces a deterministic
  reordering given a seed (used to remove position bias at presentation time).
* A ``QuestionBank`` is keyed by ``task_id``. We validate uniqueness of
  ``question_id`` and cross-reference against the associated ``BuildTaskSpec``.
"""

from __future__ import annotations

import hashlib
import random
from pathlib import Path
from typing import Any, Literal

import yaml
from pydantic import BaseModel, model_validator

from bnf.schema.spec import BuildTaskSpec

QuestionType = Literal[
    "capability_recognition",
    "design_intention_recognition",
    "edit_add_recognition",
    "edit_remove_recognition",
    "edit_preserve_recognition",
    "negative_contrast",
    # Hard-MCQ families introduced for breaking accuracy saturation.
    # All three are still spec-derivable (a spec-only oracle answers
    # them at 100%); they differ from the recognition types above by
    # demanding that the finder reason about scenarios, absences, or
    # cross-section interactions rather than label a visible design.
    "failure_mode_recognition",
    "forbidden_coupling",
    "multi_spec_interaction",
]

_OPTION_KEYS = ("A", "B", "C", "D")


class SourceTrace(BaseModel):
    spec_section: Literal["functional_requirements", "structural_requirements", "edit_requirements"]
    source_requirement_id: str
    source_intention_id: str


class FindQuestion(BaseModel):
    question_id: str
    codebase_task_id: str
    intention_id: str
    question_type: QuestionType
    prompt: str
    options: dict[str, str]  # keys A..D
    correct_option: Literal["A", "B", "C", "D"]
    source_trace: SourceTrace
    distractor_strategy: str | None = None
    notes: str | None = None

    @model_validator(mode="after")
    def _check_options(self) -> "FindQuestion":
        if set(self.options.keys()) != set(_OPTION_KEYS):
            raise ValueError(
                f"options must be keyed exactly {_OPTION_KEYS}; got {list(self.options)}"
            )
        if self.correct_option not in _OPTION_KEYS:
            raise ValueError(f"correct_option must be one of {_OPTION_KEYS}")
        # Distinct option texts (prevents accidental duplicates that would
        # create multiple correct answers).
        texts = [t.strip().lower() for t in self.options.values()]
        if len(set(texts)) != len(texts):
            raise ValueError(f"options must be textually distinct in {self.question_id}")
        # Intention id must match source_trace.source_intention_id.
        if self.intention_id != self.source_trace.source_intention_id:
            raise ValueError("intention_id and source_trace.source_intention_id must match")
        return self

    # --- helpers -----------------------------------------------------------

    def question_hash(self) -> str:
        payload = self.model_dump_json().encode("utf-8")
        return hashlib.sha256(payload).hexdigest()[:16]

    def shuffled(self, seed: int) -> tuple[dict[str, str], str]:
        """Return ``(reordered_options, new_correct_key)`` deterministically.

        We shuffle the *content* while keeping keys A..D stable for the
        finder. This removes position bias without changing the question's
        visual structure.
        """

        rng = random.Random(f"{seed}|{self.question_hash()}")
        texts = [self.options[k] for k in _OPTION_KEYS]
        rng.shuffle(texts)
        new_options = dict(zip(_OPTION_KEYS, texts))
        correct_text = self.options[self.correct_option]
        new_correct = next(k for k, v in new_options.items() if v == correct_text)
        return new_options, new_correct


class QuestionBank(BaseModel):
    task_id: str
    questions: list[FindQuestion]

    @model_validator(mode="after")
    def _unique_ids(self) -> "QuestionBank":
        seen: set[str] = set()
        for q in self.questions:
            if q.question_id in seen:
                raise ValueError(f"duplicate question_id {q.question_id}")
            seen.add(q.question_id)
            if q.codebase_task_id != self.task_id:
                raise ValueError(
                    f"question {q.question_id} has codebase_task_id {q.codebase_task_id} "
                    f"but bank task_id is {self.task_id}"
                )
        return self

    def cross_validate(self, spec: BuildTaskSpec) -> list[str]:
        """Return a list of error messages; empty iff bank matches the spec."""

        errors: list[str] = []
        declared = {it.id for it in spec.intentions.all()}
        for q in self.questions:
            if q.intention_id not in declared:
                errors.append(
                    f"{q.question_id}: intention {q.intention_id} not declared in spec {spec.task_id}"
                )
            it = spec.intentions.by_id(q.intention_id)
            if it is not None and it.source != q.source_trace.source_requirement_id:
                errors.append(
                    f"{q.question_id}: source_requirement_id {q.source_trace.source_requirement_id} "
                    f"does not match intention source {it.source}"
                )
        return errors


def load_question_bank(path: str | Path) -> QuestionBank:
    p = Path(path)
    with p.open("r", encoding="utf-8") as f:
        data: dict[str, Any] = yaml.safe_load(f)
    return QuestionBank.model_validate(data)
