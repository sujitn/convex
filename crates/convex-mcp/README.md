# Convex MCP Server

MCP (Model Context Protocol) server for Convex fixed income analytics. Enables AI assistants like Claude to perform bond pricing, yield curve analysis, and spread calculations.

## Features

- **Bond Analytics**: Create bonds, calculate YTM, duration, convexity
- **Yield Curves**: Build curves from zero rates, query rates at tenors
- **Spread Calculations**: Z-spread, I-spread, G-spread, ASW
- **Demo Mode**: Realistic December 2025 market data for testing
- **Multiple Transports**: stdio (local) and HTTP (remote)

## Quick Start

### Build

```bash
# stdio transport (for Claude Desktop)
cargo build --release -p convex-mcp

# HTTP transport (for Fly.io)
cargo build --release -p convex-mcp --features http
```

### Run

```bash
# Local with demo data
convex-mcp-server --demo

# HTTP server with demo data
convex-mcp-server --http --port 8080 --demo
```

## Integration

### Claude Desktop

Add to `claude_desktop_config.json`:
```json
{
  "mcpServers": {
    "convex": {
      "command": "/path/to/convex-mcp-server",
      "args": ["--demo"]
    }
  }
}
```

### Claude Code

Add to `.mcp.json`:
```json
{
  "mcpServers": {
    "convex": {
      "command": "convex-mcp-server",
      "args": ["--demo"]
    }
  }
}
```

See [docs/integration-guide.md](docs/integration-guide.md) for more client integrations.

## Available Tools

| Tool | Description |
|------|-------------|
| `create_bond` | Create a fixed rate bond |
| `calculate_yield` | Calculate YTM from price |
| `create_curve` | Create curve from zero rates |
| `get_zero_rate` | Query rate at tenor |
| `calculate_z_spread` | Calculate Z-spread |
| `list_all_bonds` | List stored bonds |
| `list_all_curves` | List stored curves |
| `get_market_snapshot` | Demo market overview |
| `list_demo_bonds` | List demo bonds |
| `list_demo_curves` | List demo curves |

## Demo Mode

Demo mode provides a December 2025 market snapshot:

**Bonds:**
- US Treasuries: UST.2Y, UST.5Y, UST.10Y, UST.30Y
- Corporates: AAPL.10Y, MSFT.5Y, JPM.10Y
- European: DBR.10Y (Bund), FRTR.10Y (OAT), BTPS.10Y (BTP)
- UK: UKT.10Y, UKT.30Y
- Special: Callable, Premium, Discount samples

**Curves:**
- USD.TSY - US Treasury zero curve
- USD.SOFR - SOFR swap curve
- USD.IG - Investment grade credit
- EUR.BUND - German Bund curve
- EUR.BTP - Italian BTP curve
- GBP.GILT - UK Gilt curve

## CLI Options

```
convex-mcp-server [OPTIONS]

Options:
  -d, --demo           Enable demo mode with December 2025 data
      --http           Use HTTP transport instead of stdio
  -p, --port <PORT>    HTTP port [default: 8080]
      --host <HOST>    HTTP host [default: 127.0.0.1]
  -v, --verbose        Enable verbose logging
  -h, --help           Print help
  -V, --version        Print version
```

## License

MIT
