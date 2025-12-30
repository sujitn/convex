//! Server configuration.

use serde::{Deserialize, Serialize};

/// Server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Host to bind to
    #[serde(default = "default_host")]
    pub host: String,

    /// Port to listen on
    #[serde(default = "default_port")]
    pub port: u16,

    /// Enable WebSocket
    #[serde(default = "default_true")]
    pub websocket_enabled: bool,

    /// WebSocket port (if different from HTTP)
    pub websocket_port: Option<u16>,

    /// Storage path
    #[serde(default = "default_storage_path")]
    pub storage_path: String,

    /// Market data quotes file
    pub quotes_file: Option<String>,

    /// Market data curves file
    pub curves_file: Option<String>,

    /// Market data fixings file
    pub fixings_file: Option<String>,

    /// Reference data bonds file
    pub bonds_file: Option<String>,
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    8080
}

fn default_true() -> bool {
    true
}

fn default_storage_path() -> String {
    "./data/convex.redb".to_string()
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            websocket_enabled: true,
            websocket_port: None,
            storage_path: default_storage_path(),
            quotes_file: None,
            curves_file: None,
            fixings_file: None,
            bonds_file: None,
        }
    }
}

impl ServerConfig {
    /// Load configuration from a TOML file.
    pub fn from_file(path: &str) -> Result<Self, std::io::Error> {
        let content = std::fs::read_to_string(path)?;
        toml::from_str(&content).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}
