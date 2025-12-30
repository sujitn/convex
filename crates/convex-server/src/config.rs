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
        let mut config: Self = toml::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        config.apply_env_overrides();
        Ok(config)
    }

    /// Create configuration from defaults with environment variable overrides.
    ///
    /// This is useful when no config file is provided.
    pub fn from_env() -> Self {
        let mut config = Self::default();
        config.apply_env_overrides();
        config
    }

    /// Apply environment variable overrides to the configuration.
    ///
    /// Environment variables take precedence over file-based configuration.
    /// Supported variables:
    /// - `CONVEX_HOST` - Server host (default: "0.0.0.0")
    /// - `CONVEX_PORT` - Server port (default: 8080)
    /// - `CONVEX_WEBSOCKET_ENABLED` - Enable WebSocket ("true"/"false")
    /// - `CONVEX_WEBSOCKET_PORT` - WebSocket port (if different from HTTP)
    /// - `CONVEX_STORAGE_PATH` - Storage file path
    /// - `CONVEX_QUOTES_FILE` - Market data quotes file
    /// - `CONVEX_CURVES_FILE` - Market data curves file
    /// - `CONVEX_FIXINGS_FILE` - Market data fixings file
    /// - `CONVEX_BONDS_FILE` - Reference data bonds file
    pub fn apply_env_overrides(&mut self) {
        if let Ok(host) = std::env::var("CONVEX_HOST") {
            self.host = host;
        }

        if let Ok(port) = std::env::var("CONVEX_PORT") {
            if let Ok(p) = port.parse::<u16>() {
                self.port = p;
            }
        }

        if let Ok(ws_enabled) = std::env::var("CONVEX_WEBSOCKET_ENABLED") {
            self.websocket_enabled = ws_enabled.to_lowercase() == "true" || ws_enabled == "1";
        }

        if let Ok(ws_port) = std::env::var("CONVEX_WEBSOCKET_PORT") {
            if let Ok(p) = ws_port.parse::<u16>() {
                self.websocket_port = Some(p);
            }
        }

        if let Ok(storage_path) = std::env::var("CONVEX_STORAGE_PATH") {
            self.storage_path = storage_path;
        }

        if let Ok(quotes_file) = std::env::var("CONVEX_QUOTES_FILE") {
            self.quotes_file = Some(quotes_file);
        }

        if let Ok(curves_file) = std::env::var("CONVEX_CURVES_FILE") {
            self.curves_file = Some(curves_file);
        }

        if let Ok(fixings_file) = std::env::var("CONVEX_FIXINGS_FILE") {
            self.fixings_file = Some(fixings_file);
        }

        if let Ok(bonds_file) = std::env::var("CONVEX_BONDS_FILE") {
            self.bonds_file = Some(bonds_file);
        }
    }

    /// Get all configured environment variable names.
    ///
    /// Useful for documentation and debugging.
    pub fn env_var_names() -> &'static [&'static str] {
        &[
            "CONVEX_HOST",
            "CONVEX_PORT",
            "CONVEX_WEBSOCKET_ENABLED",
            "CONVEX_WEBSOCKET_PORT",
            "CONVEX_STORAGE_PATH",
            "CONVEX_QUOTES_FILE",
            "CONVEX_CURVES_FILE",
            "CONVEX_FIXINGS_FILE",
            "CONVEX_BONDS_FILE",
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::Mutex;

    // Mutex to ensure env var tests run serially
    static ENV_TEST_MUTEX: Mutex<()> = Mutex::new(());

    // Helper to run tests with specific env vars set, then clean up
    fn with_env_vars<F, T>(vars: &[(&str, &str)], f: F) -> T
    where
        F: FnOnce() -> T,
    {
        let _guard = ENV_TEST_MUTEX.lock().unwrap();

        // Clear all CONVEX_ env vars first
        for var_name in ServerConfig::env_var_names() {
            env::remove_var(var_name);
        }

        // Set vars
        for (key, value) in vars {
            env::set_var(key, value);
        }

        let result = f();

        // Clean up
        for (key, _) in vars {
            env::remove_var(key);
        }

        result
    }

    #[test]
    fn test_default_config() {
        let config = ServerConfig::default();
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.port, 8080);
        assert!(config.websocket_enabled);
        assert!(config.websocket_port.is_none());
        assert_eq!(config.storage_path, "./data/convex.redb");
    }

    #[test]
    fn test_env_override_host() {
        with_env_vars(&[("CONVEX_HOST", "127.0.0.1")], || {
            let config = ServerConfig::from_env();
            assert_eq!(config.host, "127.0.0.1");
        });
    }

    #[test]
    fn test_env_override_port() {
        with_env_vars(&[("CONVEX_PORT", "3000")], || {
            let config = ServerConfig::from_env();
            assert_eq!(config.port, 3000);
        });
    }

    #[test]
    fn test_env_override_port_invalid() {
        with_env_vars(&[("CONVEX_PORT", "not_a_number")], || {
            let config = ServerConfig::from_env();
            // Should keep default when parsing fails
            assert_eq!(config.port, 8080);
        });
    }

    #[test]
    fn test_env_override_websocket_enabled_true() {
        with_env_vars(&[("CONVEX_WEBSOCKET_ENABLED", "true")], || {
            let config = ServerConfig::from_env();
            assert!(config.websocket_enabled);
        });
    }

    #[test]
    fn test_env_override_websocket_enabled_false() {
        with_env_vars(&[("CONVEX_WEBSOCKET_ENABLED", "false")], || {
            let config = ServerConfig::from_env();
            assert!(!config.websocket_enabled);
        });
    }

    #[test]
    fn test_env_override_websocket_enabled_numeric() {
        with_env_vars(&[("CONVEX_WEBSOCKET_ENABLED", "1")], || {
            let config = ServerConfig::from_env();
            assert!(config.websocket_enabled);
        });

        with_env_vars(&[("CONVEX_WEBSOCKET_ENABLED", "0")], || {
            let config = ServerConfig::from_env();
            assert!(!config.websocket_enabled);
        });
    }

    #[test]
    fn test_env_override_websocket_port() {
        with_env_vars(&[("CONVEX_WEBSOCKET_PORT", "8081")], || {
            let config = ServerConfig::from_env();
            assert_eq!(config.websocket_port, Some(8081));
        });
    }

    #[test]
    fn test_env_override_storage_path() {
        with_env_vars(&[("CONVEX_STORAGE_PATH", "/custom/path.redb")], || {
            let config = ServerConfig::from_env();
            assert_eq!(config.storage_path, "/custom/path.redb");
        });
    }

    #[test]
    fn test_env_override_market_data_files() {
        with_env_vars(
            &[
                ("CONVEX_QUOTES_FILE", "/data/quotes.json"),
                ("CONVEX_CURVES_FILE", "/data/curves.json"),
                ("CONVEX_FIXINGS_FILE", "/data/fixings.json"),
                ("CONVEX_BONDS_FILE", "/data/bonds.json"),
            ],
            || {
                let config = ServerConfig::from_env();
                assert_eq!(config.quotes_file, Some("/data/quotes.json".to_string()));
                assert_eq!(config.curves_file, Some("/data/curves.json".to_string()));
                assert_eq!(config.fixings_file, Some("/data/fixings.json".to_string()));
                assert_eq!(config.bonds_file, Some("/data/bonds.json".to_string()));
            },
        );
    }

    #[test]
    fn test_env_override_multiple() {
        with_env_vars(
            &[
                ("CONVEX_HOST", "localhost"),
                ("CONVEX_PORT", "9000"),
                ("CONVEX_WEBSOCKET_ENABLED", "false"),
            ],
            || {
                let config = ServerConfig::from_env();
                assert_eq!(config.host, "localhost");
                assert_eq!(config.port, 9000);
                assert!(!config.websocket_enabled);
            },
        );
    }

    #[test]
    fn test_env_var_names() {
        let names = ServerConfig::env_var_names();
        assert_eq!(names.len(), 9);
        assert!(names.contains(&"CONVEX_HOST"));
        assert!(names.contains(&"CONVEX_PORT"));
        assert!(names.contains(&"CONVEX_WEBSOCKET_ENABLED"));
        assert!(names.contains(&"CONVEX_WEBSOCKET_PORT"));
        assert!(names.contains(&"CONVEX_STORAGE_PATH"));
        assert!(names.contains(&"CONVEX_QUOTES_FILE"));
        assert!(names.contains(&"CONVEX_CURVES_FILE"));
        assert!(names.contains(&"CONVEX_FIXINGS_FILE"));
        assert!(names.contains(&"CONVEX_BONDS_FILE"));
    }
}
