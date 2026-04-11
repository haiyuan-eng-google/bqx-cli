# BigQuery CLI Benchmark Results: dcx vs bq

Systematic latency, correctness, and token-efficiency comparison of `dcx`
against the standard `bq` CLI across 12 BigQuery tasks covering metadata
reads, SQL queries, dry-run validation, and error handling.

## Key Numbers

| Metric | dcx | dcx (minified) | bq |
|--------|-----|----------------|-----|
| **Average task p50 (warm)** | 648 ms | 613 ms | 2,939 ms |
| **Avg p50 ratio** | **4.5x faster** | **4.8x faster** | baseline |
| **Correctness (warm trials)** | 33/36 (92%) | 33/36 (92%) | 30/36 (83%) |
| **6-step workflow tokens** | ~2,859 | **~2,080** | ~2,115 |

`--format=json-minified` reduces output tokens by **27%** compared to
pretty JSON, bringing dcx below `bq`'s token cost while preserving the
same schema and envelope structure. No latency penalty — minified output
is slightly faster due to reduced serialization and I/O.

Results were directionally stable across 3 warm trials per task; see the
[scorecard](../benchmarks/results/scorecards/20260411-013709-b4c8ac5.md)
for variance bounds.

**Benchmark contract:** Correctness is defined by the deterministic
validation rules checked into
[`benchmarks/tasks/bigquery_overlap.yaml`](../benchmarks/tasks/bigquery_overlap.yaml)
— per-variant `json_keys`, `json_parse`, `semantic_sql_result`, or
`exit_code_only` checks executed automatically by the runner. No manual
judgment is involved in pass/fail determination.

## Scope and Limits

This benchmark measures a specific, narrow slice:

- **Single machine:** macOS (Darwin 25.3.0, x86_64), no cross-platform data
- **Single project:** one GCP project with ADC auth, no service-account or
  token-based flows
- **12 tasks:** 4 metadata reads, 2 SQL queries, 1 dry-run, 5 error-handling
  — read-heavy, no mutations, no concurrent load
- **3 CLI variants:** `dcx` (pretty JSON), `dcx-minified` (json-minified),
  `bq` (json)
- **One known bad task:** `bq-error-permission-denied` targets a public
  project and does not actually produce a permission error for either CLI
- **3 warm trials per task:** sufficient for directional conclusions, not for
  percentile-level statistical claims
- **No cloud-side telemetry:** bytes processed, slot milliseconds, and cache
  hit rates were not captured in this run

The results are directionally strong but should be validated on Linux CI
hosts and with higher trial counts before quoting in external materials.

## Test Environment

| | |
|---|---|
| Run ID | `20260411-013709-b4c8ac5` |
| dcx version | 0.5.0 (Rust, compiled release) |
| bq version | BigQuery CLI 2.1.28 (Python) |
| gcloud SDK | 559.0.0 |
| OS | macOS (Darwin 25.3.0, x86_64) |
| Auth | Application Default Credentials (ADC) |
| Region | us-central1 |
| Trials per task | 1 cold + 3 warm |

## Per-Task Results

### Metadata Operations

These are the bread-and-butter commands an agent uses to explore a project
before writing SQL: list datasets, inspect a dataset, list tables, get a
table's schema.

| Task | dcx p50 | dcx-minified p50 | bq p50 | Speedup (dcx) | dcx | bq |
|------|--------:|-----------------:|-------:|--------------:|:---:|:--:|
| List datasets | 875 ms | 821 ms | 3,123 ms | **3.6x** | PASS | PASS |
| Get dataset metadata | 692 ms | 589 ms | 2,807 ms | **4.1x** | PASS | PASS |
| List tables | 600 ms | 595 ms | 2,825 ms | **4.7x** | PASS | PASS |
| Get table schema | 690 ms | 583 ms | 2,784 ms | **4.0x** | PASS | PASS |

**Average metadata speedup: 4.1x.**

### SQL Queries

| Task | dcx p50 | dcx-minified p50 | bq p50 | Speedup (dcx) | dcx | bq |
|------|--------:|-----------------:|-------:|--------------:|:---:|:--:|
| Aggregate query (`COUNT`, `AVG`) | 875 ms | 874 ms | 3,261 ms | **3.7x** | PASS | PASS |
| Nested-field query (STRUCT access) | 820 ms | 815 ms | 3,255 ms | **4.0x** | PASS | PASS |

Query execution time is dominated by BigQuery server-side processing, so the
speedup here is ~3.9x rather than the higher ratios seen in metadata ops.
`dcx-minified` has identical latency since the serialization savings are
negligible relative to network round-trip time.

### Dry-Run (SQL Validation)

| Task | dcx p50 | dcx-minified p50 | bq p50 | Speedup (dcx) | dcx | bq |
|------|--------:|-----------------:|-------:|--------------:|:---:|:--:|
| Dry-run aggregate query | 97 ms | 101 ms | 3,317 ms | **34.2x** | PASS | PASS |

This is the most dramatic result. `dcx --dry-run` resolves entirely locally
(validates flags, builds the request, and returns the structured request body)
without any network call. `bq --dry_run` makes an API round-trip to BigQuery
servers and returns estimated bytes processed.

**Fairness note:** these are not identical product semantics. `dcx --dry-run`
previews the outbound request; `bq --dry_run` performs a server-side
validation pass. The 34.2x result is valid operationally — an agent using
dry-run to check flag/SQL structure before executing saves ~3 seconds per
step — but it is not a pure apples-to-apples API round-trip comparison.

### Error Handling

| Task | dcx p50 | dcx-minified p50 | bq p50 | Speedup (dcx) | dcx | bq |
|------|--------:|-----------------:|-------:|--------------:|:---:|:--:|
| Malformed SQL | 553 ms | 573 ms | 2,881 ms | **5.2x** | PASS | PASS |
| Nonexistent dataset | 657 ms | 661 ms | 2,881 ms | **4.4x** | PASS | PASS |
| Invalid auth | 179 ms | 177 ms | 3,303 ms | **18.4x** | PASS | FAIL |
| Invalid flag | 99 ms | 99 ms | 891 ms | **9.0x** | PASS | PASS |
| Permission denied | 1,643 ms | 1,473 ms | 3,938 ms | **2.4x** | * | * |

Error-handling speed matters because agent self-correction loops depend on
fast feedback. When an agent issues a bad query and needs to retry, `dcx`
returns the error 3–18x faster.

**Auth failure (18.4x):** `dcx` detects the invalid credential locally and
exits in 179 ms. `bq` with `--credential_file /dev/null` exits 0 and returns
data — the explicit credential override appears not to be honored in this
scenario, and `bq` falls back to ADC silently.

**Invalid flag (9.0x):** `dcx` validates flags locally (99 ms) before making
any network call. `bq` still takes ~891 ms to report the same error.

\* Permission-denied task targets `bigquery-public-data`, which is publicly
accessible. Both CLIs return exit 0 with data instead of a permission error.
This is a test-design issue, not a CLI issue.

## Architectural Factors (Inference)

The sections above report measured results. This section offers an
architectural explanation for the observed latency gap. These factors are
inferred from code-level knowledge of both CLIs, not directly measured by
this benchmark.

| Factor | dcx | bq |
|--------|-----|-----|
| **Language** | Compiled Rust binary | Python interpreter |
| **Startup** | ~5 ms (estimated) | ~300–500 ms (estimated) |
| **API calls** | Direct REST, minimal framing | Wraps `google-api-python-client` |
| **Auth** | Loads ADC/token directly | Routes through `gcloud auth` subsystem |
| **Validation** | Validates flags before network | Validates after starting API flow |

The Python interpreter startup cost appears in every `bq` invocation. In a
5-step agent workflow, this alone would add 1.5–2.5 seconds of overhead —
consistent with the gap observed in the workflow rollup below. Isolating
startup from API latency was not done in this run.

## Agent Workflow Impact

Consider a typical agent exploration loop:

```
list datasets → pick dataset → list tables → get schema → dry-run query → execute query
```

That's 6 sequential CLI calls. Using warm p50 numbers:

### Latency

| | dcx | dcx (minified) | bq |
|---|---|---|---|
| Total latency | 875 + 692 + 600 + 690 + 97 + 875 = **3,829 ms** | 821 + 589 + 595 + 583 + 101 + 874 = **3,563 ms** | 3,123 + 2,807 + 2,825 + 2,784 + 3,317 + 3,261 = **18,117 ms** |
| Wall clock | **~4 seconds** | **~4 seconds** | **~18 seconds** |

An agent using `dcx` completes the same exploration in **4 seconds vs 18
seconds** — a 4.7x end-to-end speedup. For iterative workflows where the
agent retries 2–3 times (common with self-correction), the gap widens to
~12 seconds vs ~54 seconds.

### Token Cost

| | dcx (json) | dcx (json-minified) | bq |
|---|---:|---:|---:|
| Total output bytes | 11,436 B | 8,319 B | 8,461 B |
| Estimated tokens (÷4) | ~2,859 | **~2,080** | ~2,115 |
| vs dcx (json) | baseline | **−27%** | −26% |

`--format=json-minified` closes the token gap entirely: dcx-minified uses
**~2,080 tokens** vs bq's **~2,115 tokens** — effectively equivalent, with
dcx providing a more consistent envelope structure.

## Token Efficiency

Agent workflows pay for every byte of CLI output that enters the LLM context
window. This section estimates token cost using the approximation
**1 token ~ 4 bytes** (conservative for JSON with repeated keys).

### Per-Task Token Comparison

| Task | dcx (json) | dcx (minified) | bq | Minified reduction |
|------|--------:|--------:|--------:|--------:|
| List datasets | 7,699 B (~1,925 tok) | 5,531 B (~1,383 tok) | 5,501 B (~1,375 tok) | **28%** |
| Get dataset | 812 B (~203 tok) | 645 B (~161 tok) | 650 B (~163 tok) | **21%** |
| List tables | 704 B (~176 tok) | 518 B (~130 tok) | 488 B (~122 tok) | **26%** |
| Get table schema | 1,272 B (~318 tok) | 986 B (~247 tok) | 203 B (~51 tok) | **22%** |
| Dry-run | 350 B (~88 tok) | 312 B (~78 tok) | 1,317 B (~329 tok) | **11%** |
| Aggregate query | 599 B (~150 tok) | 327 B (~82 tok) | 302 B (~76 tok) | **45%** |
| Nested query | 748 B (~187 tok) | 476 B (~119 tok) | 451 B (~113 tok) | **36%** |

**Average reduction with `json-minified`: 27%** across read/query tasks.

Tasks with more data (list datasets, query results) see the largest absolute
savings. Dry-run sees only 11% reduction because its output is already small
and has fewer nested structures to compact.

### Token Tradeoffs

The tradeoff is **parseability vs compactness**. `dcx` normalizes all list
responses to a consistent envelope:

```json
{"items": [...], "source": "BigQuery", "next_page_token": "..."}
```

`bq` returns raw API shapes that vary by command (JSON array, nested
`datasets` key, free-text error strings on stdout). An agent using `bq` must
carry per-command parsing logic in its prompt or tool definitions, which
itself consumes tokens. `dcx`'s uniform envelope covers every list command;
get, query, dry-run, and error responses still have their own shapes, but
the list normalization alone reduces the per-command parsing burden for the
most common discovery operations.

With `json-minified`, dcx achieves token parity with `bq` while retaining
the consistent envelope. The remaining token overhead (if any) comes from
richer field detail in metadata responses — information that is useful for
agent decision-making.

### Error Output Token Cost

Error handling reveals a structural difference in where each CLI puts
diagnostic information:

| Error task | dcx stderr | ~tokens | bq stdout | ~tokens |
|-----------|--------:|--------:|--------:|--------:|
| Malformed SQL | 174 B | ~44 | 170 B | ~43 |
| Not found | 187 B | ~47 | 106 B | ~27 |
| Auth failure | 326 B | ~82 | 5,501 B* | ~1,375 |
| Invalid flag | 143 B | ~36 | 101 B | ~25 |

`dcx` emits errors as structured JSON on stderr (`{"error":"..."}`), keeping
stdout clean. `bq` mixes error text into stdout for some errors and uses
stderr for others, with no consistent format.

\* `bq` returns the full dataset listing (5.5 KB) on the auth-failure task
because it falls back to ADC — the agent receives ~1,375 tokens of
unintended successful response instead of an error signal.

## Correctness Notes

Three tasks had validation outcomes worth discussing:

1. **bq-error-auth-failure:** `bq` exits 0 despite being given
   `--credential_file /dev/null`. The explicit credential override appears
   not to be honored in this scenario; `bq` falls back to ADC and succeeds.
   `dcx` returns exit 1 with a structured error.

2. **bq-error-permission-denied:** Both CLIs exit 0 because the test targets
   `bigquery-public-data`, which grants public read access. The task spec
   should be updated to target a genuinely restricted project.

3. **Overall correctness:** Excluding the permission-denied design issue,
   `dcx` passes 33/33 trials (100%). `bq` passes 30/33 (91%), with the
   auth-failure task accounting for all 3 failures.

## Product Implications

For `bq` CLI:

- **Normalized machine-output mode.** `bq` list commands return varying JSON
  shapes. A `--format=machine-json` mode with a stable envelope (items array,
  pagination token, source tag) would make `bq` output parseable without
  per-command logic.
- **Credential override behavior.** `--credential_file /dev/null` is silently
  ignored and ADC is used instead. Clarifying or fixing this would make
  explicit credential overrides trustworthy for CI and testing scenarios.
- **Local-only dry-run preview.** `bq --dry_run` always makes a server
  round-trip. A local-only mode that shows the constructed request (method,
  URL, body) without network access would enable faster agent preflight
  checks and offline validation.

For `dcx`:

- **Token parity achieved.** With `--format=json-minified`, dcx output
  tokens (~2,080) are now below `bq` (~2,115) for the standard 6-step
  exploration workflow. The 27% reduction from Phase 1 closes the gap that
  existed with pretty JSON output.
- **Further reduction possible.** Phase 2 (typed compact schemas) can strip
  redundant API fields (kind, selfLink, etag) and hoist projectId to the
  envelope, targeting an additional ~35% reduction on top of minification.
- **Permission-denied test gap.** The current benchmark does not exercise a
  real permission-denied scenario. Adding a task against a restricted project
  would validate dcx's error classification for this case.

## Next Benchmark

- Rerun on a Linux CI host (e.g., GitHub Actions runner) to confirm the
  speedup is not macOS-specific
- Rerun with a genuinely restricted project for the permission-denied task
- Add bytes-processed / job metadata appendix using
  `benchmarks/scripts/collect_bigquery_jobs.sql`
- Increase warm trials to 10+ for percentile-level statistical claims
- Benchmark Phase 2 compact output format when available

## Artifacts

- [Scorecard](../benchmarks/results/scorecards/20260411-013709-b4c8ac5.md)
- [Summary JSON](../benchmarks/results/raw/20260411-013709-b4c8ac5/summary.json)
- [Raw results (NDJSON)](../benchmarks/results/raw/20260411-013709-b4c8ac5/results.ndjson)
- [Environment snapshot](../benchmarks/results/raw/20260411-013709-b4c8ac5/environment.json)
- [Benchmark methodology](cli_benchmark_plan.md)

### Previous Runs

- [Run 20260410-171926](../benchmarks/results/scorecards/20260410-171926-cdc4d94.md)
  — initial benchmark (dcx vs bq only, no json-minified variant)

## Reproduction

```bash
# Build dcx
cargo build --release
export PATH="target/release:$PATH"

# Seed benchmark data
benchmarks/scripts/seed_bigquery.sh YOUR_PROJECT_ID

# Configure manifest
sed -i '' 's/YOUR_PROJECT_ID/your-actual-project/' benchmarks/manifest.yaml

# Run (1 cold + 3 warm trials)
benchmarks/scripts/run_benchmarks.sh --tasks bigquery_overlap --trials 3 --cold-trials 1

# Generate scorecard
python3 benchmarks/scripts/score_results.py benchmarks/results/raw/<run-id>
```
