//! # Convex MCP Server
//!
//! Model Context Protocol (MCP) server exposing Convex's bond pricing and
//! analytics over stdio or streamable HTTP.

#![warn(missing_docs)]
#![allow(dead_code)]
#![allow(unused_imports)]

pub mod error;
pub mod server;

pub use server::ConvexMcpServer;

/// Protocol version supported by this server
pub const PROTOCOL_VERSION: &str = "2024-11-05";

/// Server name for MCP protocol
pub const SERVER_NAME: &str = "convex-mcp";

/// Server version (same as crate version)
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
