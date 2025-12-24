//! # Convex MCP Server
//!
//! Model Context Protocol (MCP) server for Convex fixed income analytics.
//!
//! This crate exposes Convex's bond pricing and analytics capabilities
//! through the MCP protocol, enabling integration with AI assistants like
//! Claude Desktop, Claude Code, Cursor, and other MCP-compatible clients.
//!
//! ## Features
//!
//! - **Bond Analytics**: YTM, duration, convexity, DV01, spreads
//! - **Curve Building**: Bootstrapping, interpolation, scenario analysis
//! - **Demo Mode**: Realistic December 2025 market data for testing
//! - **Multiple Transports**: stdio (local) and HTTP (remote)
//!
//! ## Quick Start
//!
//! ```bash
//! # Run with stdio transport (for Claude Desktop)
//! convex-mcp-server
//!
//! # Run with HTTP transport (for remote hosting)
//! convex-mcp-server --http --port 8080
//!
//! # Run in demo mode with sample data
//! convex-mcp-server --demo
//! ```

#![warn(missing_docs)]
#![allow(dead_code)]
#![allow(unused_imports)]

pub mod demo;
pub mod server;

pub use server::ConvexMcpServer;

/// Protocol version supported by this server
pub const PROTOCOL_VERSION: &str = "2024-11-05";

/// Server name for MCP protocol
pub const SERVER_NAME: &str = "convex-mcp";

/// Server version (same as crate version)
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
