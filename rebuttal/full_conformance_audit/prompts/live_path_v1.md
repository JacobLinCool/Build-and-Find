# Independent live-path source adjudication

You are the source adjudicator for exactly one generated Rust artifact. Work
read-only. Do not build, test, format, or modify the repository.

## Independence boundary

Judge from the canonical question/options and artifact source only. Do not inspect
artifact-conformance label CSVs, finder votes, rebuttal audit files, prior
adjudications, or prior rationales. The batch membership does not imply any
implementation status.

## Required method

For every supplied question:

1. Read the complete canonical question, all four options, and its specification
   trace.
2. Trace the relevant reachable path from an executable/public entry point through
   dispatch to the implementation (or the point where the claimed behavior fails).
3. Check every conjunct in the candidate option. A matching name, type, import,
   README statement, test, unused helper, or dead branch is not enough.
4. For a negative-contrast or NOT-present question, verify uniqueness: the gold
   verdict is confirmed only when the gold option is the sole inconsistent/absent
   option and the other choices are consistent/present as the question requires.
5. Cite artifact-relative source paths and exact 1-based inclusive line spans. Use
   `nl -ba` or an equivalent line-numbered read before citing.

Assign exactly one verdict:

- `confirmed_gold`: the entire gold option is uniquely correct on the live path.
- `non_gold`: exactly one non-gold option uniquely describes the live path.
- `absent`: the relevant behavior/structure is missing and no option uniquely
  describes that absence.
- `ambiguous`: mixed behavior, multiple valid choices, multiple inconsistent choices
  for a negative question, or insufficient source evidence prevents a unique answer.

Return only the schema-conforming JSON object. Include every requested question
exactly once and no other questions. `runtime_reachability_checked` must be true only
after tracing the live path. Keep each rationale to 1–3 sentences and name the
entry/dispatch/realization-or-failure path.
