from __future__ import annotations

import importlib.util
import json
import sys
from collections import defaultdict
from pathlib import Path

from bnf.prompts import finder_prompt_hash
from bnf.schema import load_specs_dir


ROOT = Path(__file__).resolve().parents[1]
RELEASE_OUTPUT_DIR = ROOT / "runs_release" / "reports" / "option_free_check"


def load_script_module(name: str, relative_path: str):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative_path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


pilot = load_script_module("run_open_pilot", "scripts/run_open_pilot.py")


def released_records() -> list[dict]:
    paths = sorted((RELEASE_OUTPUT_DIR / "records").glob("openfind-*.json"))
    assert len(paths) == 24
    return [json.loads(path.read_text(encoding="utf-8")) for path in paths]


def test_rendered_prompts_match_released_record_hashes() -> None:
    specs = {spec.task_id: spec for spec in load_specs_dir(ROOT / "benchmarks" / "specs")}
    recorded: dict[tuple[str, str], set[str]] = defaultdict(set)
    spec_hashes: dict[str, set[str]] = defaultdict(set)
    for record in released_records():
        recorded[(record["task_id"], record["condition"])].add(record["manifest"]["prompt_hash"])
        spec_hashes[record["task_id"]].add(record["manifest"]["spec_hash"])

    rendered = {}
    for task_id in pilot.OPEN_QUESTION_IDS:
        spec = specs[task_id]
        questions = pilot._selected_questions(ROOT / "benchmarks" / "questions", task_id)
        assert spec_hashes[task_id] == {spec.spec_hash()}
        for condition in pilot.OPEN_CONDITIONS:
            prompt = pilot._render_open_prompt(condition, spec, questions)
            rendered[(task_id, condition)] = finder_prompt_hash(prompt)

    # Each task and condition has one prompt; its six sessions share the hash.
    assert {key: {value} for key, value in rendered.items()} == dict(recorded)
    assert sorted(rendered.values()) == sorted(
        ["908b85bc75890dcd", "8eb01ca8f59d16ba", "c763acf83d20dff9", "a37c0c33cd9d29ba"]
    )


def test_runner_design_matches_released_summary() -> None:
    summary = json.loads((RELEASE_OUTPUT_DIR / "summary.json").read_text(encoding="utf-8"))
    design = summary["design"]
    assert {task: list(ids) for task, ids in pilot.OPEN_QUESTION_IDS.items()} == design["questions"]
    assert pilot.DEFAULT_ARTIFACT_MAP == design["artifacts"]
    assert tuple(design["conditions"]) == pilot.OPEN_CONDITIONS
    assert design["finders"] == pilot.DEFAULT_FINDERS.split(",")
    assert design["trials"] == pilot.DEFAULT_TRIALS
