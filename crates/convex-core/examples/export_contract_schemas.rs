//! Print canonical schema snapshots without a server or external data source.

#[cfg(feature = "schemars")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "{}",
        serde_json::to_string_pretty(&convex_core::contracts::json_schemas())?
    );
    Ok(())
}

#[cfg(not(feature = "schemars"))]
fn main() {
    eprintln!("Enable --features schemars to export canonical contract schemas");
    std::process::exit(1);
}
