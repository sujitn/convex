# Convex MCP Server Integration Guide

This guide covers how to integrate the Convex MCP Server with various AI assistants and development tools.

## Quick Start

### Building the Server

```bash
# Build with stdio transport (for local use)
cargo build --release -p convex-mcp

# Build with HTTP transport (for remote hosting)
cargo build --release -p convex-mcp --features http
```

### Running the Server

```bash
# Run with stdio transport (demo mode)
./target/release/convex-mcp-server --demo

# Run with HTTP transport (demo mode)
./target/release/convex-mcp-server --http --port 8080 --demo
```

---

## Claude Desktop

Add to your `claude_desktop_config.json`:

**Windows:**
```json
{
  "mcpServers": {
    "convex": {
      "command": "C:\\path\\to\\convex-mcp-server.exe",
      "args": ["--demo"],
      "env": {}
    }
  }
}
```

**macOS/Linux:**
```json
{
  "mcpServers": {
    "convex": {
      "command": "/path/to/convex-mcp-server",
      "args": ["--demo"],
      "env": {}
    }
  }
}
```

Config file locations:
- **Windows:** `%APPDATA%\Claude\claude_desktop_config.json`
- **macOS:** `~/Library/Application Support/Claude/claude_desktop_config.json`
- **Linux:** `~/.config/Claude/claude_desktop_config.json`

---

## Claude Code CLI

Add to your project's `.mcp.json`:

```json
{
  "mcpServers": {
    "convex": {
      "command": "./target/release/convex-mcp-server",
      "args": ["--demo"]
    }
  }
}
```

Or use globally in `~/.claude/settings.json`:
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

---

## Cursor IDE

Add to `.cursor/mcp.json` in your project:

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

---

## Cline (VS Code Extension)

In VS Code settings, add:

```json
{
  "cline.mcpServers": {
    "convex": {
      "command": "convex-mcp-server",
      "args": ["--demo"]
    }
  }
}
```

---

## Continue.dev

Add to `.continue/config.json`:

```json
{
  "mcpServers": [
    {
      "name": "convex",
      "command": "convex-mcp-server",
      "args": ["--demo"]
    }
  ]
}
```

---

## Zed Editor

Add to `~/.config/zed/mcp.json`:

```json
{
  "servers": {
    "convex": {
      "command": "convex-mcp-server",
      "args": ["--demo"]
    }
  }
}
```

---

## Available Tools

Once connected, the following tools are available:

### Bond Management
- `create_bond` - Create a fixed rate bond
- `list_all_bonds` - List all stored bonds

### Yield Calculations
- `calculate_yield` - Calculate yield to maturity from price

### Curve Management
- `create_curve` - Create a yield curve from zero rates
- `get_zero_rate` - Get zero rate at a specific tenor
- `list_all_curves` - List all stored curves

### Spread Analytics
- `calculate_z_spread` - Calculate Z-spread for a bond

### Demo Mode (when enabled)
- `get_market_snapshot` - Get December 2025 market overview
- `list_demo_bonds` - List all demo bonds with details
- `list_demo_curves` - List all demo curves with details

---

## Example Prompts

After connecting, try these prompts:

1. **View demo data:**
   > "List all the demo bonds available"

2. **Calculate yield:**
   > "Calculate the yield to maturity for UST.10Y at price 100.25"

3. **Create a bond:**
   > "Create a new 5-year bond with 5% coupon"

4. **Calculate spreads:**
   > "Calculate the Z-spread for AAPL.10Y using the USD.TSY curve"
