//! Offline configuration operator; local database ownership requires a stopped server.
#![forbid(unsafe_code)]
use convex_core::contracts::*;
use convex_engine::{
    configuration::*,
    ports::{configuration::*, storage::Pagination},
};
use convex_ext_file::JsonFixingRepository;
use convex_server::{
    configuration_admin::{
        document_capabilities, effective_configuration, fixture_evidence,
        local_replay_capabilities, mode_capabilities, replay_events,
    },
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
    let mut args = std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    // `--demo` checks a document against the demo profile's composition, which
    // accepts Demo-quality inputs and results.
    let demo = args.first().is_some_and(|a| a.as_os_str() == "--demo");
    if demo {
        args.remove(0);
    }
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
    let fixtures = if validation {
        if args.len() != 5 {
            return Err(usage());
        }
        let fixtures: Vec<ConfigurationFixture> = read(&args[3])?;
        let events = replay_events(&args[4]).await?;
        fixture_evidence(&fixtures, Some(&events))?;
        fixtures
    } else {
        vec![]
    };
    let convex_ext_redb::RedbStorage {
        storage, snapshots, ..
    } = convex_ext_redb::open_redb_storage(&args[0]).map_err(|_| ConfigurationError::Storage)?;
    if command == "compile" && args.len() == 3 {
        let selection: PlanSelection = read(&args[2])?;
        let capabilities = mode_capabilities(selection.mode, vec![selection.source.clone()], demo)?;
        let document = assemble_from_repository(
            &selection,
            storage.bonds.as_ref(),
            storage.configs.as_ref(),
            storage.curves.as_ref(),
            &capabilities,
        )
        .await?;
        return Ok((json!(document), true));
    }
    // Dry-run resolve reads the database's snapshots and never publishes content.
    let cases = |capabilities| {
        ConfigurationUseCases::new(
            storage.configuration.clone(),
            storage.bonds.clone(),
            storage.configs.clone(),
            Arc::new(JsonFixingRepository::empty()),
            snapshots.clone(),
            capabilities,
        )
    };
    let inspect = || Ok::<_, ConfigurationError>(cases(local_replay_capabilities(Vec::new())?));
    match command {
        "version" if args.len() == 4 => {
            let id = SourceId::new(name(&args[2])?)
                .map_err(|_| ConfigurationError::Invalid("invalid configuration identity"))?;
            let revision: u64 = name(&args[3])?
                .parse()
                .map_err(|_| ConfigurationError::Invalid("invalid configuration version"))?;
            let staged = inspect()?
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
            let selected = inspect()?
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
            let report = cases(document_capabilities(&document, demo)?)
                .validate(&document, &fixtures)
                .await?;
            let valid = report.valid;
            Ok((json!(report), valid))
        }
        "stage" => {
            let staged: StagedConfiguration = read(&args[2])?;
            let cases = cases(document_capabilities(&staged.document, demo)?);
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
            let staged = storage
                .configuration
                .get_staged(&request.id, request.configuration_version)
                .await?
                .ok_or(ConfigurationError::Invalid("missing staged configuration"))?;
            let cases = cases(document_capabilities(&staged.document, demo)?);
            Ok((
                json!({"schema_version":1,"activation":cases.activate(&request, &fixtures).await?,"application":"restart_required"}),
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
                effective_configuration(&inspect()?, &id, as_of, known_at).await?,
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
            let page = inspect()?
                .history(&id, &Pagination::new(offset, limit))
                .await?;
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
    ConfigurationError::Invalid("usage: convex-config schema | process validate/explain [TOML] | diff OLD NEW | [--demo] DB compile SELECTION | [--demo] DB validate/explain/stage/activate/rollback JSON FIXTURES REPLAY | DB version ID VERSION | DB export-plan ID | DB effective ID AS_OF KNOWN_AT | DB history ID OFFSET LIMIT")
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
