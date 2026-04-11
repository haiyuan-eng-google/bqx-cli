# Execution Plan: Compact Output

Three PRs, each independently mergeable and contract-consistent at merge
time. Based on the approved design in
[compact-output-proposal.md](compact-output-proposal.md).

---

## PR 1: Add `--format=json-minified` (runtime + contract + docs)

**Goal:** Add the new output format across all code paths, update the
machine contract and generated skills to include it, and document it.
Every merged state leaves the repo contract-consistent.

### Runtime changes

**1. `OutputFormat` enum** (`src/cli.rs:560-565`)

```rust
#[derive(Clone, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Json,
    #[value(name = "json-minified")]
    JsonMinified,
    Table,
    Text,
}
```

**2. Central renderer** (`src/output.rs:7-24`)

Add `JsonMinified` arm to `render()`:

```rust
OutputFormat::Json => {
    let json = serde_json::to_string_pretty(value)?;
    println!("{json}");
}
OutputFormat::JsonMinified => {
    let json = serde_json::to_string(value)?;
    println!("{json}");
}
```

Also handle the two `to_string_pretty` fallbacks in `fmt_value_as_table()`
at lines 83 and 86 — these only fire for `Table` format, so no change
needed (they don't match `Json` or `JsonMinified`).

**3. Dynamic executor** (`src/bigquery/dynamic/executor.rs`)

Three functions need the new arm:

- `render_dry_run_response()` (line 200-207): Add `JsonMinified` arm
  using `to_string()`.
- `render_response()` (line 262-275): Add `JsonMinified` arm. Normalize
  list responses the same way, just serialize with `to_string()`.
- `render_page_all_response()` (line 290-302): Add `JsonMinified` arm.

**4. Analytics commands** (8 files)

All use the same pattern — `OutputFormat::Json => { output::render(...) }`.
Each needs `OutputFormat::JsonMinified` added to the match. Since they
all delegate to `output::render()`, they can share the arm:

```rust
OutputFormat::Json | OutputFormat::JsonMinified => {
    output::render(result, &config.format)?;
}
```

Files:
- `src/commands/analytics/list_traces.rs:203`
- `src/commands/analytics/drift.rs:233`
- `src/commands/analytics/distribution.rs:183`
- `src/commands/analytics/get_trace.rs:201`
- `src/commands/analytics/categorical_eval.rs:577`
- `src/commands/analytics/hitl_metrics.rs:306`
- `src/commands/analytics/views.rs:169,329`
- `src/commands/analytics/insights.rs:417`
- `src/commands/analytics/categorical_views.rs:217`

**5. Meta commands** (`src/commands/meta.rs:122-164`)

Three functions match `OutputFormat::Json => output::render(...)`.
Add `JsonMinified` to each:

```rust
OutputFormat::Json | OutputFormat::JsonMinified => output::render(&list, format),
```

**6. Generate skills** (`src/commands/generate_skills.rs:36,49`)

Same pattern — add `JsonMinified` to the match arms.

**7. Database helpers** (`src/commands/database_helpers.rs:534`)

One match on `OutputFormat::Json` — add `JsonMinified`.

### Contract changes

**8. `runtime_behavior()` format lists** (`src/commands/meta.rs`)

The `formats` field in `RuntimeBehavior` is hardcoded per command group
and does NOT auto-derive from the clap enum. Every `formats: vec![...]`
entry needs `json-minified` added:

- Line 739: `auth check` — `vec!["json", "json-minified", "table", "text"]`
- Line 757: `profiles test` — same
- Line 766: `generate-skills | profiles | meta` — same
- Line 775: `analytics evaluate | drift` — same
- Line 788: `analytics` (other) — same
- Line 807: `ca ask` — same
- Line 826: `ca` (other) — same
- Line 843: dynamic commands — same

**9. Hardcoded value parser** (`src/commands/meta.rs:919`)

The test/contract builder's `.value_parser(["json", "table", "text"])`
must add `"json-minified"`.

**10. Skill templates** (`src/skills/templates.rs`)

Two hardcoded format references in generated skill content:

- Line 203: `"- Use \`--format table\` for visual scanning, \`--format json\` for piping\n"`
  → Add: `"- Use \`--format json-minified\` for agent pipelines (same schema, ~32% fewer tokens)\n"`
- Line 290: `" \\\n  [--format json|table|text]\n```\n\n"`
  → Change to: `" \\\n  [--format json|json-minified|table|text]\n```\n\n"`

**11. Contract tests** (`src/commands/meta.rs:1091,1110,1376,1407`)

Existing contract assertions check `formats.is_empty()` for some commands
and `!formats.is_empty()` for others. These still pass since we are adding
to the list, not removing. No changes needed, but verify.

### Documentation changes

**12. README**

Add one line to the Output Format section:

```markdown
Use `--format=json-minified` for agent/CI pipelines — same schema,
28% fewer tokens (benchmarked).
```

**13. Benchmark doc**

Add a note to `docs/benchmark_results_bigquery.md` Token Efficiency
section noting that `json-minified` closes the token gap.

### What does NOT change

- `--format=json` behavior (still pretty-printed)
- `--format=table` / `--format=text`
- MCP bridge (still forces `json` — changed in PR 2)
- Error output on stderr
- Test assertions (all existing tests use `OutputFormat::Json`)

### Tests

- Add unit tests for `output::render()` with `JsonMinified` — verify
  output is valid single-line JSON with same keys as pretty.
- Add integration tests for a few representative commands:
  `datasets list`, `tables get`, `jobs query`, `analytics evaluate`.
- Verify `--format=json-minified` is accepted by clap parser.
- Verify `meta describe` output includes `json-minified` in format list.
- Verify generated skill content includes `json-minified`.

### Estimated size

~110 lines changed across 16 source files + documentation.

---

## PR 2: MCP bridge defaults to `json-minified`

**Goal:** MCP consumers get the 27% token reduction automatically.

### Changes

**1. MCP execute_tool** (`src/commands/mcp.rs:214-216`)

```rust
// Before:
args.push("--format".to_string());
args.push("json".to_string());

// After:
let mcp_format = std::env::var("DCX_MCP_FORMAT")
    .unwrap_or_else(|_| "json-minified".to_string());
args.push("--format".to_string());
args.push(mcp_format);
```

**2. MCP schema comment** (`src/commands/mcp.rs:79`)

Update comment:
```rust
// Skip --format: the MCP bridge defaults to json-minified.
// Override with DCX_MCP_FORMAT=json for debugging.
```

### Tests

- MCP integration test: verify tool call output is minified JSON by default.
- MCP integration test: verify `DCX_MCP_FORMAT=json` produces pretty JSON.
- Verify existing MCP snapshot tests still pass (update golden files from
  pretty to minified).

### Estimated size

~10 lines changed in 1 source file + snapshot test updates.

---

## PR 3+: Typed compact schemas (Phase 2, future)

**Goal:** Per-resource `Compact*` structs for BigQuery dynamic commands.

This is the larger body of work from Phase 2 of the proposal. It should
be scoped after Phase 1 ships and real agent usage data is available.

### Scope per PR

One PR per resource group:

| PR | Resources | Structs |
|----|-----------|---------|
| 3a | datasets list/get | `CompactDatasetListItem`, `CompactDataset` |
| 3b | tables list/get | `CompactTableListItem`, `CompactTable` |
| 3c | routines, models list | `CompactRoutineListItem`, `CompactModelListItem` |
| 3d | Spanner resources | `CompactSpannerInstance`, etc. |
| 3e | AlloyDB / CloudSQL / Looker | Remaining services |

Each PR includes:
- Typed struct with `#[derive(Serialize)]`
- Transform function in new `src/bigquery/compact.rs` module
- Envelope with `project_id` hoisted
- Snapshot tests for compact output
- Contract tests (compact fields subset of full fields)
- `runtime_behavior()` and skill template updates for `compact`
  (same pattern as PR 1 — contract stays consistent at merge)

### Dependencies

- PR 3a (or a dedicated prep PR) adds the `OutputFormat::Compact` variant
  to the enum and wires it through `output::render()`, `executor.rs`,
  and the match arms already updated in PR 1. This is the same kind of
  enum-expansion work as PR 1 but for a new format with different
  serialization logic.
- PR 3a-3e are independent of each other after the enum variant exists.

---

## Execution order

```
PR 1 (json-minified: runtime + contract + docs)
  ├── PR 2 (MCP default)      — can start immediately after PR 1 merges
  └── PR 3a-3e (compact)      — design after Phase 1 ships
```

PR 1 is self-contained: every merged state leaves runtime, contract, and
documentation consistent. No temporary drift between what the CLI accepts
and what `meta describe` / skills advertise.

## Validation

**Completed.** Benchmark run `20260411-013709-b4c8ac5` measured actual
token savings after PR 1 + PR 2 merged:

| | dcx (json) | dcx (json-minified) | bq |
|---|---:|---:|---:|
| 6-step workflow bytes | 11,436 B | 8,319 B | 8,461 B |
| Estimated tokens (÷4) | ~2,859 | **~2,080** | ~2,115 |
| Reduction vs json | — | **27%** | — |

Result: **27% reduction** (predicted 32%). dcx-minified now uses fewer
tokens than bq (2,080 vs 2,115) — token parity achieved.
