//! Bounded, pull-based JSONL quote replay. File order and timestamps are preserved.
#![forbid(unsafe_code)]

use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use async_trait::async_trait;
use convex_ports::market_data::{QuoteEvent, QuoteEventSource, StreamError};
use convex_ports::TraitError;

/// Maximum physical replay file size, 256 MiB.
pub const MAX_REPLAY_BYTES: u64 = 256 * 1024 * 1024;
/// Maximum JSON record size including its newline, 64 KiB.
pub const MAX_REPLAY_RECORD_BYTES: usize = 64 * 1024;
/// Maximum records processed by one cursor.
pub const MAX_REPLAY_RECORDS: u64 = 1_000_000;

/// Sequential replay cursor with no background producer or accumulation queue.
/// Invalid records terminate the cursor with Failure; subsequent reads are Closed.
pub struct FileQuoteReplay {
    reader: BufReader<std::fs::File>,
    line: u64,
    bytes: u64,
    ended: bool,
}

impl FileQuoteReplay {
    /// Open a regular file and enforce its initial size. Empty files are a closed stream.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, TraitError> {
        let file = std::fs::File::open(path).map_err(|e| TraitError::IoError(e.to_string()))?;
        let metadata = file
            .metadata()
            .map_err(|e| TraitError::IoError(e.to_string()))?;
        if !metadata.is_file() {
            return Err(TraitError::InvalidInput("replay requires a regular file"));
        }
        if metadata.len() > MAX_REPLAY_BYTES {
            return Err(TraitError::CapacityExceeded);
        }
        Ok(Self {
            reader: BufReader::with_capacity(8192, file),
            line: 0,
            bytes: 0,
            ended: false,
        })
    }

    /// Number of consumed physical records, including a rejected record.
    pub fn position(&self) -> u64 {
        self.line
    }

    /// Synchronous bounded pull suitable for offline harnesses. Does not sleep or alter time.
    pub fn read_event(&mut self) -> Result<QuoteEvent, StreamError> {
        if self.ended {
            return Err(StreamError::Closed);
        }
        let outcome = self.read_record();
        if outcome.is_err() {
            self.ended = true;
        }
        outcome
    }

    fn read_record(&mut self) -> Result<QuoteEvent, StreamError> {
        let mut record = Vec::with_capacity(4096);
        let read = self
            .reader
            .by_ref()
            .take((MAX_REPLAY_RECORD_BYTES + 1) as u64)
            .read_until(b'\n', &mut record)
            .map_err(|e| TraitError::IoError(e.to_string()))?;
        if read == 0 {
            return Err(StreamError::Closed);
        }
        self.line += 1;
        self.bytes = self.bytes.saturating_add(read as u64);
        if read > MAX_REPLAY_RECORD_BYTES
            || self.bytes > MAX_REPLAY_BYTES
            || self.line > MAX_REPLAY_RECORDS
        {
            return Err(TraitError::CapacityExceeded.into());
        }
        let event: QuoteEvent = serde_json::from_slice(&record)
            .map_err(|e| TraitError::ParseError(format!("replay line {}: {e}", self.line)))?;
        // Semantic faults remain events for the assembler's quarantine and trace.
        // Schema decoding failures terminate delivery; no skipping malformed evidence.
        Ok(event)
    }
}

#[async_trait]
impl QuoteEventSource for FileQuoteReplay {
    async fn next_event(&mut self) -> Result<QuoteEvent, StreamError> {
        self.read_event()
    }
}
