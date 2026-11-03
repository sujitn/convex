# Convex MCP Server

Model Context Protocol server for Convex fixed income analytics, built from
`crates/convex-mcp` (binary `convex-mcp-server`). The crate is not published to
crates.io; build it from this repository.

## Build

```bash
# stdio transport (Claude Desktop, Claude Code)
cargo build --release -p convex-mcp

# stdio + streamable HTTP
cargo build --release -p convex-mcp --features http
```

## Run

```bash
# stdio (default)
convex-mcp-server

# HTTP
convex-mcp-server --http --port 8080

# Explicit browser origin; repeat --allow-origin for multiple clients.
convex-mcp-server --http --port 8080 --allow-origin https://pricing.example
```

## Claude Desktop / Claude Code

```json
{
  "mcpServers": {
    "convex": {
      "command": "/path/to/convex-mcp-server"
    }
  }
}
```

See [docs/integration-guide.md](docs/integration-guide.md) for additional clients.

## Tools

| Tool | Purpose |
|------|---------|
| `create_bond` | Create a fixed rate bond. |
| `create_curve` | Create a yield curve from zero rates. |
| `bootstrap_curve` | Bootstrap a curve from deposits / swaps / OIS. |
| `price_bond` | Price a bond against a trader `Mark` (price / yield / spread). |
| `calculate_yield` | Yield to maturity from clean price. |
| `compute_spread` | Z-spread or I-spread against a curve. For a government spread, pass the benchmark securities to `library`. |
| `get_zero_rate` | Zero rate at a tenor. |
| `list_all_bonds` | List stored bonds. |
| `list_all_curves` | List stored curves. |
| `make_whole_call_price` | Make-whole call price for a callable bond with a make-whole spread. |
| `compute_position_risk` | Per-position DV01, durations, convexity and key-rate buckets. |
| `aggregate_book_risk` | Combine position risks into one book risk profile. |
| `propose_hedges` | Propose hedges for a risk profile. |
| `propose_book_hedges` | Propose hedges per group of a book, with independent constraints. |
| `compare_hedges` | Compare hedge proposals side by side and recommend one. |
| `narrate_recommendation` | Template-based trader-brief paragraph from a comparison (no LLM call). |
| `library` | Run a complete `LibraryRequest` JSON (no stored handles). |
| `library_schema` | JSON schema of `LibraryRequest`. |

## CLI

```
convex-mcp-server [OPTIONS]

Options:
      --http           Use HTTP transport instead of stdio
  -p, --port <PORT>    HTTP port [default: 8080]
      --host <HOST>    HTTP host [default: 127.0.0.1]
      --allow-origin <ORIGIN>  Exact HTTP browser origin; repeat for more
  -v, --verbose        Enable verbose logging
  -h, --help           Print help
  -V, --version        Print version
```

## Library API

The `library` tool takes the same request as the Rust `convex::library` module.
It returns the JSON envelope unchanged, with decimal strings and error
diagnostics intact. See the [library API](../../docs/LIBRARY-API.md) for the
request shapes and limits, and [removed convenience routes](../../docs/internal/ROUTE-MIGRATION.md)
for migration notes.
