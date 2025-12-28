//! Config command implementation.
//!
//! Manages CLI configuration settings.

use anyhow::Result;
use clap::{Args, Subcommand};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::cli::OutputFormat;
use crate::output::{print_header, print_info, print_success, print_warning, KeyValue};

/// Arguments for the config command.
#[derive(Args, Debug)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,
}

/// Config subcommands.
#[derive(Subcommand, Debug)]
pub enum ConfigCommand {
    /// Show current configuration
    Show,

    /// Get a configuration value
    Get(GetArgs),

    /// Set a configuration value
    Set(SetArgs),

    /// List available configuration keys
    List,

    /// Reset configuration to defaults
    Reset(ResetArgs),

    /// Show configuration file location
    Path,
}

/// Arguments for get subcommand.
#[derive(Args, Debug)]
pub struct GetArgs {
    /// Configuration key
    pub key: String,
}

/// Arguments for set subcommand.
#[derive(Args, Debug)]
pub struct SetArgs {
    /// Configuration key
    pub key: String,

    /// Configuration value
    pub value: String,
}

/// Arguments for reset subcommand.
#[derive(Args, Debug)]
pub struct ResetArgs {
    /// Reset all settings (not just one)
    #[arg(long)]
    pub all: bool,

    /// Specific key to reset (optional)
    pub key: Option<String>,
}

/// CLI configuration keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigKey {
    /// Default output format
    DefaultFormat,
    /// Default day count convention
    DefaultDayCount,
    /// Default frequency
    DefaultFrequency,
    /// Default currency
    DefaultCurrency,
    /// Default interpolation method
    DefaultInterpolation,
    /// Precision for decimal output
    DecimalPrecision,
    /// Whether to use colors
    UseColors,
}

impl ConfigKey {
    fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "default_format" | "format" => Some(Self::DefaultFormat),
            "default_daycount" | "daycount" => Some(Self::DefaultDayCount),
            "default_frequency" | "frequency" => Some(Self::DefaultFrequency),
            "default_currency" | "currency" => Some(Self::DefaultCurrency),
            "default_interpolation" | "interpolation" => Some(Self::DefaultInterpolation),
            "decimal_precision" | "precision" => Some(Self::DecimalPrecision),
            "use_colors" | "colors" => Some(Self::UseColors),
            _ => None,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::DefaultFormat => "default_format",
            Self::DefaultDayCount => "default_daycount",
            Self::DefaultFrequency => "default_frequency",
            Self::DefaultCurrency => "default_currency",
            Self::DefaultInterpolation => "default_interpolation",
            Self::DecimalPrecision => "decimal_precision",
            Self::UseColors => "use_colors",
        }
    }

    fn description(&self) -> &'static str {
        match self {
            Self::DefaultFormat => "Default output format (table, json, csv, minimal)",
            Self::DefaultDayCount => "Default day count convention (act360, act365, thirty360)",
            Self::DefaultFrequency => "Default coupon frequency (1, 2, 4, 12)",
            Self::DefaultCurrency => "Default currency (USD, EUR, GBP, etc.)",
            Self::DefaultInterpolation => "Default interpolation method (linear, monotone-convex, etc.)",
            Self::DecimalPrecision => "Number of decimal places for output (2-10)",
            Self::UseColors => "Enable colored output (true, false)",
        }
    }

    fn default_value(&self) -> &'static str {
        match self {
            Self::DefaultFormat => "table",
            Self::DefaultDayCount => "act365",
            Self::DefaultFrequency => "2",
            Self::DefaultCurrency => "USD",
            Self::DefaultInterpolation => "monotone-convex",
            Self::DecimalPrecision => "6",
            Self::UseColors => "true",
        }
    }

    fn all() -> &'static [Self] {
        &[
            Self::DefaultFormat,
            Self::DefaultDayCount,
            Self::DefaultFrequency,
            Self::DefaultCurrency,
            Self::DefaultInterpolation,
            Self::DecimalPrecision,
            Self::UseColors,
        ]
    }
}

/// Simple config storage.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct Config {
    #[serde(flatten)]
    values: HashMap<String, String>,
}

impl Config {
    fn load() -> Result<Self> {
        let path = config_path()?;
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            Ok(serde_json::from_str(&content)?)
        } else {
            Ok(Self::default())
        }
    }

    fn save(&self) -> Result<()> {
        let path = config_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    fn get(&self, key: &str) -> Option<&String> {
        self.values.get(key)
    }

    fn set(&mut self, key: String, value: String) {
        self.values.insert(key, value);
    }

    fn remove(&mut self, key: &str) {
        self.values.remove(key);
    }

    fn clear(&mut self) {
        self.values.clear();
    }
}

/// Get the config file path.
fn config_path() -> Result<PathBuf> {
    let home = dirs::config_dir()
        .or_else(dirs::home_dir)
        .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;
    Ok(home.join("convex").join("config.json"))
}

/// Execute the config command.
pub fn execute(args: ConfigArgs, format: OutputFormat) -> Result<()> {
    match args.command {
        ConfigCommand::Show => execute_show(format),
        ConfigCommand::Get(get_args) => execute_get(get_args, format),
        ConfigCommand::Set(set_args) => execute_set(set_args),
        ConfigCommand::List => execute_list(format),
        ConfigCommand::Reset(reset_args) => execute_reset(reset_args),
        ConfigCommand::Path => execute_path(),
    }
}

/// Show current configuration.
fn execute_show(format: OutputFormat) -> Result<()> {
    let config = Config::load()?;

    let mut results = Vec::new();
    for key in ConfigKey::all() {
        let value = config
            .get(key.as_str())
            .map(|s| s.as_str())
            .unwrap_or(key.default_value());
        results.push(KeyValue::new(key.as_str(), value));
    }

    match format {
        OutputFormat::Table => {
            print_header("Current Configuration");
            crate::output::print_output(&results, format)?;
        }
        OutputFormat::Json => {
            let mut output = HashMap::new();
            for key in ConfigKey::all() {
                let value = config
                    .get(key.as_str())
                    .cloned()
                    .unwrap_or_else(|| key.default_value().to_string());
                output.insert(key.as_str().to_string(), value);
            }
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        OutputFormat::Csv => {
            crate::output::print_output(&results, format)?;
        }
        OutputFormat::Minimal => {
            for key in ConfigKey::all() {
                let value = config
                    .get(key.as_str())
                    .map(|s| s.as_str())
                    .unwrap_or(key.default_value());
                println!("{}={}", key.as_str(), value);
            }
        }
    }

    Ok(())
}

/// Get a configuration value.
fn execute_get(args: GetArgs, format: OutputFormat) -> Result<()> {
    let config = Config::load()?;

    let key = ConfigKey::from_str(&args.key)
        .ok_or_else(|| anyhow::anyhow!("Unknown configuration key: {}", args.key))?;

    let value = config
        .get(key.as_str())
        .map(|s| s.as_str())
        .unwrap_or(key.default_value());

    match format {
        OutputFormat::Table | OutputFormat::Csv => {
            println!("{}: {}", key.as_str(), value);
        }
        OutputFormat::Json => {
            let output = serde_json::json!({
                "key": key.as_str(),
                "value": value
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        OutputFormat::Minimal => {
            println!("{}", value);
        }
    }

    Ok(())
}

/// Set a configuration value.
fn execute_set(args: SetArgs) -> Result<()> {
    let key = ConfigKey::from_str(&args.key)
        .ok_or_else(|| anyhow::anyhow!("Unknown configuration key: {}", args.key))?;

    // Validate value
    validate_config_value(key, &args.value)?;

    let mut config = Config::load()?;
    config.set(key.as_str().to_string(), args.value.clone());
    config.save()?;

    print_success(&format!("Set {} = {}", key.as_str(), args.value));
    Ok(())
}

/// List available configuration keys.
fn execute_list(format: OutputFormat) -> Result<()> {
    let mut results = Vec::new();
    for key in ConfigKey::all() {
        results.push(KeyValue::new(
            key.as_str(),
            format!("{} (default: {})", key.description(), key.default_value()),
        ));
    }

    match format {
        OutputFormat::Table => {
            print_header("Available Configuration Keys");
            crate::output::print_output(&results, format)?;
        }
        OutputFormat::Json => {
            let output: Vec<_> = ConfigKey::all()
                .iter()
                .map(|key| {
                    serde_json::json!({
                        "key": key.as_str(),
                        "description": key.description(),
                        "default": key.default_value()
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        OutputFormat::Csv => {
            println!("key,description,default");
            for key in ConfigKey::all() {
                println!("{},{},{}", key.as_str(), key.description(), key.default_value());
            }
        }
        OutputFormat::Minimal => {
            for key in ConfigKey::all() {
                println!("{}", key.as_str());
            }
        }
    }

    Ok(())
}

/// Reset configuration.
fn execute_reset(args: ResetArgs) -> Result<()> {
    let mut config = Config::load()?;

    if args.all {
        config.clear();
        config.save()?;
        print_success("Reset all configuration to defaults");
    } else if let Some(key_str) = args.key {
        let key = ConfigKey::from_str(&key_str)
            .ok_or_else(|| anyhow::anyhow!("Unknown configuration key: {}", key_str))?;
        config.remove(key.as_str());
        config.save()?;
        print_success(&format!("Reset {} to default ({})", key.as_str(), key.default_value()));
    } else {
        print_warning("Use --all to reset all settings, or specify a key to reset");
    }

    Ok(())
}

/// Show configuration file path.
fn execute_path() -> Result<()> {
    let path = config_path()?;
    print_info(&format!("Config file: {}", path.display()));
    if path.exists() {
        print_info("Status: exists");
    } else {
        print_info("Status: not created yet (using defaults)");
    }
    Ok(())
}

/// Validate a configuration value.
fn validate_config_value(key: ConfigKey, value: &str) -> Result<()> {
    match key {
        ConfigKey::DefaultFormat => {
            if !["table", "json", "csv", "minimal"].contains(&value.to_lowercase().as_str()) {
                return Err(anyhow::anyhow!(
                    "Invalid format: {}. Use table, json, csv, or minimal.",
                    value
                ));
            }
        }
        ConfigKey::DefaultDayCount => {
            if !["act360", "act365", "thirty360"].contains(&value.to_lowercase().as_str()) {
                return Err(anyhow::anyhow!(
                    "Invalid day count: {}. Use act360, act365, or thirty360.",
                    value
                ));
            }
        }
        ConfigKey::DefaultFrequency => {
            if !["1", "2", "4", "12"].contains(&value) {
                return Err(anyhow::anyhow!(
                    "Invalid frequency: {}. Use 1, 2, 4, or 12.",
                    value
                ));
            }
        }
        ConfigKey::DefaultCurrency => {
            if !["USD", "EUR", "GBP", "JPY", "CHF", "CAD", "AUD"]
                .contains(&value.to_uppercase().as_str())
            {
                return Err(anyhow::anyhow!(
                    "Invalid currency: {}. Use USD, EUR, GBP, JPY, CHF, CAD, or AUD.",
                    value
                ));
            }
        }
        ConfigKey::DefaultInterpolation => {
            if !["linear", "log-linear", "cubic", "monotone-convex"]
                .contains(&value.to_lowercase().as_str())
            {
                return Err(anyhow::anyhow!(
                    "Invalid interpolation: {}. Use linear, log-linear, cubic, or monotone-convex.",
                    value
                ));
            }
        }
        ConfigKey::DecimalPrecision => {
            let precision: u32 = value
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid precision: {}. Must be a number.", value))?;
            if !(2..=10).contains(&precision) {
                return Err(anyhow::anyhow!(
                    "Invalid precision: {}. Must be between 2 and 10.",
                    precision
                ));
            }
        }
        ConfigKey::UseColors => {
            if !["true", "false", "1", "0", "yes", "no"].contains(&value.to_lowercase().as_str()) {
                return Err(anyhow::anyhow!(
                    "Invalid boolean: {}. Use true or false.",
                    value
                ));
            }
        }
    }
    Ok(())
}
