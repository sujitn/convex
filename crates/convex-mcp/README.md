# Convex MCP Server

Model Context Protocol server for Convex fixed income analytics.

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
| `compute_spread` | Z-spread, I-spread, or G-spread against a curve. |
| `get_zero_rate` | Zero rate at a tenor. |
| `list_all_bonds` | List stored bonds. |
| `list_all_curves` | List stored curves. |

## CLI

```
convex-mcp-server [OPTIONS]

Options:
      --http           Use HTTP transport instead of stdio
  -p, --port <PORT>    HTTP port [default: 8080]
      --host <HOST>    HTTP host [default: 127.0.0.1]
  -v, --verbose        Enable verbose logging
  -h, --help           Print help
  -V, --version        Print version
```
