//! Packed regression fixtures, read once per process.
//!
//! Corpora listed in `tests/fixtures/packed.json` are committed xz-compressed;
//! `python3 tools/fixtures.py materialize` writes the exact pinned JSON beside
//! each archive before tests, examples or scripts read it.
#![allow(dead_code)]
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Text of a workspace-relative packed fixture, cached like `include_str!`.
pub fn text(path: &str) -> &'static str {
    static CACHE: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(Default::default).lock().unwrap();
    if let Some(&text) = cache.get(path) {
        return text;
    }
    let file = format!("{}/../../{path}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&file)
        .unwrap_or_else(|e| panic!("{file}: {e}; run `python3 tools/fixtures.py materialize`"))
        .leak();
    cache.insert(path.to_string(), text);
    text
}
