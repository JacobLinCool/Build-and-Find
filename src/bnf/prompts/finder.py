"""Finder prompt renderer.

Design notes
------------
* The finder is given a codebase plus multiple-choice questions. We do
  NOT tell the finder that the questions were derived from a hidden
  specification — the finder simply picks the option that best fits the
  codebase it observes. The benchmark's ground truth is server-side; the
  finder is evaluated against it externally.
* Answers are written to a file inside the workspace rather than
  extracted from stdout. This avoids fragile parsing of agent CLI output
  and gives the agent an unambiguous persistence target.
* The presented option keys stay as ``A..D`` but their textual contents
  are optionally shuffled per-seed (see ``FindQuestion.shuffled``). The
  mapping is stored in the ``FindRun`` record for auditing.
"""

from __future__ import annotations

import hashlib
import json
import textwrap
from pathlib import Path
from typing import Any

from bnf.schema import FindQuestion, QuestionBank

ANSWERS_FILENAME = "find_answers.json"


def _preamble(answers_path: str) -> str:
    return textwrap.dedent(
        f"""\
        You are examining a Rust codebase that has been placed in your
        current working directory. Read it — the file tree, `Cargo.toml`,
        any README, the source files, and the tests — and then answer the
        multiple-choice questions below.

        How to respond
        --------------
        - For each question, choose EXACTLY ONE of A, B, C, D — the option
          that best fits what the codebase actually is and does.
        - Do not include commentary in stdout. Your response is read from
          a file you will create.

        Output file
        -----------
        Write your answers to `{answers_path}` at the workspace root as a
        single JSON object with this shape:

            {{
              "answers": [
                {{
                  "question_id": "q_xxx",
                  "selected_option": "A",
                  "evidence_files": ["src/foo.rs"],
                  "evidence_symbols": ["Foo::bar"],
                  "rationale": "one short sentence"
                }}
              ]
            }}

        `selected_option` is required and must be one of "A", "B", "C",
        "D". `evidence_files`, `evidence_symbols`, and `rationale` are
        optional but encouraged. Every question must appear in the array.
        """
    ).strip()


def _format_question(q: FindQuestion, options: dict[str, str]) -> str:
    lines = [f"Question {q.question_id}", q.prompt.strip(), ""]
    for key in ("A", "B", "C", "D"):
        lines.append(f"  {key}. {options[key]}")
    return "\n".join(lines)


def render_finder_prompt(
    bank: QuestionBank,
    *,
    shuffle_seed: int | None = None,
) -> tuple[str, dict[str, tuple[dict[str, str], str]]]:
    """Render the finder prompt.

    Returns ``(prompt, presentation_map)`` where ``presentation_map`` maps
    each ``question_id`` to ``(presented_options, presented_correct_key)``.
    """

    presentation: dict[str, tuple[dict[str, str], str]] = {}
    parts = [_preamble(ANSWERS_FILENAME), "", f"Total questions: {len(bank.questions)}", ""]
    for q in bank.questions:
        if shuffle_seed is not None:
            opts, correct = q.shuffled(shuffle_seed)
        else:
            opts, correct = dict(q.options), q.correct_option
        presentation[q.question_id] = (opts, correct)
        parts.append(_format_question(q, opts))
        parts.append("")
    return "\n".join(parts).rstrip() + "\n", presentation


def finder_prompt_hash(prompt: str) -> str:
    return hashlib.sha256(prompt.encode("utf-8")).hexdigest()[:16]


def read_finder_answers(workspace: Path) -> dict[str, dict[str, Any]]:
    """Read ``find_answers.json`` from the finder's workspace.

    Returns ``{question_id: {"selected_option": ..., "evidence_files":
    ..., "evidence_symbols": ..., "rationale": ...}}``. Missing file
    or malformed JSON yields an empty mapping so the runner can record
    "unanswered" rows rather than crash.
    """

    path = workspace / ANSWERS_FILENAME
    if not path.exists():
        return {}
    try:
        obj = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return {}
    answers = obj.get("answers") if isinstance(obj, dict) else None
    if not isinstance(answers, list):
        return {}
    out: dict[str, dict[str, Any]] = {}
    for a in answers:
        if not isinstance(a, dict):
            continue
        qid = a.get("question_id")
        sel = a.get("selected_option")
        if not isinstance(qid, str) or sel not in ("A", "B", "C", "D"):
            continue
        out[qid] = {
            "selected_option": sel,
            "evidence_files": a.get("evidence_files") or [],
            "evidence_symbols": a.get("evidence_symbols") or [],
            "rationale": a.get("rationale"),
        }
    return out
