//! Convex REST API Server.
//!
//! This crate provides a REST API for the Convex fixed income analytics library.
//!
//! ## Features
//!
//! - Bond CRUD operations
//! - Yield and price calculations
//! - Risk analytics (duration, convexity, DV01)
//! - Curve construction and bootstrapping
//! - Spread calculations (Z-spread, I-spread, G-spread)
//!
//! ## Usage
//!
//! ```bash
//! # Start server on default port
//! convex-api-server
//!
//! # Start with demo data
//! convex-api-server --demo
//!
//! # Custom host and port
//! convex-api-server --host 0.0.0.0 --port 3000 --demo
//! ```

pub mod dto;
pub mod error;
pub mod routes;
pub mod server;
pub mod state;

pub use error::{ApiError, ApiResult};
pub use server::create_router;
pub use state::AppState;
