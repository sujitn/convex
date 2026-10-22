//! Strict process configuration. All settings require a restart.

use std::ffi::OsString;
use std::fmt;
use std::io::Read;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Explicit server mode; production is the safe default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerProfile {
    /// Requires real provider composition before the server may start.
    #[default]
    Production,
    /// Explicitly permits the existing demonstration providers.
    Demo,
    /// Explicit local historical replay; never live/execution readiness.
    Replay,
    /// Explicit local live runtime fed by `POST /api/v1/market/quotes`; never execution readiness.
    Live,
}

/// Configuration failures never include input values, file contents or paths.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// The selected configuration file cannot be read.
    #[error("CONFIG_FILE_IO: cannot read configuration ({kind:?})")]
    FileIo {
        /// Portable I/O error classification.
        kind: std::io::ErrorKind,
    },
    /// Invalid TOML, type, enum value or unknown key.
    #[error("CONFIG_FILE_PARSE: invalid TOML, field type, profile or unknown key")]
    FileParse,
    /// Configuration input exceeds the bounded process settings size.
    #[error("CONFIG_FILE_TOO_LARGE: configuration exceeds 65536 bytes")]
    FileTooLarge,
    /// A supported field has an invalid value.
    #[error("INVALID_CONFIG_VALUE: {field} requires {expected}")]
    InvalidValue {
        /// Setting name, never the submitted value.
        field: &'static str,
        /// Expected format.
        expected: &'static str,
    },
    /// An unrecognized CONVEX_ environment variable was supplied.
    #[error("UNKNOWN_CONFIG_ENVIRONMENT: {name}")]
    UnknownEnvironment {
        /// Variable name; its value is never retained.
        name: String,
    },
    /// Environment values must be valid Unicode.
    #[error("INVALID_CONFIG_ENCODING: {name} requires Unicode")]
    EnvironmentEncoding {
        /// Variable name; its value is never retained.
        name: String,
    },
    /// A setting requests behavior not implemented by the composition root.
    #[error("UNSUPPORTED_CONFIG: {field}: {reason}")]
    Unsupported {
        /// Unsupported setting name.
        field: &'static str,
        /// Missing capability.
        reason: &'static str,
    },
}

/// Server process settings. Defaults < file < environment; unknown keys fail.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    /// Authentication, authorization, browser origins and TLS ingress boundary.
    pub access: crate::access::identity::AccessSettings,
    /// Explicit demo or production profile.
    pub profile: ServerProfile,
    /// IPv4 or IPv6 bind address. Hostnames are unsupported.
    pub host: String,
    /// HTTP port; zero asks the operating system for an ephemeral port.
    pub port: u16,
    /// Enable the WebSocket routes on the HTTP listener.
    pub websocket_enabled: bool,
    /// Only the HTTP port is supported; a distinct listener is rejected.
    pub websocket_port: Option<u16>,
    /// Local redb file. Its parent directory must already exist.
    pub storage_path: String,
    /// Reserved file integration; rejected until the provider is wired.
    pub quotes_file: Option<String>,
    /// Complete bounded JSON runtime plan; required in replay mode.
    pub runtime_plan: Option<String>,
    /// Live only, instead of `runtime_plan`: plan the stored active configuration.
    pub configuration_id: Option<String>,
    /// Reserved file integration; rejected until the provider is wired.
    pub curves_file: Option<String>,
    /// Reserved file integration; rejected until the provider is wired.
    pub fixings_file: Option<String>,
    /// Reserved file integration; rejected until the provider is wired.
    pub bonds_file: Option<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            access: Default::default(),
            profile: ServerProfile::Production,
            host: "0.0.0.0".into(),
            port: 8080,
            websocket_enabled: true,
            websocket_port: None,
            storage_path: "./data/convex.redb".into(),
            quotes_file: None,
            runtime_plan: None,
            configuration_id: None,
            curves_file: None,
            fixings_file: None,
            bonds_file: None,
        }
    }
}

/// Safe startup inspection: all paths are redacted, including unsupported inputs.
#[derive(Debug, Serialize)]
pub struct EffectiveConfig<'a> {
    /// Safe access configuration summary; no secret references, URLs or paths.
    pub access: crate::access::identity::RedactedAccessSettings,
    /// Selected profile.
    pub profile: ServerProfile,
    /// Validated bind address.
    pub host: &'a str,
    /// Requested listener port; the bound address is logged separately.
    pub port: u16,
    /// Whether WebSocket routes are registered.
    pub websocket_enabled: bool,
    /// Shared listener setting.
    pub websocket_port: Option<u16>,
    /// Redacted storage path.
    pub storage_path: &'static str,
    /// Redacted optional quote source.
    pub quotes_file: Option<&'static str>,
    /// Redacted optional runtime plan location.
    pub runtime_plan: Option<&'static str>,
    /// Stored configuration the live runtime plans; an identity, not a path.
    pub configuration_id: Option<&'a str>,
    /// Redacted optional curve source.
    pub curves_file: Option<&'static str>,
    /// Redacted optional fixing source.
    pub fixings_file: Option<&'static str>,
    /// Redacted optional bond source.
    pub bonds_file: Option<&'static str>,
}

impl fmt::Debug for ServerConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.effective_redacted().fmt(formatter)
    }
}

impl ServerConfig {
    /// Load an explicit file, or the optional config/convex.toml when no path is supplied.
    /// Only an absent optional default file is allowed; other I/O failures propagate.
    pub fn load(path: Option<&Path>) -> Result<Self, ConfigError> {
        let mut config = match path {
            Some(path) => Self::read_file(path)?,
            None => match Self::read_file(Path::new("config/convex.toml")) {
                Ok(config) => config,
                Err(ConfigError::FileIo {
                    kind: std::io::ErrorKind::NotFound,
                }) => Self::default(),
                Err(error) => return Err(error),
            },
        };
        config.apply_env_overrides()?;
        Ok(config)
    }

    /// Load an explicit TOML file and apply environment overrides. Missing files fail.
    pub fn from_file(path: &str) -> Result<Self, ConfigError> {
        Self::load(Some(Path::new(path)))
    }

    /// Load defaults and environment only, without looking for a configuration file.
    pub fn from_env() -> Result<Self, ConfigError> {
        let mut config = Self::default();
        config.apply_env_overrides()?;
        Ok(config)
    }

    fn read_file(path: &Path) -> Result<Self, ConfigError> {
        const MAX_BYTES: u64 = 65536;
        let file = std::fs::File::open(path)
            .map_err(|error| ConfigError::FileIo { kind: error.kind() })?;
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| ConfigError::FileIo { kind: error.kind() })?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(ConfigError::FileTooLarge);
        }
        let content = std::str::from_utf8(&bytes).map_err(|_| ConfigError::FileParse)?;
        Self::parse_file(content)
    }

    fn parse_file(content: &str) -> Result<Self, ConfigError> {
        let config: Self = toml::from_str(content).map_err(|_| ConfigError::FileParse)?;
        config.validate()?;
        Ok(config)
    }

    /// Apply supported CONVEX_ overrides atomically. Invalid and unknown values fail.
    /// See env_var_names and the server configuration guide for the supported settings.
    pub fn apply_env_overrides(&mut self) -> Result<(), ConfigError> {
        self.apply_overrides(std::env::vars_os())
    }

    fn apply_overrides(
        &mut self,
        variables: impl IntoIterator<Item = (OsString, OsString)>,
    ) -> Result<(), ConfigError> {
        let mut candidate = self.clone();
        for (name, value) in variables {
            if !name.to_string_lossy().starts_with("CONVEX_") {
                continue;
            }
            let name = name
                .into_string()
                .map_err(|_| ConfigError::EnvironmentEncoding {
                    name: "CONVEX_ variable name".into(),
                })?;
            if !Self::env_var_names().contains(&name.as_str()) {
                return Err(ConfigError::UnknownEnvironment { name });
            }
            let value = value
                .into_string()
                .map_err(|_| ConfigError::EnvironmentEncoding { name: name.clone() })?;
            match name.as_str() {
                "CONVEX_ACCESS_MODE" => {
                    candidate.access.mode = match value.as_str() {
                        "required" => crate::access::identity::AccessMode::Required,
                        "local_development" => {
                            crate::access::identity::AccessMode::LocalDevelopment
                        }
                        _ => {
                            return Err(ConfigError::InvalidValue {
                                field: "access.mode",
                                expected: "required or local_development",
                            })
                        }
                    }
                }
                "CONVEX_ACCESS_POLICY_FILE" => candidate.access.policy_file = Some(value),
                "CONVEX_TLS_BOUNDARY" => {
                    candidate.access.tls_boundary = match value.as_str() {
                        "local_only" => crate::access::identity::TlsBoundary::LocalOnly,
                        "proxy" => crate::access::identity::TlsBoundary::Proxy,
                        _ => {
                            return Err(ConfigError::InvalidValue {
                                field: "access.tls_boundary",
                                expected: "local_only or proxy",
                            })
                        }
                    }
                }
                "CONVEX_CORS_ORIGINS" => {
                    candidate.access.cors_origins =
                        serde_json::from_str(&value).map_err(|_| ConfigError::InvalidValue {
                            field: "access.cors_origins",
                            expected: "a JSON array of exact origins",
                        })?
                }
                "CONVEX_PROFILE" => {
                    candidate.profile = match value.as_str() {
                        "production" => ServerProfile::Production,
                        "demo" => ServerProfile::Demo,
                        "replay" => ServerProfile::Replay,
                        "live" => ServerProfile::Live,
                        _ => {
                            return Err(ConfigError::InvalidValue {
                                field: "profile",
                                expected: "production, demo, replay or live",
                            })
                        }
                    }
                }
                "CONVEX_HOST" => candidate.host = value,
                "CONVEX_PORT" => candidate.port = Self::parse_port(&value, "port")?,
                "CONVEX_WEBSOCKET_ENABLED" => {
                    candidate.websocket_enabled = match value.to_ascii_lowercase().as_str() {
                        "true" | "1" => true,
                        "false" | "0" => false,
                        _ => {
                            return Err(ConfigError::InvalidValue {
                                field: "websocket_enabled",
                                expected: "true, false, 1 or 0",
                            })
                        }
                    }
                }
                "CONVEX_WEBSOCKET_PORT" => {
                    candidate.websocket_port = Some(Self::parse_port(&value, "websocket_port")?)
                }
                "CONVEX_STORAGE_PATH" => candidate.storage_path = value,
                "CONVEX_RUNTIME_PLAN" => candidate.runtime_plan = Some(value),
                "CONVEX_CONFIGURATION_ID" => candidate.configuration_id = Some(value),
                "CONVEX_QUOTES_FILE" => candidate.quotes_file = Some(value),
                "CONVEX_CURVES_FILE" => candidate.curves_file = Some(value),
                "CONVEX_FIXINGS_FILE" => candidate.fixings_file = Some(value),
                "CONVEX_BONDS_FILE" => candidate.bonds_file = Some(value),
                _ => return Err(ConfigError::UnknownEnvironment { name }),
            }
        }
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }

    fn parse_port(value: &str, field: &'static str) -> Result<u16, ConfigError> {
        value.parse().map_err(|_| ConfigError::InvalidValue {
            field,
            expected: "an integer in 0..=65535",
        })
    }

    /// Validate syntax and supported settings without creating resources.
    pub fn validate(&self) -> Result<(), ConfigError> {
        self.bind_address()?;
        self.access
            .validate()
            .map_err(|_| ConfigError::InvalidValue {
                field: "access",
                expected: "valid bounded access settings and exact origins",
            })?;
        if let Some(port) = self.websocket_port {
            if !self.websocket_enabled || port != self.port {
                return Err(ConfigError::Unsupported {
                    field: "websocket_port",
                    reason: "only enabled WebSockets on the HTTP listener are supported",
                });
            }
        }
        if self.storage_path.trim().is_empty() || self.storage_path.contains('\0') {
            return Err(ConfigError::InvalidValue {
                field: "storage_path",
                expected: "a nonempty file path without NUL",
            });
        }
        for (field, configured) in [
            (
                "quotes_file",
                self.quotes_file.is_some() && self.profile != ServerProfile::Replay,
            ),
            ("curves_file", self.curves_file.is_some()),
            ("fixings_file", self.fixings_file.is_some()),
            ("bonds_file", self.bonds_file.is_some()),
        ] {
            if configured {
                return Err(ConfigError::Unsupported {
                    field,
                    reason: "file provider composition is not implemented",
                });
            }
        }
        if self.profile == ServerProfile::Replay
            && (self.runtime_plan.is_none() || self.quotes_file.is_none())
        {
            return Err(ConfigError::InvalidValue {
                field: "replay",
                expected: "runtime_plan and canonical quotes_file",
            });
        }
        if self.profile == ServerProfile::Live
            && self.runtime_plan.is_some() == self.configuration_id.is_some()
        {
            return Err(ConfigError::InvalidValue {
                field: "live",
                expected: "exactly one of runtime_plan or configuration_id",
            });
        }
        if let Some(id) = &self.configuration_id {
            if !matches!(self.profile, ServerProfile::Live | ServerProfile::Demo) {
                return Err(ConfigError::Unsupported {
                    field: "configuration_id",
                    reason: "a stored configuration plans only the live or demo profile",
                });
            }
            if convex_core::contracts::SourceId::new(id.as_str()).is_err() {
                return Err(ConfigError::InvalidValue {
                    field: "configuration_id",
                    expected: "a stored configuration identity",
                });
            }
        }
        if matches!(self.profile, ServerProfile::Replay | ServerProfile::Live) {
            if !self.host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback()) {
                return Err(ConfigError::Unsupported {
                    field: "host",
                    reason: "this executable uses a loopback-only HTTP upstream",
                });
            }
        } else if self.runtime_plan.is_some() {
            return Err(ConfigError::Unsupported {
                field: "runtime_plan",
                reason: "runtime plan requires the explicit replay or live profile",
            });
        }
        Ok(())
    }

    /// Refuse the current placeholder composition in production, before opening storage.
    pub fn validate_startup(&self) -> Result<(), ConfigError> {
        self.validate()?;
        if self.profile == ServerProfile::Production {
            return Err(ConfigError::Unsupported { field: "profile", reason: "live production provider composition is not implemented; demo or local replay requires explicit opt-in" });
        }
        if !self.bind_address()?.ip().is_loopback() {
            return Err(ConfigError::Unsupported {
                field: "host",
                reason:
                    "plain HTTP must bind loopback; remote ingress requires an external TLS proxy",
            });
        }
        if self.access.mode == crate::access::identity::AccessMode::Required
            && (self.access.policy_file.is_none() || self.access.introspection.is_none())
        {
            return Err(ConfigError::InvalidValue {
                field: "access",
                expected: "policy_file and introspection settings, or explicit local_development",
            });
        }
        if self
            .access
            .introspection
            .as_ref()
            .is_some_and(|i| i.allow_http_loopback)
            && self.access.tls_boundary != crate::access::identity::TlsBoundary::LocalOnly
        {
            return Err(ConfigError::Unsupported {
                field: "access.introspection",
                reason: "HTTP identity fixtures require the local_only TLS boundary",
            });
        }
        Ok(())
    }

    /// Resolve the exact listener address; never fall back to a different interface.
    pub fn bind_address(&self) -> Result<SocketAddr, ConfigError> {
        let host = self
            .host
            .parse::<IpAddr>()
            .map_err(|_| ConfigError::InvalidValue {
                field: "host",
                expected: "an IPv4 or IPv6 literal",
            })?;
        Ok(SocketAddr::new(host, self.port))
    }

    /// Return an inspection view that cannot expose configured paths.
    pub fn effective_redacted(&self) -> EffectiveConfig<'_> {
        const REDACTED: &str = "[REDACTED]";
        EffectiveConfig {
            access: self.access.redacted(),
            profile: self.profile,
            // Debug on an unvalidated config must not expose an arbitrary input value.
            host: if self.host.parse::<IpAddr>().is_ok() {
                &self.host
            } else {
                REDACTED
            },
            port: self.port,
            websocket_enabled: self.websocket_enabled,
            websocket_port: self.websocket_port,
            storage_path: REDACTED,
            quotes_file: self.quotes_file.as_ref().map(|_| REDACTED),
            runtime_plan: self.runtime_plan.as_ref().map(|_| REDACTED),
            // Shown only once valid, like `host`.
            configuration_id: self
                .configuration_id
                .as_deref()
                .filter(|id| convex_core::contracts::SourceId::new(*id).is_ok()),
            curves_file: self.curves_file.as_ref().map(|_| REDACTED),
            fixings_file: self.fixings_file.as_ref().map(|_| REDACTED),
            bonds_file: self.bonds_file.as_ref().map(|_| REDACTED),
        }
    }

    /// Supported environment names; unknown CONVEX_ variables are rejected.
    pub fn env_var_names() -> &'static [&'static str] {
        &[
            "CONVEX_PROFILE",
            "CONVEX_ACCESS_MODE",
            "CONVEX_ACCESS_POLICY_FILE",
            "CONVEX_TLS_BOUNDARY",
            "CONVEX_CORS_ORIGINS",
            "CONVEX_HOST",
            "CONVEX_PORT",
            "CONVEX_WEBSOCKET_ENABLED",
            "CONVEX_WEBSOCKET_PORT",
            "CONVEX_STORAGE_PATH",
            "CONVEX_QUOTES_FILE",
            "CONVEX_RUNTIME_PLAN",
            "CONVEX_CURVES_FILE",
            "CONVEX_FIXINGS_FILE",
            "CONVEX_BONDS_FILE",
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overrides(config: &mut ServerConfig, values: &[(&str, &str)]) -> Result<(), ConfigError> {
        config.apply_overrides(
            values
                .iter()
                .map(|(key, value)| (OsString::from(key), OsString::from(value))),
        )
    }

    #[test]
    fn defaults_are_production_and_file_keys_are_strict() {
        let config = ServerConfig::parse_file("").unwrap();
        assert_eq!(config.profile, ServerProfile::Production);
        assert_eq!(config.port, 8080);
        assert!(matches!(
            ServerConfig::parse_file("unknown = 'secret'"),
            Err(ConfigError::FileParse)
        ));
        assert!(matches!(
            ServerConfig::parse_file("port = -1"),
            Err(ConfigError::FileParse)
        ));
        assert!(matches!(
            ServerConfig::parse_file("profile = 'unknown'"),
            Err(ConfigError::FileParse)
        ));
    }

    #[test]
    fn environment_precedence_is_deterministic_without_global_mutation() {
        let mut config = ServerConfig::parse_file(
            "profile='production'\nhost='::1'\nport=9000\nwebsocket_enabled=true",
        )
        .unwrap();
        overrides(
            &mut config,
            &[
                ("CONVEX_PROFILE", "demo"),
                ("CONVEX_ACCESS_MODE", "local_development"),
                ("CONVEX_HOST", "127.0.0.1"),
                ("CONVEX_PORT", "0"),
                ("CONVEX_WEBSOCKET_ENABLED", "false"),
                ("CONVEX_STORAGE_PATH", "/custom/store"),
            ],
        )
        .unwrap();
        assert_eq!(config.profile, ServerProfile::Demo);
        assert_eq!(
            config.bind_address().unwrap(),
            "127.0.0.1:0".parse().unwrap()
        );
        assert!(!config.websocket_enabled);
        assert_eq!(config.storage_path, "/custom/store");
        config.validate_startup().unwrap();
    }

    #[test]
    fn live_profile_needs_one_plan_source_loopback_and_no_quotes_file() {
        let mut config = ServerConfig::parse_file("").unwrap();
        overrides(
            &mut config,
            &[
                ("CONVEX_PROFILE", "live"),
                ("CONVEX_ACCESS_MODE", "local_development"),
                ("CONVEX_HOST", "127.0.0.1"),
                ("CONVEX_RUNTIME_PLAN", "/plan.json"),
            ],
        )
        .unwrap();
        assert_eq!(config.profile, ServerProfile::Live);
        config.validate_startup().unwrap();
        config.runtime_plan = None;
        assert!(matches!(
            config.validate(),
            Err(ConfigError::InvalidValue { field: "live", .. })
        ));
        config.configuration_id = Some("SHOWCASE-USD".into());
        config.validate_startup().unwrap();
        config.runtime_plan = Some("/plan.json".into());
        assert!(matches!(
            config.validate(),
            Err(ConfigError::InvalidValue { field: "live", .. })
        ));
        config.configuration_id = None;
        config.quotes_file = Some("/quotes.jsonl".into());
        assert!(matches!(
            config.validate(),
            Err(ConfigError::Unsupported {
                field: "quotes_file",
                ..
            })
        ));
        config.quotes_file = None;
        config.host = "0.0.0.0".into();
        assert!(matches!(
            config.validate(),
            Err(ConfigError::Unsupported { field: "host", .. })
        ));
        config.host = "127.0.0.1".into();
        // The demo profile may run a stored configuration live; it never takes a plan file.
        config.profile = ServerProfile::Demo;
        config.runtime_plan = None;
        config.configuration_id = Some("SHOWCASE-USD".into());
        config.validate_startup().unwrap();
        config.runtime_plan = Some("/plan.json".into());
        assert!(matches!(
            config.validate(),
            Err(ConfigError::Unsupported {
                field: "runtime_plan",
                ..
            })
        ));
        config.runtime_plan = None;
        config.profile = ServerProfile::Replay;
        config.quotes_file = Some("/quotes.jsonl".into());
        config.runtime_plan = Some("/plan.json".into());
        assert!(matches!(
            config.validate(),
            Err(ConfigError::Unsupported {
                field: "configuration_id",
                ..
            })
        ));
    }

    #[test]
    fn invalid_overrides_are_typed_and_never_partially_apply() {
        for (name, value, field) in [
            ("CONVEX_PORT", "bad", "port"),
            ("CONVEX_PORT", "-1", "port"),
            ("CONVEX_PORT", "65536", "port"),
            ("CONVEX_HOST", "localhost", "host"),
            ("CONVEX_PROFILE", "", "profile"),
            ("CONVEX_WEBSOCKET_ENABLED", "yes", "websocket_enabled"),
            ("CONVEX_STORAGE_PATH", " ", "storage_path"),
        ] {
            let mut config = ServerConfig::default();
            let result = overrides(&mut config, &[("CONVEX_HOST", "::1"), (name, value)]);
            assert!(
                matches!(result, Err(ConfigError::InvalidValue { field: actual, .. }) if actual == field)
            );
            assert_eq!(config.host, "0.0.0.0");
        }
    }

    #[test]
    fn unsupported_settings_and_unknown_environment_are_typed() {
        for name in [
            "CONVEX_QUOTES_FILE",
            "CONVEX_CURVES_FILE",
            "CONVEX_FIXINGS_FILE",
            "CONVEX_BONDS_FILE",
            "CONVEX_WEBSOCKET_PORT",
        ] {
            assert!(matches!(
                overrides(&mut ServerConfig::default(), &[(name, "9000")]),
                Err(ConfigError::Unsupported { .. })
            ));
        }
        assert!(matches!(
            overrides(&mut ServerConfig::default(), &[("CONVEX_SECRET", "secret")]),
            Err(ConfigError::UnknownEnvironment { .. })
        ));
        assert!(matches!(
            ServerConfig::default().validate_startup(),
            Err(ConfigError::Unsupported {
                field: "profile",
                ..
            })
        ));
    }

    #[test]
    fn shared_websocket_port_and_all_supported_booleans_work() {
        for (value, enabled) in [
            ("true", true),
            ("TRUE", true),
            ("1", true),
            ("false", false),
            ("0", false),
        ] {
            let mut config = ServerConfig::default();
            overrides(&mut config, &[("CONVEX_WEBSOCKET_ENABLED", value)]).unwrap();
            assert_eq!(config.websocket_enabled, enabled);
        }
        overrides(
            &mut ServerConfig::default(),
            &[("CONVEX_WEBSOCKET_PORT", "8080")],
        )
        .unwrap();
        let result = overrides(
            &mut ServerConfig::default(),
            &[
                ("CONVEX_WEBSOCKET_ENABLED", "false"),
                ("CONVEX_WEBSOCKET_PORT", "8080"),
            ],
        );
        assert!(matches!(
            result,
            Err(ConfigError::Unsupported {
                field: "websocket_port",
                ..
            })
        ));
    }

    #[test]
    fn ipv6_and_maximum_port_are_valid() {
        let mut config = ServerConfig::default();
        overrides(
            &mut config,
            &[("CONVEX_HOST", "::1"), ("CONVEX_PORT", "65535")],
        )
        .unwrap();
        assert_eq!(
            config.bind_address().unwrap(),
            "[::1]:65535".parse().unwrap()
        );
    }

    #[test]
    fn inspection_debug_and_errors_never_expose_values() {
        let config = ServerConfig {
            host: "secret-marker".into(),
            storage_path: "secret-marker".into(),
            quotes_file: Some("secret-marker".into()),
            ..ServerConfig::default()
        };
        let view = serde_json::to_string(&config.effective_redacted()).unwrap();
        assert!(!view.contains("secret-marker"));
        assert!(!format!("{config:?}").contains("secret-marker"));
        let error = config.validate().unwrap_err();
        assert!(!error.to_string().contains("secret-marker"));
        assert!(!format!("{error:?}").contains("secret-marker"));
    }

    #[cfg(unix)]
    #[test]
    fn non_unicode_environment_values_are_rejected() {
        use std::os::unix::ffi::OsStringExt;
        let result = ServerConfig::default()
            .apply_overrides([(OsString::from("CONVEX_HOST"), OsString::from_vec(vec![255]))]);
        assert!(matches!(
            result,
            Err(ConfigError::EnvironmentEncoding { .. })
        ));
    }
}
