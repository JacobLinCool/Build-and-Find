## Build-and-Find Makefile.
##
## Usage examples:
##   make help              # list targets
##   make prepare-release   # sanitize local runs/ into the release tree runs_release/
##   make release-check     # regenerate the release reports and verify the release tree
##   make paper-analysis    # regenerate final paper analysis reports
##   make paper-figures     # regenerate paper figures from final reports (FIGURES_DIR=...)
##   make label-sensitivity # regenerate the adjudicated-label sensitivity analysis
##   make analysis-tables   # materialize canonical analysis tables
##   make reproducibility-index # write the reproducibility index of the paper results
##   make update-croissant  # refresh Croissant metadata checksums
##   make reviewer-check-fast # fast smoke test with deterministic release checks
##   make reviewer-check    # smoke test with the full test suite, no model calls
##   make eandd-final       # run the frozen E&D final experiment matrix
##   make eandd-continue    # continue final matrix without rerunning successful records
##   make eandd-dry-run     # smoke-test final matrix in an isolated temp root
##   make test              # run pytest
##   make lint              # ruff check
##   make specs             # validate specs and question banks via the CLI
##
## The manuscript build targets (paper, paper-force, paper-all, paper-clean,
## paper-distclean) are defined in paper/paper.mk and are available only in
## checkouts that include the manuscript sources.

PAPER_MK := paper/paper.mk

PY      := uv run python
PAPER_PY := uv run --extra paper python
UV_RUN  := uv run
BNF     := $(UV_RUN) bnf
RUNS_ROOT ?= runs_release
# Output directory of scripts/paper_figures.py. Empty selects the script default:
# the manuscript figures directory when present, else reports/figures under RUNS_ROOT.
FIGURES_DIR ?=
LOCAL_RUNS_ROOT := runs
RELEASE_RUNS_ROOT := runs_release

.DEFAULT_GOAL := help

# ---------------------------------------------------------------------------
# Help
# ---------------------------------------------------------------------------

.PHONY: help
help: ## Show this help message
	@awk 'BEGIN {FS = ":.*##"; printf "\nTargets:\n"} \
	     /^[a-zA-Z_-]+:.*##/ {printf "  \033[36m%-22s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)

# ---------------------------------------------------------------------------
# Paper analysis and figures
# ---------------------------------------------------------------------------

.PHONY: paper-analysis paper-figures
paper-analysis: analysis-tables ## Regenerate final paper analysis reports
	BNF_RUNS_ROOT=$(RUNS_ROOT) $(PY) scripts/analyze_eandd_final.py
	BNF_RUNS_ROOT=$(RUNS_ROOT) $(PY) scripts/analyze_eandd_pivots.py
	BNF_RUNS_ROOT=$(RUNS_ROOT) $(PY) scripts/analyze_eandd_within_finder_efficiency.py
	BNF_RUNS_ROOT=$(RUNS_ROOT) $(PY) scripts/select_eandd_core_finders.py
	BNF_RUNS_ROOT=$(RUNS_ROOT) $(PAPER_PY) scripts/analyze_eandd_low_prior.py
	BNF_RUNS_ROOT=$(RUNS_ROOT) $(PY) scripts/analyze_eandd_validity_stress.py
	BNF_RUNS_ROOT=$(RUNS_ROOT) $(PY) scripts/analyze_eandd_calibration_uncertainty.py
	BNF_RUNS_ROOT=$(RUNS_ROOT) $(PY) scripts/analyze_artifact_conformance_impact.py
	BNF_RUNS_ROOT=$(RUNS_ROOT) $(PY) scripts/analyze_control_exclusions.py
	BNF_RUNS_ROOT=$(RUNS_ROOT) $(PY) scripts/analyze_option_free_check.py
	BNF_RUNS_ROOT=$(RUNS_ROOT) $(PY) scripts/analyze_transcript_exposure.py
	BNF_RUNS_ROOT=$(RUNS_ROOT) $(PY) scripts/summarize_evidence_audit.py
	BNF_RUNS_ROOT=$(RUNS_ROOT) $(PY) scripts/summarize_runtime_tests.py
	BNF_RUNS_ROOT=$(RUNS_ROOT) $(PY) scripts/write_reproducibility_index.py

paper-figures: paper-analysis ## Regenerate all paper figures (FIGURES_DIR overrides the output directory)
	BNF_RUNS_ROOT=$(RUNS_ROOT) $(PAPER_PY) scripts/plot_affinity_residuals.py
	BNF_RUNS_ROOT=$(RUNS_ROOT) $(if $(FIGURES_DIR),BNF_FIGURES_DIR=$(FIGURES_DIR) )$(PAPER_PY) scripts/paper_figures.py

# Manuscript build targets, present only in checkouts with the manuscript sources.
-include $(PAPER_MK)

# ---------------------------------------------------------------------------
# Analysis scripts
# ---------------------------------------------------------------------------

.PHONY: analysis-tables reproducibility-index label-sensitivity
analysis-tables: ## Materialize canonical analysis tables from raw run records
	BNF_RUNS_ROOT=$(RUNS_ROOT) $(PY) scripts/export_analysis_tables.py

reproducibility-index: analysis-tables ## Write reproducibility_index.md under the configured reports root
	BNF_RUNS_ROOT=$(RUNS_ROOT) $(PY) scripts/write_reproducibility_index.py

label-sensitivity: ## Regenerate the adjudicated-label sensitivity analysis (inputs under rebuttal/)
	$(PY) scripts/analyze_rebuttal_label_sensitivity.py \
		--runs-root $(RELEASE_RUNS_ROOT) \
		--full-audit-disagreements rebuttal/full_conformance_audit/disagreements.csv \
		--author-decisions rebuttal/full_conformance_audit/author_decisions.csv \
		--model-assisted-decisions rebuttal/full_conformance_audit/model_assisted_decisions.csv \
		--label-corrections $(RELEASE_RUNS_ROOT)/reports/artifact_conformance_consensus/final/label_corrections_v1_1.csv \
		--output-dir rebuttal/full_conformance_audit/label_sensitivity_adjudicated

# ---------------------------------------------------------------------------
# Release artifact
# ---------------------------------------------------------------------------

.PHONY: prepare-release release-analysis release-figures release-check update-croissant reviewer-check-fast reviewer-check local-paper-analysis
prepare-release: ## Sanitize local runs/ into the release tree runs_release/ (deletes it first)
	$(PY) scripts/prepare_release.py

update-croissant: ## Refresh Croissant metadata checksums and modified date
	$(PY) scripts/update_croissant_metadata.py

release-analysis: ## Regenerate analysis from runs_release/
	$(MAKE) paper-analysis RUNS_ROOT=$(RELEASE_RUNS_ROOT)

release-figures: ## Regenerate paper figures from runs_release/
	$(MAKE) paper-figures RUNS_ROOT=$(RELEASE_RUNS_ROOT)

release-check: ## Regenerate the release reports and verify the release tree
	$(MAKE) release-analysis
	$(MAKE) update-croissant
	BNF_RUNS_ROOT=$(RELEASE_RUNS_ROOT) $(PY) scripts/verify_release.py

reviewer-check-fast: ## Fast smoke test: specs, tables, release metadata, table tests
	$(MAKE) specs-validate
	BNF_RUNS_ROOT=$(RELEASE_RUNS_ROOT) $(PY) scripts/export_analysis_tables.py
	$(MAKE) update-croissant
	BNF_RUNS_ROOT=$(RELEASE_RUNS_ROOT) $(PY) scripts/verify_release.py
	$(UV_RUN) pytest tests/test_analysis_tables.py -q
	@if [ ! -f "$(PAPER_MK)" ]; then \
		if [ "$${REVIEWER_CHECK_LATEX:-0}" = "1" ]; then \
			printf '%s\n' 'REVIEWER_CHECK_LATEX=1 needs the manuscript sources, which this checkout does not include.' >&2; \
			exit 1; \
		fi; \
		printf '%s\n' 'Skipping LaTeX compile; the manuscript sources are not part of this checkout.'; \
	elif [ "$${REVIEWER_CHECK_LATEX:-0}" = "1" ]; then \
		$(MAKE) paper-force; \
	else \
		printf '%s\n' 'Skipping LaTeX compile; set REVIEWER_CHECK_LATEX=1 to include it.'; \
	fi

reviewer-check: ## Smoke test: specs, full test suite, tables, release verification
	$(MAKE) specs-validate
	$(MAKE) test
	BNF_RUNS_ROOT=$(RELEASE_RUNS_ROOT) $(PY) scripts/export_analysis_tables.py
	$(MAKE) update-croissant
	BNF_RUNS_ROOT=$(RELEASE_RUNS_ROOT) $(PY) scripts/verify_release.py

local-paper-analysis: ## Regenerate analysis from local unsanitized runs/
	$(MAKE) paper-analysis RUNS_ROOT=$(LOCAL_RUNS_ROOT)

# ---------------------------------------------------------------------------
# Test / lint
# ---------------------------------------------------------------------------

.PHONY: test test-cov lint format check
test: ## Run the pytest suite
	$(UV_RUN) pytest

test-cov: ## Run pytest with coverage
	$(UV_RUN) pytest --cov=bnf --cov-report=term-missing

lint: ## Lint with ruff
	$(UV_RUN) ruff check src tests scripts

format: ## Auto-format with ruff
	$(UV_RUN) ruff format src tests scripts

check: lint test ## Lint and run tests

# ---------------------------------------------------------------------------
# Benchmark CLI shortcuts
# ---------------------------------------------------------------------------

.PHONY: agents specs questions specs-validate
agents: ## List registered agent harnesses
	$(BNF) agents list

specs: ## List build specs
	$(BNF) specs list

questions: ## List question banks
	$(BNF) questions list

specs-validate: ## Validate specs against their question banks
	$(BNF) specs validate

# ---------------------------------------------------------------------------
# Live experiments (cost API spend; opt-in via explicit phase target)
# ---------------------------------------------------------------------------

.PHONY: eandd-final eandd-continue eandd-dry-run
eandd-final: ## Run the frozen E&D final scratch-only experiment matrix
	bash scripts/run_eandd_final_2026.sh

eandd-continue: ## Continue final matrix without rerunning successful records
	$(PY) scripts/continue_eandd_final_2026.py

eandd-dry-run: ## Run the final matrix dry-run in an isolated temp root
	bash scripts/dry_run_eandd_final_2026.sh

# ---------------------------------------------------------------------------
# Maintenance
# ---------------------------------------------------------------------------

.PHONY: clean
clean: ## Remove generated build artifacts
	find . -name "__pycache__" -type d -prune -exec rm -rf {} +
	find . -name "*.pyc" -delete
