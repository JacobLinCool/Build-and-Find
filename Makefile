## Build-and-Find Makefile.
##
## Usage examples:
##   make help              # list targets
##   make prepare-release   # sanitize local runs/ into reviewer-facing runs_release/
##   make release-check     # verify reviewer-facing release data and regenerate reports
##   make paper-all         # regenerate release analysis, figures, and PDF
##   make paper             # build the NeurIPS PDF from current artifacts
##   make paper-analysis    # regenerate final paper analysis reports
##   make paper-figures     # regenerate paper figures from final reports
##   make analysis-tables   # materialize canonical analysis tables
##   make claim-evidence    # write the paper claim-evidence index
##   make update-croissant  # refresh Croissant metadata checksums
##   make reviewer-check-fast # reviewer smoke test with deterministic release checks
##   make reviewer-check    # fast reviewer smoke test without paid model calls
##   make eandd-final       # run the frozen E&D final experiment matrix
##   make eandd-continue    # continue final matrix without rerunning successful records
##   make eandd-dry-run     # smoke-test final matrix in an isolated temp root
##   make test              # run pytest
##   make lint              # ruff check
##   make specs             # validate specs and question banks via the CLI

PAPER_DIR := paper/neurips2026_eandd
PAPER_PDF := $(PAPER_DIR)/main.pdf
PAPER_TEX := $(PAPER_DIR)/main.tex
PAPER_ABS_DIR := $(CURDIR)/$(PAPER_DIR)
PAPER_BUILD_DIR ?= /tmp/build-and-find-neurips2026-eandd
PAPER_LOCAL_LATEX_ARTIFACTS := main.aux main.bbl main.blg main.fdb_latexmk main.fls main.log main.out main.toc main.lof main.lot main.synctex.gz

PY      := uv run python
PAPER_PY := uv run --extra paper python
UV_RUN  := uv run
BNF     := $(UV_RUN) bnf
PDFLATEX := pdflatex -interaction=nonstopmode -halt-on-error -recorder -output-directory=$(PAPER_BUILD_DIR)
BIBTEX ?= bibtex
RUNS_ROOT ?= runs_release
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
# Paper
# ---------------------------------------------------------------------------

.PHONY: paper paper-force paper-all paper-analysis paper-figures paper-clean paper-distclean
paper: $(PAPER_PDF) ## Build the NeurIPS PDF from current analysis and figures

$(PAPER_PDF): $(PAPER_TEX) $(PAPER_DIR)/checklist.tex $(PAPER_DIR)/references.bib
	rm -rf $(PAPER_BUILD_DIR)
	mkdir -p $(PAPER_BUILD_DIR)
	cd $(PAPER_DIR) && $(PDFLATEX) main.tex
	cd $(PAPER_BUILD_DIR) && BIBINPUTS="$(PAPER_ABS_DIR):" BSTINPUTS="$(PAPER_ABS_DIR):" $(BIBTEX) main
	cd $(PAPER_DIR) && $(PDFLATEX) main.tex
	cd $(PAPER_DIR) && $(PDFLATEX) main.tex
	cd $(PAPER_DIR) && $(PDFLATEX) main.tex
	cp $(PAPER_BUILD_DIR)/main.pdf $(PAPER_PDF)
	cd $(PAPER_DIR) && rm -f $(PAPER_LOCAL_LATEX_ARTIFACTS)

paper-force: ## Force rebuild the NeurIPS PDF from current analysis and figures
	rm -rf $(PAPER_BUILD_DIR)
	mkdir -p $(PAPER_BUILD_DIR)
	cd $(PAPER_DIR) && rm -f $(PAPER_LOCAL_LATEX_ARTIFACTS)
	cd $(PAPER_DIR) && $(PDFLATEX) main.tex
	cd $(PAPER_BUILD_DIR) && BIBINPUTS="$(PAPER_ABS_DIR):" BSTINPUTS="$(PAPER_ABS_DIR):" $(BIBTEX) main
	cd $(PAPER_DIR) && $(PDFLATEX) main.tex
	cd $(PAPER_DIR) && $(PDFLATEX) main.tex
	cd $(PAPER_DIR) && $(PDFLATEX) main.tex
	cp $(PAPER_BUILD_DIR)/main.pdf $(PAPER_PDF)
	cd $(PAPER_DIR) && rm -f $(PAPER_LOCAL_LATEX_ARTIFACTS)

paper-all: ## Regenerate final analysis, figures, and PDF
	$(MAKE) paper-figures
	$(MAKE) paper-force

paper-analysis: analysis-tables ## Regenerate final paper analysis reports
	BNF_RUNS_ROOT=$(RUNS_ROOT) $(PY) scripts/analyze_eandd_final.py
	BNF_RUNS_ROOT=$(RUNS_ROOT) $(PY) scripts/analyze_eandd_pivots.py
	BNF_RUNS_ROOT=$(RUNS_ROOT) $(PY) scripts/analyze_eandd_within_finder_efficiency.py
	BNF_RUNS_ROOT=$(RUNS_ROOT) $(PY) scripts/select_eandd_core_finders.py
	BNF_RUNS_ROOT=$(RUNS_ROOT) $(PAPER_PY) scripts/analyze_eandd_low_prior.py
	BNF_RUNS_ROOT=$(RUNS_ROOT) $(PY) scripts/analyze_eandd_validity_stress.py
	BNF_RUNS_ROOT=$(RUNS_ROOT) $(PY) scripts/analyze_eandd_calibration_uncertainty.py
	BNF_RUNS_ROOT=$(RUNS_ROOT) $(PY) scripts/analyze_artifact_conformance_impact.py
	BNF_RUNS_ROOT=$(RUNS_ROOT) $(PY) scripts/summarize_evidence_audit.py
	BNF_RUNS_ROOT=$(RUNS_ROOT) $(PY) scripts/write_claim_evidence.py

paper-figures: paper-analysis ## Regenerate all paper figures
	BNF_RUNS_ROOT=$(RUNS_ROOT) $(PAPER_PY) scripts/plot_affinity_residuals.py
	BNF_RUNS_ROOT=$(RUNS_ROOT) $(PAPER_PY) scripts/paper_figures.py

paper-clean: ## Remove latex build artifacts (keeps the PDF)
	rm -rf $(PAPER_BUILD_DIR)
	cd $(PAPER_DIR) && rm -f $(PAPER_LOCAL_LATEX_ARTIFACTS)

paper-distclean: ## Remove all latex artifacts including the PDF
	rm -rf $(PAPER_BUILD_DIR)
	cd $(PAPER_DIR) && rm -f $(PAPER_LOCAL_LATEX_ARTIFACTS) main.pdf

# ---------------------------------------------------------------------------
# Analysis scripts
# ---------------------------------------------------------------------------

.PHONY: analysis-tables claim-evidence
analysis-tables: ## Materialize canonical analysis tables from raw run records
	BNF_RUNS_ROOT=$(RUNS_ROOT) $(PY) scripts/export_analysis_tables.py

claim-evidence: analysis-tables ## Write claim_evidence.md under the configured reports root
	BNF_RUNS_ROOT=$(RUNS_ROOT) $(PY) scripts/write_claim_evidence.py

# ---------------------------------------------------------------------------
# Release artifact
# ---------------------------------------------------------------------------

.PHONY: prepare-release release-analysis release-figures release-check update-croissant reviewer-check-fast reviewer-check local-paper-analysis
prepare-release: ## Sanitize local runs/ into reviewer-facing runs_release/
	$(PY) scripts/prepare_release.py

update-croissant: ## Refresh Croissant metadata checksums and modified date
	$(PY) scripts/update_croissant_metadata.py

release-analysis: ## Regenerate analysis from runs_release/
	$(MAKE) paper-analysis RUNS_ROOT=$(RELEASE_RUNS_ROOT)

release-figures: ## Regenerate paper figures from runs_release/
	$(MAKE) paper-figures RUNS_ROOT=$(RELEASE_RUNS_ROOT)

release-check: ## Verify and regenerate reviewer-facing release artifacts
	$(MAKE) release-analysis
	$(MAKE) update-croissant
	BNF_RUNS_ROOT=$(RELEASE_RUNS_ROOT) $(PY) scripts/verify_release.py

reviewer-check-fast: ## Fast reviewer smoke test: schema, tables, release metadata, small tests
	$(MAKE) specs-validate
	BNF_RUNS_ROOT=$(RELEASE_RUNS_ROOT) $(PY) scripts/export_analysis_tables.py
	$(MAKE) update-croissant
	BNF_RUNS_ROOT=$(RELEASE_RUNS_ROOT) $(PY) scripts/verify_release.py
	$(UV_RUN) pytest tests/test_analysis_tables.py -q
	@if [ "$${REVIEWER_CHECK_LATEX:-0}" = "1" ]; then \
		$(MAKE) paper-force; \
	else \
		printf '%s\n' 'Skipping LaTeX compile; set REVIEWER_CHECK_LATEX=1 to include it.'; \
	fi

reviewer-check: ## Fast reviewer smoke test: specs, tests, key table materialization
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
clean: paper-clean ## Remove generated build artifacts
	find . -name "__pycache__" -type d -prune -exec rm -rf {} +
	find . -name "*.pyc" -delete
