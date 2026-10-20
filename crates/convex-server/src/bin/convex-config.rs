//! Offline configuration operator; local database ownership requires a stopped server.
#![forbid(unsafe_code)]
use convex_core::contracts::*;
use convex_engine::{
    configuration::*,
    ports::{
        configuration::*,
        market_data::{QuoteEventSource, StreamError},
        quote_event::QuoteEvent,
        storage::Pagination,
    },
};
use convex_ext_file::{FileQuoteReplay, FileSnapshotRepository, JsonFixingRepository};
use convex_server::{
    configuration_admin::{effective_configuration, local_replay_capabilities},
    ServerConfig,
};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::Arc,
};

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok((value, valid)) => match serde_json::to_string_pretty(&value) {
            Ok(output) if output.len() <= 4 * 1024 * 1024 => {
                println!("{output}");
                if valid {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            }
            _ => {
                eprintln!("CONFIGURATION_CAPACITY_EXCEEDED");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
async fn run() -> Result<(Value, bool), ConfigurationError> {
    let args = std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    let Some(first) = args.first().and_then(|p| p.to_str()) else {
        return Err(usage());
    };
    if first == "schema" && args.len() == 1 {
        return Ok((
            convex_engine::ports::configuration_schema::configuration_schema(),
            true,
        ));
    }
    if first == "process" && (args.len() == 2 || args.len() == 3) {
        let command = name(&args[1])?;
        if !matches!(command, "validate" | "explain") {
            return Err(usage());
        }
        let config = ServerConfig::load(args.get(2).map(PathBuf::as_path)).map_err(|_| {
            ConfigurationError::Invalid("invalid process settings; values redacted")
        })?;
        let ready = config.validate_startup().is_ok();
        return Ok((
            json!({"schema_version":1,"syntax_valid":true,"startup_supported":ready,"configuration":config.effective_redacted()}),
            command == "explain" || ready,
        ));
    }
    if first == "diff" && args.len() == 3 {
        let previous: ConfigurationDocument = read(&args[1])?;
        let next: ConfigurationDocument = read(&args[2])?;
        previous.validate()?;
        next.validate()?;
        return Ok((
            json!({"schema_version":1,"changes":configuration_diff(&previous,&next),"previous":previous.redacted()?,"next":next.redacted()?}),
            true,
        ));
    }
    if args.len() < 3 {
        return Err(usage());
    }
    let command = name(&args[1])?;
    let validation = matches!(
        command,
        "validate" | "explain" | "stage" | "activate" | "rollback"
    );
    let (fixtures, events) = if validation {
        if args.len() != 5 {
            return Err(usage());
        }
        let fixtures: Vec<ConfigurationFixture> = read(&args[3])?;
        if fixtures.is_empty() || fixtures.len() > 8 {
            return Err(ConfigurationError::Invalid(
                "one to eight fixture cases required",
            ));
        }
        let events = replay(&args[4]).await?;
        if fixtures
            .iter()
            .any(|f| f.events.len() > events.len() || f.events != events[..f.events.len()])
        {
            return Err(ConfigurationError::Invalid(
                "fixture trace differs from actual canonical replay prefix",
            ));
        }
        (fixtures, events)
    } else {
        (vec![], vec![])
    };
    let sources = events
        .iter()
        .map(|e| {
            SourceId::new(&e.key.source)
                .map_err(|_| ConfigurationError::Invalid("invalid replay provider identity"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let storage =
        convex_ext_redb::create_redb_storage(&args[0]).map_err(|_| ConfigurationError::Storage)?;
    // Construction checks the existing snapshot directory; dry-run resolve never publishes content.
    let snapshots = Arc::new(
        FileSnapshotRepository::new(PathBuf::from(format!("{}.snapshots", name(&args[0])?)))
            .map_err(|_| ConfigurationError::Storage)?,
    );
    let cases = ConfigurationUseCases::new(
        storage.configuration,
        storage.bonds,
        storage.configs,
        Arc::new(JsonFixingRepository::empty()),
        snapshots,
        local_replay_capabilities(sources)?,
    );
    match command {
        "version" if args.len() == 4 => {
            let id = SourceId::new(name(&args[2])?)
                .map_err(|_| ConfigurationError::Invalid("invalid configuration identity"))?;
            let revision: u64 = name(&args[3])?
                .parse()
                .map_err(|_| ConfigurationError::Invalid("invalid configuration version"))?;
            let staged = cases
                .get_version(
                    &id,
                    Version::new(revision).map_err(|_| {
                        ConfigurationError::Invalid("invalid configuration version")
                    })?,
                )
                .await?
                .ok_or(ConfigurationError::Invalid("missing staged configuration"))?;
            Ok((
                json!({"schema_version":1,"configuration":staged.document.redacted()?,"digest":staged.document.digest()?,"actor":staged.actor,"reason":staged.reason,"staged_at":staged.staged_at,"idempotency_key":staged.idempotency_key}),
                true,
            ))
        }
        "export-plan" if args.len() == 3 => {
            let id = SourceId::new(name(&args[2])?)
                .map_err(|_| ConfigurationError::Invalid("invalid configuration identity"))?;
            let selected = cases
                .active(&id)
                .await?
                .ok_or(ConfigurationError::Invalid("no active configuration"))?;
            Ok((
                json!(convex_engine::configuration::plan(
                    &selected.staged.document,
                    vec![]
                )),
                true,
            ))
        }
        "validate" | "explain" => {
            let document: ConfigurationDocument = read(&args[2])?;
            let report = cases.validate(&document, &fixtures).await?;
            let valid = report.valid;
            Ok((json!(report), valid))
        }
        "stage" => {
            let staged: StagedConfiguration = read(&args[2])?;
            Ok((json!(cases.stage(&staged, &fixtures).await?), true))
        }
        "activate" | "rollback" => {
            let request: ActivationCommand = read(&args[2])?;
            if (command == "rollback") != matches!(request.action, ActivationKind::Rollback { .. })
            {
                return Err(ConfigurationError::Invalid(
                    "command and activation action differ",
                ));
            }
            Ok((
                json!({"schema_version":1,"activation":cases.activate(&request, &fixtures, false).await?,"application":"restart_required"}),
                true,
            ))
        }
        "effective" if args.len() == 5 => {
            let id = SourceId::new(name(&args[2])?)
                .map_err(|_| ConfigurationError::Invalid("invalid configuration identity"))?;
            let as_of = UtcTimestamp::parse(name(&args[3])?)
                .map_err(|_| ConfigurationError::Invalid("invalid economic time"))?;
            let known_at = UtcTimestamp::parse(name(&args[4])?)
                .map_err(|_| ConfigurationError::Invalid("invalid information cutoff"))?;
            Ok((
                effective_configuration(&cases, &id, as_of, known_at).await?,
                true,
            ))
        }
        "history" if args.len() == 5 => {
            let id = SourceId::new(name(&args[2])?)
                .map_err(|_| ConfigurationError::Invalid("invalid configuration identity"))?;
            let offset = name(&args[3])?
                .parse()
                .map_err(|_| ConfigurationError::Invalid("invalid history offset"))?;
            let limit = name(&args[4])?
                .parse()
                .map_err(|_| ConfigurationError::Invalid("invalid history limit"))?;
            let page = cases.history(&id, &Pagination::new(offset, limit)).await?;
            Ok((
                json!({"schema_version":1,"items":page.items,"total":page.total,"offset":page.offset,"limit":page.limit}),
                true,
            ))
        }
        _ => Err(usage()),
    }
}
fn name(value: &Path) -> Result<&str, ConfigurationError> {
    value
        .to_str()
        .ok_or(ConfigurationError::Invalid("argument encoding is invalid"))
}
fn usage() -> ConfigurationError {
    ConfigurationError::Invalid("usage: convex-config schema | process validate/explain [TOML] | diff OLD NEW | DB validate/explain/stage/activate/rollback JSON FIXTURES REPLAY | DB version ID VERSION | DB export-plan ID | DB effective ID AS_OF KNOWN_AT | DB history ID OFFSET LIMIT")
}
fn read<T: DeserializeOwned>(path: &Path) -> Result<T, ConfigurationError> {
    let file = std::fs::File::open(path)
        .map_err(|_| ConfigurationError::Invalid("input file is unavailable; path redacted"))?;
    let mut bytes = Vec::new();
    file.take((MAX_CONFIGURATION_BYTES + 16384) as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ConfigurationError::Invalid("input file read failed; path redacted"))?;
    if bytes.len() > MAX_CONFIGURATION_BYTES + 16384 {
        return Err(ConfigurationError::CapacityExceeded);
    }
    serde_json::from_slice(&bytes).map_err(|_| {
        ConfigurationError::Invalid("input JSON violates the strict schema; values redacted")
    })
}
async fn replay(path: &Path) -> Result<Vec<QuoteEvent>, ConfigurationError> {
    let mut source = FileQuoteReplay::open(path).map_err(|_| {
        ConfigurationError::Invalid("canonical replay file is unavailable; path redacted")
    })?;
    let mut events = vec![];
    loop {
        match source.next_event().await {
            Ok(event) => {
                if events.len() == 4096 {
                    return Err(ConfigurationError::CapacityExceeded);
                }
                events.push(event);
            }
            Err(StreamError::Closed) => return Ok(events),
            Err(_) => {
                return Err(ConfigurationError::Invalid(
                    "canonical replay failed; provider details redacted",
                ))
            }
        }
    }
}
