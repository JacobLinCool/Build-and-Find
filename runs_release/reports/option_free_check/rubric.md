# Open-pilot grading rubric (pre-registered)

Status: **frozen 2026-07-24, before any pilot session was launched.**
This rubric is written from `benchmarks/specs/*.yaml` and
`benchmarks/questions/*.yaml` only; no pilot output existed when it was
fixed. Any later edit must be recorded as a dated amendment at the
bottom of this file, and claims graded before the amendment must not be
re-graded under it.

Scope: the 8 open-pilot questions (the 10-question low-prior subset
minus its two `negative_contrast` items `q_db_07` / `q_nw_11`, which
presuppose visible options), each answered as a free-form claim under
two conditions (`artifact_open`, `question_only_open`) by 2 finders x 3
trials x 2 tasks = 24 sessions, 96 claims total.

## General grading rules

- `claim_correct` is graded per claim as one of:
  - `correct` — the claim asserts every element listed under the
    question's **Accept** criteria and asserts nothing that matches a
    **Reject** criterion.
  - `incorrect` — the claim asserts a Reject alternative, contradicts an
    Accept element, or hedges across alternatives without committing
    (e.g. "either X or Y"). A claim that is merely vaguer than the
    Accept elements (missing a required element) is also `incorrect`;
    there is no partial credit.
  - `unanswered` — no claim was recorded for the question (missing row,
    empty claim, or unparseable `open_answers.json`).
- Extra correct detail beyond the Accept elements never penalises;
  extra *incorrect* detail (a false assertion alongside a true one)
  makes the claim `incorrect`.
- The gold anchor is the spec's intention statement (quoted verbatim
  below from `benchmarks/specs/<task>.yaml`), operationalised by the
  underlying FR/SR statement. The MCQ gold/distractor texts are listed
  only as calibration for what counts as an accept/reject shape; the
  claim does not need to echo MCQ wording.
- `evidence_sufficient` is graded only after all 96 `claim_correct`
  labels are frozen (see procedure), as one of:
  - `sufficient` — at least one cited `evidence_files` path exists in
    the source artifact and that file actually contains the code
    implementing the accepted behaviour (the grader opens the file and
    checks). Citations that are only README/docs, or only tests that
    exercise the behaviour without any implementing source file cited,
    are not sufficient.
  - `insufficient` — evidence was cited but no cited file contains the
    implementing code (wrong file, nonexistent path, docs-only,
    tests-only).
  - `no_evidence_cited` — an `artifact_open` answer with empty
    `evidence_files` (a protocol violation by the finder; count it
    separately, do not fold it into `insufficient`).
  - `not_applicable` — all `question_only_open` rows, and rows with
    `claim_correct = unanswered`.
- Evidence sufficiency is judged against the behaviour the claim
  asserts, for `correct` and `incorrect` claims alike (an incorrect
  claim can still be checked for whether its cited file says what the
  finder says it says; record sufficiency of support for the *asserted*
  behaviour).

## Blind-grading procedure

1. Export the 96 claims from `rebuttal/open_pilot/records/*.json` into
   a grading sheet. The export assigns each row an opaque `grading_id`
   and **excludes** condition, finder, trial, seed, and run id from the
   grading sheet. A separate key file maps `grading_id` back to those
   columns; the key file is not opened until step 4.
   - Note: `question_only_open` rows will have empty
     `evidence_files` by design; the evidence columns therefore leak
     condition partially. To keep correctness grading blind, the
     grading sheet is split into two passes over the SAME sheet with
     evidence columns hidden during pass one (see step 2).
2. **Pass 1 — correctness.** Rows sorted in shuffled order (deterministic:
   sort by `sha256(f"{20260725}|{open_find_run_id}|{question_id}")`,
   computed at export time; the hash input is not shown to the grader).
   Evidence columns (`evidence_files`, `evidence_symbols`) hidden.
   Grade `claim_correct` for all 96 rows against this rubric. Freeze.
3. **Pass 2 — evidence.** Unhide the evidence columns. For each row,
   grade `evidence_sufficient` by opening the cited files inside the
   corresponding source artifact under `runs_release/artifacts/`
   (read-only). Freeze.
4. Only after both passes are frozen, join the key file and compute
   per-condition results.

## Grading-sheet CSV layout

`rebuttal/open_pilot/grading_sheet.csv` (visible to grader):

| column | filled by | notes |
|---|---|---|
| `shuffle_index` | export | 1..96, shuffled order (see step 2) |
| `grading_id` | export | opaque id, e.g. `g_0042` |
| `task_id` | export | needed to open the right artifact in pass 2 |
| `question_id` | export | |
| `question_prompt` | export | |
| `claim` | export | finder's free-form claim |
| `evidence_files` | export | `\|`-joined; hidden during pass 1 |
| `evidence_symbols` | export | `\|`-joined; hidden during pass 1 |
| `rationale` | export | |
| `claim_correct` | grader (pass 1) | `correct` / `incorrect` / `unanswered` |
| `evidence_sufficient` | grader (pass 2) | `sufficient` / `insufficient` / `no_evidence_cited` / `not_applicable` |
| `grader_note` | grader | free text, optional |

`rebuttal/open_pilot/grading_key.csv` (NOT opened until step 4):

| column | notes |
|---|---|
| `grading_id` | join key |
| `open_find_run_id` | |
| `condition` | `artifact_open` / `question_only_open` |
| `finder_id` | |
| `trial_id` | |
| `seed` | |
| `source_build_run_id` | empty for `question_only_open` |

---

## Per-question criteria

### q_db_01 — batch-mode exit codes (`scratch_minidb`, I-FR2)

Question: "Batch mode maps different failure categories to different
exit codes. Which mapping does the codebase implement?"

**Gold claim** — spec intention I-FR2 (verbatim):
> The binary supports single-statement batch evaluation via an --eval
> flag with distinct exit codes for usage, SQL, and IO errors.

Operationalised by FR2: exit codes are 0 on success, **1 on SQL error,
2 on CLI usage error, 3 on storage/IO error**.

**Accept** — the claim must assert (a) distinct exit codes per failure
category, and (b) the specific assignment 1 = SQL, 2 = CLI usage,
3 = storage/IO (0 = success may be implicit). All three non-zero
assignments must be correct.

**Reject** — common plausible alternatives:
- a single collapsed error code (exit 1 for everything, categories only
  distinguishable on stderr);
- sysexits.h-style codes (64/65/74);
- any permuted assignment (e.g. 2 = IO, 3 = usage);
- "non-zero on error" without the category mapping (vague → incorrect).

**Evidence sufficiency** — a cited file must contain the code that maps
error categories to process exit codes (e.g. the CLI entry point's
match on the error enum producing 1/2/3). A file that merely defines
the error enum without the exit-code mapping is insufficient.

### q_db_05 — query execution model (`scratch_minidb`, I-SR3)

Question: "What execution model does the query engine use?"

**Gold claim** — spec intention I-SR3 (verbatim):
> Execution uses a Volcano-style iterator model with open/next/close on
> a PhysicalOperator trait.

**Accept** — the claim must assert a pull-based / Volcano-style /
iterator execution model in which physical operators implement a common
trait with open/next/close-style methods returning one row (or a small
batch) at a time. Naming it "Volcano" or "iterator model" plus the
trait shape is enough; the exact trait name need not be
`PhysicalOperator`.

**Reject**:
- vectorised/columnar batch execution;
- full materialisation between operators;
- push-based dataflow-scheduler execution;
- "it executes the plan" without identifying the iterator model.

**Evidence sufficiency** — a cited file must contain the operator trait
definition (open/next/close) or an operator implementing it. Citing
only the planner or parser is insufficient.

### q_db_06 — secondary index organisation (`scratch_minidb`, I-SR7)

Question: "How are secondary indexes physically organised?"

**Gold claim** — spec intention I-SR7 (verbatim):
> Secondary indexes are B-trees keyed by (index_key, row_id) and live
> in a dedicated storage sub-module.

**Accept** — the claim must assert (a) a B-tree structure, and (b) the
composite key on (indexed value, row id) — or an equivalent statement
that duplicate values are disambiguated/ordered by appending the row
id to the key.

**Reject**:
- hash map keyed by value with a chained Vec<RowId>;
- per-value bitmaps;
- lazily-built in-memory inverted lists;
- "a B-tree" alone with an *incorrect* description of the keying
  (e.g. keyed by value only with a value->list payload). Note: "a
  B-tree keyed by the column value mapping to row ids" without any
  composite/duplicate-handling statement is missing element (b) →
  incorrect (no partial credit).

**Evidence sufficiency** — a cited file must contain the B-tree index
implementation showing the composite keying (e.g. `storage/btree.rs`
with the (key, row_id) entry type). Citing only the planner's index
selection is insufficient.

### q_db_15 — invalid `--format` value (`scratch_minidb`, I-FR15)

Question: "The user invokes `minidb --format yaml --eval "SELECT 1;"`.
`yaml` is not a supported value for `--format`. What does the process
do?"

**Gold claim** — spec intention I-FR15 (verbatim):
> SELECT output can be formatted as a human-readable table, CSV (RFC
> 4180), or JSON arrays of row-objects.

Operationalised by FR15 + FR2 interaction: `yaml` is not one of
`{table,csv,json}`, so this is a **CLI usage error**: the process
prints a usage error on stderr and **exits with code 2** without
executing the SELECT.

**Accept** — the claim must assert (a) the invocation is rejected as a
CLI/usage error (no silent fallback, the SELECT does not produce
output), and (b) exit code 2 (the usage-error code).

**Reject**:
- silent fallback to the default `table` format (statement executes);
- treated as a SQL error / exit code 1;
- panic / unwrap on the unrecognised value;
- "exits with an error" without identifying the usage-error class and
  code 2 (vague → incorrect).

**Evidence sufficiency** — a cited file must contain the CLI argument
validation that rejects unknown format values and produces the usage
exit code (e.g. clap value parsing plus the error-to-exit-code
mapping). Citing only the formatter implementations is insufficient.

### q_nw_01 — HTTP/1.1 parsing (`scratch_nanoweb`, I-FR2)

Question: "How is HTTP/1.1 parsing wired into this framework?"

**Gold claim** — spec intention I-FR2 (verbatim):
> The framework ships its own HTTP/1.1 parser that handles chunked and
> Content-Length bodies and rejects malformed requests.

**Accept** — the claim must assert that the framework implements its
own / hand-written HTTP parser (a dedicated parser module reading raw
bytes), rather than delegating parsing to a third-party HTTP library.
Mentions of chunked/Content-Length handling, 400 rejection, or the
"other layers consume typed Request/Response values" encapsulation are
supporting detail, not required.

**Reject**:
- a wrapper around hyper/httparse or any third-party HTTP parser;
- parsing delegated to an upstream proxy;
- lazy per-handler parsing with no central parser module.

**Evidence sufficiency** — a cited file must contain the hand-written
parsing code (request line / header / body parsing over bytes), e.g.
`src/http/parser.rs`. Citing only the type definitions (`Method`,
`Header`) is insufficient.

### q_nw_04 — middleware composition (`scratch_nanoweb`, I-SR4)

Question: "How is the middleware stack composed?"

**Gold claim** — spec intention I-SR4 (verbatim):
> Middleware composes via a single Middleware trait; registration
> order drives outer-to-inner call order.

**Accept** — the claim must assert (a) a single trait that all
middlewares implement, composed as a chain/list of trait objects (or
equivalent dynamic dispatch), and (b) that registration order
determines the call/wrapping order (outer-to-inner or an equivalent
ordering statement).

**Reject**:
- plain closures in a Vec with a per-request fold (no trait
  abstraction);
- compile-time proc-macro composition;
- reuse of tower `Layer`/`Service`;
- a trait-based chain with an *incorrect* ordering claim (e.g.
  "registration order is irrelevant").
Note: element (a) without any ordering statement is a missing element
→ incorrect (no partial credit).

**Evidence sufficiency** — a cited file must contain the middleware
trait definition and/or the chain-invocation code that walks the
registered middlewares in order. Citing only one concrete middleware is
insufficient.

### q_nw_07 — observability metrics (`scratch_nanoweb`, I-FR11)

Question: "How does the framework expose observability metrics?"

**Gold claim** — spec intention I-FR11 (verbatim):
> Prometheus metrics are exposed at a configurable endpoint, including
> counters by method/status/route and a request- duration histogram.

(The "request- duration" spacing is a YAML folding artifact in the
released spec; read "request-duration".) Operationalised by FR11: a
Prometheus **text-exposition** endpoint mounted at `/metrics`
(configurable), with request counters by method/status/route and a
request-duration histogram.

**Accept** — the claim must assert (a) metrics are exposed by an HTTP
scrape endpoint in Prometheus text format, and (b) at least one of:
labeled request counters (method/status/route), a request-duration
histogram, or the configurable `/metrics` path.

**Reject**:
- statsd UDP push per request;
- OpenTelemetry OTLP export to a collector;
- structured JSON log lines with downstream aggregation;
- "it has metrics" with no exposition mechanism (vague → incorrect).

**Evidence sufficiency** — a cited file must contain the metrics
registry/encoder or the endpoint handler that renders the Prometheus
text format. Citing only config defaults is insufficient.

### q_nw_10 — template engine (`scratch_nanoweb`, I-FR13)

Question: "What shape does the framework's built-in template engine
take?"

**Gold claim** — spec intention I-FR13 (verbatim):
> A mustache-like template engine is included (sections, inverted
> sections, simple variable interpolation).

**Accept** — the claim must assert a built-in, minimal mustache-style
renderer, identified either by name ("mustache-like"/"mustache-style")
or by its syntax: `{{name}}` variable interpolation plus `{{#...}}`
sections (inverted `{{^...}}` sections are supporting detail, not
required — but interpolation alone without sections, or "some
templating" with no shape, is a missing element → incorrect).

**Reject**:
- no template engine / handlers build HTML by hand;
- a full Jinja2-class engine (filters, macros, inheritance, sandboxed
  eval);
- a compile-time HTML macro (maud-style);
- delegation to an external templating crate presented as the built-in
  engine.

**Evidence sufficiency** — a cited file must contain the template
parser/renderer implementation (the code handling `{{ }}` tokens and
sections). Citing only an example template or a handler that calls
`render` is insufficient.

---

## Reporting

After unblinding, report per condition (pooled over finders and tasks,
and broken out per finder):
- claim correctness rate (out of 48 claims per condition);
- for `artifact_open` only: evidence sufficiency rate among `correct`
  claims (primary), and among all claims (secondary);
- placed side by side with the MCQ low-prior accuracies for the same 8
  questions.

No statistical test is pre-registered for n=96; report counts and
rates, and describe the direction of the artifact-vs-question-only gap.

## Amendments

(none)
