# dcx — Agent-Native Data Cloud CLI

An agent-native CLI for Google Cloud's Data Cloud, built in Rust.
[**4.8x faster than `bq`**](docs/benchmark_results_bigquery.md) with
token-parity via `--format=json-minified`.

## Why dcx

- **One CLI for five services.** BigQuery, Spanner, AlloyDB, Cloud SQL,
  and Looker through a single binary — no per-service tooling.
- **Machine-safe output.** Structured JSON on stdout, typed errors on
  stderr, deterministic exit codes. Built for agents, scripts, and CI.
- **Faster feedback loops.** Compiled Rust, local-first validation,
  read-only API surface — [benchmarked](docs/benchmark_results_bigquery.md)
  against `bq` on real workflows.

## Quick Start

```bash
# Install
npm install -g dcx            # or: npx dcx --help

# First 5 commands
dcx auth check                # verify credentials
dcx datasets list --project-id=myproject
dcx jobs query --project-id=myproject --query="SELECT 1"
dcx meta describe jobs query  # inspect any command's contract
dcx mcp serve                 # start MCP server for agents
```

Prebuilt binaries for macOS, Linux, and Windows (x64 + ARM64) are
published as npm optional dependencies. See `npm info dcx` for details.

---

## Commands

dcx has five command domains sharing auth, output formatting,
pagination, and profiles.

```
dcx <resource> <method>              # BigQuery (dynamic, from Discovery)
dcx <service> <resource> <method>    # Spanner / AlloyDB / Cloud SQL / Looker
dcx analytics <command>              # Agent Analytics SDK
dcx ca <command>                     # Conversational Analytics
dcx meta|auth|profiles|mcp <cmd>    # Tooling and introspection
```

### BigQuery (dynamic)

Generated from the BigQuery v2 Discovery Document. Read-only.

```bash
dcx datasets list --project-id=myproject
dcx tables get --project-id=myproject --dataset-id=analytics --table-id=events
dcx jobs query --project-id=myproject --query="SELECT COUNT(*) FROM analytics.events"
dcx jobs query --query="SELECT 1" --dry-run   # local-only, no network
```

### Data Cloud APIs (dynamic)

Same Discovery-driven pipeline, namespaced per service.

```bash
dcx spanner databases list --project-id=myproject --instance-id=my-inst
dcx alloydb clusters list --project-id=myproject
dcx cloudsql instances list --project-id=myproject
dcx looker dashboards get --profile=sales-looker --dashboard-id=42
```

### Agent Analytics

Wraps the BigQuery Agent Analytics SDK (12 commands, 6 evaluators).

```bash
dcx analytics doctor
dcx analytics evaluate --evaluator=latency --threshold=5000 --last=1h --exit-code
dcx analytics list-traces --last=7d --agent-id=support_bot
```

Exit codes: 0 = success, 1 = evaluation failure (`--exit-code`),
2 = infrastructure error.

### Conversational Analytics

Natural-language queries across 6 data sources via `ca ask`.

```bash
dcx ca ask "What were the top errors yesterday?" --agent=agent-analytics
dcx ca ask --profile=finance-spanner.yaml "total revenue by region"
dcx ca create-agent --name=agent-analytics --tables=myproject.analytics.events
```

Sources: BigQuery, Looker, Looker Studio (Chat/DataAgent),
AlloyDB, Spanner, Cloud SQL (QueryData).

### Introspection and Tooling

```bash
dcx meta commands --format=json       # machine-readable command contract
dcx mcp serve                         # MCP bridge (JSON-RPC 2.0 / stdio)
dcx auth login                        # interactive OAuth
dcx profiles validate --profile=bench # validate source profiles
```

---

## Output Format

All commands default to structured JSON. `--format` controls output.
Use `--format=json-minified` for agent/CI pipelines — same schema, 27%
fewer tokens ([benchmarked](docs/benchmark_results_bigquery.md)).

```bash
dcx analytics evaluate --evaluator=latency --threshold=5000 --last=1h
# → {"evaluator":"latency","pass_rate":0.70,"sessions":[...]}

dcx analytics evaluate --evaluator=latency --threshold=5000 --last=1h --format=table
# → SESSION_ID   PASSED  LATENCY_MS  SCORE
#   sess-001     true    2340        0.85

dcx jobs query --query="SELECT 1" --dry-run
# → {"dry_run":true,"url":"...","method":"POST","body":{...}}
```

List responses use a normalized envelope:
```json
{"items": [...], "source": "BigQuery", "next_page_token": "..."}
```

Errors are structured JSON on stderr:
```json
{"error": "Dataset not found: does_not_exist"}
```

---

## Authentication

Five methods in priority order:

| Priority | Method | Use Case |
|----------|--------|----------|
| 1 | `DCX_TOKEN` env var | Pre-obtained access token |
| 2 | `DCX_CREDENTIALS_FILE` env var | Service account JSON |
| 3 | `dcx auth login` | Interactive OAuth (AES-256-GCM at rest) |
| 4 | `GOOGLE_APPLICATION_CREDENTIALS` | Standard ADC |
| 5 | `gcloud auth application-default` | Implicit gcloud credentials |

```bash
# Uses existing gcloud credentials (priority 5)
dcx datasets list --project-id=myproject

# Service account for CI
export DCX_CREDENTIALS_FILE=/path/to/sa-key.json
dcx analytics evaluate --evaluator=latency --last=24h --exit-code
```

---

## MCP Bridge

`dcx mcp serve` exposes all read-only commands as an MCP (Model Context
Protocol) server over stdio, using JSON-RPC 2.0.

- Domain filtering via `MCP_DOMAINS` env var
- Mutations and interactive commands excluded
- Output defaults to `json-minified` (~27% fewer tokens); override with
  `DCX_MCP_FORMAT=json` for debugging

```bash
# Start MCP server
dcx mcp serve

# Example JSON-RPC request
echo '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"dcx_datasets_list","arguments":{"project_id":"myproject"}}}' | dcx mcp serve
```

Supported methods: `initialize`, `tools/list`, `tools/call`, `ping`.

---

## Skills

14 declarative skills following the [Agent Skills](https://agentskills.io)
open standard. Compatible with Claude Code, Gemini CLI, Cursor, Copilot,
and Codex.

| Type | Count | Generated? | Skills |
|------|-------|------------|--------|
| Router | 6 | No | `dcx-bigquery`, `dcx-analytics`, `dcx-ca`, `dcx-databases`, `dcx-looker`, `dcx-profiles` |
| API | 5 | Yes | `dcx-bigquery-api`, `dcx-spanner-api`, `dcx-alloydb-api`, `dcx-cloudsql-api`, `dcx-looker-admin-api` |
| Recipe | 3 | No | `recipe-source-onboarding`, `recipe-debugging`, `recipe-quality-ops` |

```bash
# Generate API skills from Discovery Documents
dcx generate-skills --output-dir=./skills

# Gemini extension manifest
# Packaged at extensions/gemini/manifest.json (28 tools)
dcx meta gemini-tools --format=json
```

---

## Security

- **Model Armor:** `--sanitize <template>` screens responses through
  [Model Armor](https://cloud.google.com/security-command-center/docs/model-armor-overview)
  for prompt injection. Set `DCX_SANITIZE_TEMPLATE` for global default.
- **Credential encryption:** AES-256-GCM at rest, key in OS keyring.
- **Read-only API allowlists:** Dynamic commands restricted to read operations.
  Mutations excluded from MCP bridge.
- **Least-privilege defaults:** `dcx auth login` requests BigQuery-only
  scopes. `-s` *replaces* (not appends) the scope set.

---

## Profiles

Source profiles are YAML files in `~/.config/dcx/profiles/` that configure
connection details for CA and schema commands.

```yaml
# ~/.config/dcx/profiles/finance-spanner.yaml
name: finance-spanner
source_type: spanner
project: my-gcp-project
location: us-central1
instance_id: my-spanner-instance
database_id: my-database
```

Supported source types: `bigquery`, `looker`, `looker_studio`, `alloy_db`,
`spanner`, `cloud_sql`.

See [docs/source-matrix.md](docs/source-matrix.md) for the full source
compatibility matrix.

---

## CI/CD

```yaml
# GitHub Actions
- run: npm install -g dcx
- run: dcx analytics evaluate --evaluator latency --threshold 5000 --last 1h --exit-code
```

`--exit-code` returns exit code 1 on evaluation failure — GitHub Actions,
Jenkins, or any CI system treats this as a step failure.

See [docs/github-actions.md](docs/github-actions.md) for full examples.

---

## Benchmarks

Systematic benchmark suite comparing dcx against `bq` and `gcloud spanner`.

| Track | Tasks | Key Result |
|-------|-------|------------|
| [BigQuery parity](docs/benchmark_results_bigquery.md) | 12 tasks × 3 CLIs | **4.8x faster** (minified), 27% fewer tokens |
| Spanner parity | 11 | 1.3–3.9x faster on error-handling tasks |
| dcx differentiated | 8 | 7/8 pass, avg 141 ms |

See [docs/cli_benchmark_plan.md](docs/cli_benchmark_plan.md) for methodology
and [benchmarks/](benchmarks/) for task specs, runner, and raw results.

```bash
# Run benchmarks
benchmarks/scripts/run_benchmarks.sh --tasks bigquery_overlap --trials 3 --cold-trials 1

# Generate scorecard
python3 benchmarks/scripts/score_results.py benchmarks/results/raw/<run-id>
```

---

## Architecture

### Dynamic Command Generation

Like [`gws`](https://github.com/googleworkspace/cli), dcx generates commands
from bundled Google Cloud Discovery Documents at startup:

| Service | Namespace | Discovery Doc | Commands |
|---------|-----------|---------------|----------|
| BigQuery | _(top-level)_ | `bigquery/v2` | datasets, tables, routines, models |
| Spanner | `spanner` | `spanner/v1` | instances, databases, getDdl |
| AlloyDB | `alloydb` | `alloydb/v1` | clusters, instances |
| Cloud SQL | `cloudsql` | `sqladmin/v1` | instances, databases |
| Looker | `looker` | `looker/v1` | instances, backups |

Discovery Documents are pinned at build time (`include_str!`). No runtime
fetch, no network dependency. Read-only allowlists per service.

### Profile-Aware Helpers

Schema and database helpers use CA QueryData, routed by source profile:

```bash
dcx spanner schema describe --profile=spanner-finance
dcx alloydb schema describe --profile=alloydb-ops
dcx cloudsql schema describe --profile=cloudsql-app
dcx alloydb databases list --profile=alloydb-ops
```

### Testing

615 tests across 21 test binaries:

- **Unit tests:** Core parsing, auth resolution, output formatting
- **Integration tests:** Golden-file / snapshot tests against expected JSON
- **API mocking:** Record/replay via `wiremock` — no live GCP in CI
- **Contract tests:** Output-key regression, exit-code assertions
- **SDK contract CI:** Detects stale compatibility contracts; weekly sync
  opens PRs when upstream SDK changes
- **E2E:** Optional `--live` suite against a dedicated GCP project

---

## Documentation

| Doc | Description |
|-----|-------------|
| [Benchmark results (BigQuery)](docs/benchmark_results_bigquery.md) | dcx vs bq: latency, correctness, token efficiency |
| [Benchmark plan](docs/cli_benchmark_plan.md) | Methodology, scoring model, success criteria |
| [dcx vs bq](docs/dcx-vs-bq.md) | Technical comparison and workflow gallery |
| [GitHub Actions](docs/github-actions.md) | CI/CD integration examples |
| [Source matrix](docs/source-matrix.md) | CA source compatibility |
| [SDK alignment](docs/analytics_sdk_alignment_plan.md) | Analytics SDK parity plan |
| [SDK contract](docs/analytics_sdk_contract.md) | Per-flag compatibility status |
| [E2E validation](docs/e2e-validation.md) | Reproducible validation script |

---

## Status and Stability

Current version: **0.5.0** (implemented, not yet released).

| Surface | Stability | Notes |
|---------|-----------|-------|
| BigQuery read API | **Stable** | Discovery-generated, read-only. Parity-benchmarked against `bq`. |
| Spanner / AlloyDB / Cloud SQL / Looker | **Beta** | Discovery-generated, read-only. API surface may change. |
| Agent Analytics SDK | **Stable** | 12 commands, 6 evaluators. Upstream SDK contract tested weekly. |
| Conversational Analytics (`ca`) | **Beta** | 6 sources. Profile schema may evolve. |
| MCP bridge | **Beta** | JSON-RPC 2.0 / stdio. Read-only commands only. |
| Skills | **Stable** | 14 skills following [Agent Skills](https://agentskills.io) standard. |

**Not in scope:** write/admin operations, IAM management, billing.
dcx delegates those to `gcloud`.

### Comparison

| | dcx | `bq` CLI | `gcloud` |
|---|---|---|---|
| BigQuery reads | Yes | Yes | Via `bq` |
| Spanner / AlloyDB / Cloud SQL | Yes (read) | No | Yes (read + write) |
| Looker content | Yes | No | No |
| Structured JSON output | Default | `--format=json` | `--format=json` |
| MCP server | Built-in | No | No |
| Agent skills | 14 | No | No |
| Mutations | No | Yes | Yes |

### Roadmap

- v0.5.0 release (npm publish, CI badges)
- Linux CI benchmark validation
- Write-operation support for BigQuery (`jobs.insert`, `datasets.insert`)
- Additional evaluator types in Analytics SDK

See [PHASE5_PLAN.md](PHASE5_PLAN.md) and [PHASE6_PLAN.md](PHASE6_PLAN.md)
for detailed implementation history.

---

## Release

Tag `vX.Y.Z` on `main` triggers CI to build binaries, run smoke tests,
and publish npm packages.
