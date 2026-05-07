"""Export canonical analysis tables from raw Build-and-Find run records."""

from __future__ import annotations

from bnf.analysis.tables import export_analysis_tables
from bnf.config import paths


def main() -> None:
    p = paths()
    metadata = export_analysis_tables(
        records_dir=p.runs_dir,
        out_dir=p.tables_dir,
        questions_dir=p.questions_dir,
    )
    print(f"wrote canonical analysis tables -> {p.tables_dir}")
    for panel_id, panel in sorted(metadata["panels"].items()):
        print(
            f"  {panel_id}: "
            f"{panel['build_run_count']} builds, "
            f"{panel['find_run_count']} find runs, "
            f"{panel['question_row_count']} question rows"
        )


if __name__ == "__main__":
    main()
