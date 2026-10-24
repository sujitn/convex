//! Bounded, strict reference imports with retained row/field diagnostics.

#![forbid(unsafe_code)]

use convex_core::contracts::{SchemaVersion, UtcTimestamp};
use convex_ports::reference_repository::*;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;

/// Maximum bytes in a reference import, including concurrent file growth.
pub const MAX_REFERENCE_IMPORT_BYTES: u64 = 8 * 1024 * 1024;

/// Strict versioned file envelope. Operations are explicit creates/expected-version updates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceImportDocument {
    /// Supported reference wire version.
    pub schema_version: SchemaVersion,
    /// One bounded atomic transaction.
    pub command: ReferenceCommand,
}

/// Diagnostic identifying the rejected location without copying source values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReferenceImportDiagnostic {
    /// file, changes, master_changes or transaction.
    pub section: String,
    /// Zero-based operation index, where decoding/validation can identify it.
    pub row: Option<usize>,
    /// Stable field or stage name.
    pub field: String,
    /// Stable error category.
    pub code: String,
    /// Bounded semantic detail, never a raw provider value.
    pub detail: String,
    /// Decoder line where applicable, one-based.
    pub line: Option<usize>,
    /// Decoder column where applicable, one-based.
    pub column: Option<usize>,
}

/// Rejected import. No partial transaction is acknowledged.
#[derive(Debug, thiserror::Error)]
#[error("reference import rejected: {error}")]
pub struct ReferenceImportError {
    /// Original backend/validation category; unknown outcome still requires retry lookup.
    pub error: ReferenceError,
    /// Bounded validation/conflict evidence.
    pub diagnostics: Vec<ReferenceImportDiagnostic>,
}

fn diagnostic(
    section: &str,
    row: Option<usize>,
    error: &ReferenceError,
) -> ReferenceImportDiagnostic {
    let (code, detail) = match error {
        ReferenceError::InvalidInput(detail) => ("INVALID_INPUT", *detail),
        ReferenceError::Conflict(detail) => ("VERSION_CONFLICT", *detail),
        ReferenceError::AmbiguousIdentifier => {
            ("AMBIGUOUS_IDENTIFIER", "identifier has multiple assertions")
        }
        ReferenceError::CapacityExceeded => (
            "CAPACITY_EXCEEDED",
            "declared import/repository bound exceeded",
        ),
        ReferenceError::UnknownOutcome => (
            "UNKNOWN_OUTCOME",
            "inspect the idempotency key before retrying",
        ),
        ReferenceError::NotFound => ("MISSING_REFERENCE", "expected reference record is absent"),
        ReferenceError::Unavailable => ("UNAVAILABLE", "reference input/backend unavailable"),
        ReferenceError::Unsupported(detail) => ("UNSUPPORTED", *detail),
    };
    let field = if detail.contains("day-count") {
        "day_count"
    } else if detail.contains("stub")
        || detail.contains("coupon period")
        || detail.contains("coupon boundaries")
    {
        "stub"
    } else if detail.contains("payment lag") {
        "payment_lag"
    } else if detail.contains("coupon") {
        "coupon_rate"
    } else if detail.contains("contractual") {
        "contractual_terms"
    } else if detail.contains("pinned") || detail.contains("resource") {
        "reference_pins"
    } else if detail.contains("version") {
        "expected_version"
    } else {
        "record"
    };
    ReferenceImportDiagnostic {
        section: section.into(),
        row,
        field: field.into(),
        code: code.into(),
        detail: detail.into(),
        line: None,
        column: None,
    }
}

fn failure(error: ReferenceError) -> ReferenceImportError {
    let diagnostics = vec![diagnostic("file", None, &error)];
    ReferenceImportError { error, diagnostics }
}

pub(crate) fn read_reference_file(path: &Path) -> Result<Vec<u8>, ReferenceError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| ReferenceError::Unavailable)?;
    if !metadata.is_file() {
        return Err(ReferenceError::InvalidInput(
            "reference import must be a regular file",
        ));
    }
    if metadata.len() > MAX_REFERENCE_IMPORT_BYTES {
        return Err(ReferenceError::CapacityExceeded);
    }
    let file = std::fs::File::open(path).map_err(|_| ReferenceError::Unavailable)?;
    if !file
        .metadata()
        .map_err(|_| ReferenceError::Unavailable)?
        .is_file()
    {
        return Err(ReferenceError::InvalidInput(
            "reference import must be a regular file",
        ));
    }
    let mut bytes = Vec::new();
    file.take(MAX_REFERENCE_IMPORT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ReferenceError::Unavailable)?;
    if bytes.len() as u64 > MAX_REFERENCE_IMPORT_BYTES {
        return Err(ReferenceError::CapacityExceeded);
    }
    Ok(bytes)
}

/// Decode the entire typed envelope, reject unknown/duplicate fields, and report bad rows.
/// File loading is offline/blocking I/O; it is not part of the quote hot path.
pub fn load_reference_import(
    path: impl AsRef<Path>,
) -> Result<ReferenceImportDocument, ReferenceImportError> {
    let bytes = read_reference_file(path.as_ref()).map_err(failure)?;
    let document: ReferenceImportDocument = serde_json::from_slice(&bytes).map_err(|error| {
        let mut failed = failure(ReferenceError::InvalidInput(
            "invalid strict reference import JSON",
        ));
        failed.diagnostics[0].line = Some(error.line());
        failed.diagnostics[0].column = Some(error.column());
        failed
    })?;
    let mut diagnostics = Vec::new();
    // Validate the total bound before traversing any operations.
    let total = document
        .command
        .changes
        .len()
        .saturating_add(document.command.master_changes.len());
    if total == 0 || total > MAX_REFERENCE_CHANGES {
        return Err(failure(ReferenceError::InvalidInput(
            "expected 1..256 import changes",
        )));
    }
    for (row, change) in document.command.changes.iter().enumerate() {
        let result = match change {
            ReferenceChange::Create { bond } | ReferenceChange::Update { bond, .. } => {
                validate_bond_reference(bond)
            }
            ReferenceChange::Delete { instrument_id, .. } => validate_reference_id(instrument_id),
        };
        if let Err(error) = result {
            diagnostics.push(diagnostic("changes", Some(row), &error));
        }
    }
    for (row, change) in document.command.master_changes.iter().enumerate() {
        if let Err(error) = change.definition.validate() {
            diagnostics.push(diagnostic("master_changes", Some(row), &error));
        }
    }
    if diagnostics.is_empty() {
        if let Err(error) = document.command.validate() {
            diagnostics.push(diagnostic("transaction", None, &error));
        }
    }
    if !diagnostics.is_empty() {
        return Err(ReferenceImportError {
            error: ReferenceError::InvalidInput("reference import validation failed"),
            diagnostics,
        });
    }
    Ok(document)
}

/// Apply a fully decoded import through the sole authoritative atomic reference port.
/// A conflict cannot commit a valid prefix. A lost reply remains UnknownOutcome.
pub async fn import_reference_file(
    repository: &dyn ReferenceRepository,
    path: impl AsRef<Path>,
    recorded_at: UtcTimestamp,
) -> Result<ReferenceCommit, ReferenceImportError> {
    let document = load_reference_import(path)?;
    // Retained retries are compared by the backend before looking at current versions.
    // Preflight is diagnostic only; the final transaction still performs compare-and-set.
    if repository
        .get_commit(&document.command.idempotency_key)
        .await
        .map_err(failure)?
        .is_none()
    {
        let mut diagnostics = Vec::new();
        for (row, change) in document.command.changes.iter().enumerate() {
            let current = repository
                .get(change.instrument_id())
                .await
                .map_err(failure)?;
            let error = match (change, current) {
                (ReferenceChange::Create { .. }, None) => None,
                (ReferenceChange::Create { .. }, Some(_)) => Some(ReferenceError::Conflict(
                    "instrument already exists or was deleted",
                )),
                (
                    ReferenceChange::Update {
                        expected_version, ..
                    }
                    | ReferenceChange::Delete {
                        expected_version, ..
                    },
                    Some(current),
                ) if current.bond.is_some() && *expected_version == current.version => None,
                _ => Some(ReferenceError::Conflict(
                    "expected version differs from active reference version",
                )),
            };
            if let Some(error) = error {
                diagnostics.push(diagnostic("changes", Some(row), &error));
            }
        }
        for (row, change) in document.command.master_changes.iter().enumerate() {
            let current = repository
                .get_master(&change.definition.key())
                .await
                .map_err(failure)?;
            let matches = match (change.expected_version, current) {
                (None, None) => true,
                (Some(expected), Some(current)) => expected == current.version,
                _ => false,
            };
            if !matches {
                diagnostics.push(diagnostic(
                    "master_changes",
                    Some(row),
                    &ReferenceError::Conflict("master expected version differs from current"),
                ));
            }
        }
        if !diagnostics.is_empty() {
            return Err(ReferenceImportError {
                error: ReferenceError::Conflict("reference import conflicts"),
                diagnostics,
            });
        }
    }
    repository
        .commit(&document.command, recorded_at)
        .await
        .map_err(|error| {
            let diagnostics = vec![diagnostic("transaction", None, &error)];
            ReferenceImportError { error, diagnostics }
        })
}
