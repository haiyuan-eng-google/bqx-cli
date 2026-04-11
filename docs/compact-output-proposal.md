# Proposal: Reduce Output Token Cost

## Problem

dcx (pretty JSON) outputs ~35% more tokens than `bq` in a typical 6-step
agent workflow (2,859 vs 2,115 tokens). For agent-native tooling that bills
per token, this matters. The bloat comes from three independent sources.

### Source 1: Pretty-printed JSON (biggest contributor)

dcx uses `serde_json::to_string_pretty()` for all JSON output. Pretty-printing
adds indentation whitespace that roughly doubles the byte count.

**datasets list** — dcx: 7,699 B (pretty) vs bq: 5,501 B (minified).
Same data, same fields. If dcx minified, it would be ~5,300 B — slightly
*smaller* than bq because the `items` envelope replaces per-item `kind`
lookups.

**query result** — dcx: 599 B (pretty) vs bq: 302 B (minified).
Minified dcx would be ~340 B, nearly matching bq.

### Source 2: Redundant fields passed through from the API

dcx wraps the raw API response into the `items` envelope but passes through
every field the API returns, including fields that are redundant or low-value
for agents:

| Field | Example | Why redundant |
|-------|---------|---------------|
| `kind` | `"bigquery#dataset"` | Same for every item in a list. The `source` envelope already identifies the service. |
| `id` | `"project:dataset"` | Concatenation of `datasetReference.projectId` + `datasetReference.datasetId`. Fully derivable. |
| `selfLink` | `https://www.googleapis.com/...` | URL to the REST resource. No agent workflow needs this. |
| `etag` | `"YEoBUwSxf2W1Fzlg6er1FA=="` | Cache validator. Only useful for conditional requests dcx doesn't support. |
| `creationTime` / `lastModifiedTime` | `"1775865865837"` | Epoch millis as strings. Useful in get responses, but in list responses they add noise. |
| `numLongTermBytes`, etc. (10 fields) | `"0"` | Zero-value storage breakdown fields. Rarely needed. |

**tables get** — dcx: 1,272 B vs bq: 203 B (6.3x).
This is the worst ratio, but the comparison is not apples-to-apples:
`bq show --schema` returns only the schema fields array, while
`dcx tables get` returns the full table resource. These are different
semantic contracts. The full resource includes 10 `num*Bytes` fields,
`selfLink`, `etag`, `kind`, and nested `tableReference`. A fair
comparison would be dcx's full resource (1,272 B) vs bq's full resource
(`bq show --format=prettyjson`, which returns a similar payload).
The token-saving opportunity is real, but it requires defining a new,
purpose-built compact schema — not just trimming the existing one.

### Source 3: Nested reference objects

API responses include `datasetReference`, `tableReference` etc. — nested
objects that repeat `projectId` on every item. In a 28-dataset list response,
`"projectId": "test-project-0728-467323"` appears 28 times (1,036 bytes of
repeated project ID alone).

---

## Phased Approach

These sources have very different risk profiles. We split the work into two
phases: a safe, schema-preserving minification first, then a proper compact
schema second.

---

## Phase 1: Add `--format=json-minified`

### What changes

Add a new `OutputFormat::JsonMinified` variant that serializes with
`serde_json::to_string()` instead of `to_string_pretty()`. Same fields,
same envelope, same contract — just no indentation whitespace.

`--format=json` remains pretty-printed (current behavior, unchanged).

### Why a new format value instead of changing the default

Changing the default JSON rendering is a product decision, not a free
optimization. Every current user who pipes `dcx ... --format=json` and
reads the output visually would see a wall of text. That may be
acceptable for an agent-native CLI, but it is a breaking UX change that
should be made deliberately — not bundled into a token-optimization PR.

Adding `json-minified` as an explicit opt-in:
1. **No breaking change.** `--format=json` stays pretty-printed.
2. **Clear intent signal.** An agent or script that passes `json-minified`
   is explicitly saying "I am a machine consumer, optimize for tokens."
3. **Future default candidate.** If adoption shows most callers use
   `json-minified`, a future release can flip the default and add
   `--format=json-pretty` as the escape hatch. That is a separate decision.
4. **`bq` precedent.** `bq ls --format=json` outputs minified. dcx
   offering the same via `json-minified` achieves parity without forcing
   it on existing users.

### Token impact (Phase 1 — measured)

Measured in benchmark run `20260411-013709-b4c8ac5` (3 warm trials per task):

| Task | json (B) | json-minified (B) | Reduction |
|------|----------:|------------:|----------:|
| datasets list (28 items) | 7,699 | 5,531 | 28% |
| datasets get | 812 | 645 | 21% |
| tables list (2 items) | 704 | 518 | 26% |
| tables get | 1,272 | 986 | 22% |
| dry-run | 350 | 312 | 11% |
| query (10 rows) | 599 | 327 | 45% |
| nested query | 748 | 476 | 36% |
| **6-step workflow** | **11,436** | **8,319** | **27%** |
| **Est. tokens** | **~2,859** | **~2,080** | **27%** |

Phase 1 brings dcx from 35% above bq to slightly *below* bq on tokens per
workflow (2,080 vs 2,115) — token parity achieved with zero contract risk.

### MCP behavior (Phase 1)

MCP bridge (`dcx mcp serve`) adopts `json-minified` by default. Rationale:

- MCP consumers are always machines (LLMs / agent frameworks). There is no
  human-readability concern.
- MCP already forces `--format=json` and excludes mutations. Switching to
  minified is consistent with the "machine-only surface" design.
- Configurable via `DCX_MCP_FORMAT` env var if an MCP client needs pretty
  JSON for debugging (e.g. `DCX_MCP_FORMAT=json dcx mcp serve`).

This means MCP output gets the 27% token reduction automatically without
any client-side opt-in.

### Implementation scope

| Location | What changes |
|----------|-------------|
| `cli.rs:560-565` | `OutputFormat` enum: add `JsonMinified` variant |
| `cli.rs` (clap derive) | `--format` value parser: accept `json-minified` |
| `output.rs:10` | `render()`: add `JsonMinified` arm using `to_string()` |
| `executor.rs:269,271,274` | `render_response()`: add `JsonMinified` arm |
| `executor.rs:292` | `render_page_all_response()`: add `JsonMinified` arm |
| `commands/*.rs` | Any command with direct `to_string_pretty` calls: handle new variant |
| `mcp/` | MCP bridge: change forced format from `Json` to `JsonMinified` |
| Snapshot tests | Add `*.json-minified.stdout` golden files alongside existing ones |

**Not affected:** `--format=json` (unchanged), `--format=table`,
`--format=text`, error output on stderr.

---

## Phase 2: Compact Schema (`--format=compact`)

Phase 2 introduces a genuinely different output schema: fewer fields,
flattened references, purpose-built for minimal-token agent consumption.
This is a larger project with real contract implications.

### Design principles

1. **New format value, not a modification of `json`.** `compact` is a
   separate `OutputFormat` variant with its own documented schema.
2. **Per-resource structs, not heuristic pruning.** Each resource type gets
   an explicit `Compact*` struct with `#[derive(Serialize)]`. The output
   shape is deterministic and documentable via `meta describe`.
3. **Envelope preserved with context hoisted.** Instead of dropping
   `projectId` entirely (which breaks logging, caching, and piping), hoist
   shared context to the envelope:
   ```json
   {"project_id":"myproject","items":[{"dataset_id":"adk_logs","location":"us-central1"}],"source":"BigQuery"}
   ```
   Items reference the resource by local ID; the envelope carries the
   project context. Output remains self-contained and round-trippable
   without relying on CLI invocation state.
4. **Dry-run fidelity preserved.** `--dry-run` exists to show exactly what
   would be sent. Compact mode does not strip fields from dry-run output —
   it only minifies. The whole point of dry-run is request-level
   reproducibility.
5. **Scoped to dynamic commands initially.** Phase 2 applies to
   Discovery-generated commands (BigQuery, Spanner, AlloyDB, Cloud SQL,
   Looker) where the API response shape is known and stable. Static commands
   (analytics, ca) already have lean typed responses and don't need compact.

### Compact output shapes

#### `datasets list --format=compact`

```json
{"project_id":"test-project-0728-467323","items":[{"dataset_id":"adk_logs","location":"us-central1"},{"dataset_id":"bq_demo","location":"US"}],"source":"BigQuery"}
```

vs current minified json (Phase 1):
```json
{"items":[{"datasetReference":{"datasetId":"adk_logs","projectId":"test-project-0728-467323"},"id":"test-project-0728-467323:adk_logs","kind":"bigquery#dataset","location":"us-central1","type":"DEFAULT"}],"source":"BigQuery"}
```

Changes from minified json:
- `datasetReference` flattened to `dataset_id`
- `projectId` hoisted to envelope
- `id`, `kind`, `type` dropped

#### `tables get --format=compact`

```json
{"project_id":"test-project-0728-467323","dataset_id":"dcx_benchmark","table_id":"narrow_events","num_rows":"20","num_bytes":"644","type":"TABLE","schema":{"fields":[{"name":"event_id","type":"STRING","mode":"REQUIRED"},{"name":"event_type","type":"STRING","mode":"REQUIRED"},{"name":"created_at","type":"TIMESTAMP","mode":"REQUIRED"},{"name":"value","type":"FLOAT"}]}}
```

Changes from minified json:
- `tableReference` flattened to `table_id` + `dataset_id`; `project_id`
  hoisted to top level
- 10 `num*` storage fields collapsed to `num_rows` + `num_bytes`
- `selfLink`, `etag`, `kind`, composite `id` dropped

Note: this is a *different semantic contract* from the full table resource.
It is closer to what an agent needs for schema inspection, but it is not a
drop-in replacement for `tables get --format=json`. The benchmark doc should
not compare compact tables-get bytes against bq's `show --schema` bytes as
if they were the same operation.

#### `jobs query --format=compact`

Same as minified json. Query results are already lean:
```json
{"total_rows":10,"rows":[{"total":"25568","word":"the"},{"total":"21028","word":"I"}]}
```

#### `jobs query --dry-run --format=compact`

Same as minified json. All fields preserved:
```json
{"dry_run":true,"url":"https://bigquery.googleapis.com/...","method":"POST","body":{"query":"SELECT ...","useLegacySql":false,"location":"US"}}
```

### Per-resource compact structs

| Resource | Compact struct fields |
|----------|---------------------|
| datasets (list item) | `dataset_id`, `location` |
| datasets (get) | `dataset_id`, `location`, `type`, `access` |
| tables (list item) | `table_id`, `type` |
| tables (get) | `table_id`, `dataset_id`, `num_rows`, `num_bytes`, `type`, `schema` |
| routines (list item) | `routine_id`, `routine_type`, `language` |
| models (list item) | `model_id`, `model_type`, `location` |
| jobs query | `total_rows`, `rows` (unchanged) |
| jobs dry-run | all fields (unchanged, minified only) |

Envelope always includes `project_id` and `source` for list responses.
Get responses include `project_id` at the top level.

### Token impact (Phase 1 + Phase 2 combined)

| Task | Pretty (B) | Minified (B, measured) | Compact (B, est.) |
|------|----------:|------------:|------------:|
| datasets list (28) | 7,699 | 5,531 | ~1,500 |
| datasets get | 812 | 645 | ~380 |
| tables list (2) | 704 | 518 | ~200 |
| tables get | 1,272 | 986 | ~320 |
| dry-run | 350 | 312 | ~312 |
| query (10 rows) | 599 | 327 | ~327 |
| **Workflow total** | **11,436** | **8,319** | **~3,039** |
| **Est. tokens** | **~2,859** | **~2,080** | **~760** |

Phase 1 alone: **27% reduction** (2,859 → 2,080 tokens, measured).
Phase 1 + 2: **~73% reduction** (2,859 → ~760 tokens, estimated).

### Implementation scope (Phase 2)

This is significantly larger than Phase 1:

| Work item | Scope |
|-----------|-------|
| `OutputFormat::Compact` variant | `cli.rs`, clap derive |
| Per-resource `Compact*` structs | New module, e.g. `src/bigquery/compact.rs` |
| Compact transform in executor | `executor.rs`: new code path alongside `normalize_list_response` |
| Envelope with `project_id` | Modify envelope construction to include project context |
| `meta describe` support | Compact schema must be introspectable |
| MCP contract decision | MCP stays on `json-minified` (Phase 1) by default. `compact` available via `DCX_MCP_FORMAT=compact`. Not auto-adopted — agents must opt in. |
| Skills/docs update | Compact schema documentation |
| Snapshot tests | New golden files for each resource × compact |
| Contract tests | Compact fields are a strict subset of full fields |
| Per-service allowlists | Spanner, AlloyDB, Cloud SQL, Looker — each needs its own struct |

Estimated: 400-600 lines of new code + test updates.

---

## Recommendation

1. **Ship Phase 1 now.** `--format=json-minified` is low-risk (additive,
   no default change), gets 27% token savings for opt-in callers (measured),
   and gives MCP the reduction automatically. The main work is adding the
   new `OutputFormat` variant and snapshot tests.

2. **Design Phase 2 after Phase 1 ships.** The compact schema needs real
   agent feedback to validate the field selections. Phase 1 gives us a
   minified baseline to measure Phase 2 improvements against — separating
   "same payload, less whitespace" from "different, purpose-built schema."

3. **Default-flip is a separate decision.** If adoption data shows most
   callers use `json-minified`, a future release can make minified the
   default for `--format=json` and add `--format=json-pretty`. That is a
   product decision, not part of this proposal.

---

## Risks and Mitigations

| Risk | Phase | Mitigation |
|------|-------|-----------|
| Snapshot test churn | 1 | New golden files for `json-minified`; existing `json` tests unchanged |
| Agents depend on compact-dropped fields | 2 | Compact is opt-in; `json` (minified) is unchanged |
| `project_id` in envelope adds bytes vs dropping it | 2 | ~40 bytes per response; worth it for self-contained output |
| Compact schema locks in field selections | 2 | Start with a small set of resources; expand based on usage |
| `bq` comparison becomes unfair in benchmarks | 2 | Report minified (Phase 1) vs bq; report compact savings separately |

## Testing

### Phase 1
- Add `*.json-minified.stdout` snapshot files (existing `json` snapshots unchanged)
- Verify MCP bridge output switches to valid minified JSON
- Test `DCX_MCP_FORMAT=json` override restores pretty output for MCP debugging

### Phase 2
- Snapshot tests for each resource × compact output shape
- Contract tests: compact output fields are a strict subset of json output
- Round-trip test: compact output + envelope `project_id` is sufficient to
  reconstruct the full resource identifier
- Token-count regression test: compact workflow total stays under threshold
