//! Generate the C header for the FFI surface via cbindgen.
//!
//! The header is written to `$OUT_DIR/convex.h` on every build (no source-tree
//! churn, no rebuild loop). C consumers can pick it up from the build output;
//! the committed reference copy under `bindings/java/native-headers/convex.h`
//! is refreshed manually with the `cbindgen` CLI (see `cbindgen.toml`).
//!
//! The Java binding does not consume this header — Panama FFM binds to the
//! cdylib by symbol name — but generating it keeps the documented C ABI in
//! lockstep with the `#[no_mangle]` surface and supports other C callers.

use std::env;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let crate_dir = env::var("CARGO_MANIFEST_DIR")?;
    let out_dir = env::var("OUT_DIR")?;
    let config = cbindgen::Config::from_root_or_default(&crate_dir);

    // cbindgen parses the whole crate, so re-run on any source change.
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=cbindgen.toml");

    let bindings = cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_config(config)
        .generate()?;
    let header = PathBuf::from(&out_dir).join("convex.h");
    let mut contents = Vec::new();
    bindings.write(&mut contents);
    if contents.is_empty() {
        return Err("C ABI header generation produced no output".into());
    }
    std::fs::write(header, contents)?;
    Ok(())
}
