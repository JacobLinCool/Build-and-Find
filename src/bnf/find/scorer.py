"""Exact-match scorer for Find answers."""

from __future__ import annotations

from bnf.schema.run import FinderAnswer


def score_answers(answers: list[FinderAnswer]) -> dict[str, float]:
    if not answers:
        return {"accuracy": 0.0, "attempted": 0.0, "answered": 0, "correct": 0}
    answered = [a for a in answers if a.selected_option is not None]
    correct = sum(1 for a in answered if a.is_correct)
    return {
        "accuracy": correct / len(answers),
        "attempted_rate": len(answered) / len(answers),
        "answered": len(answered),
        "correct": correct,
        "total": len(answers),
    }
