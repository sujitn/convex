//! Offline full-product UFR fit consumer; inputs are explicitly fictional.
#[path = "../../../tests/fixtures/library/product_ufr.rs"]
mod fixture;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "schemars")]
    if std::env::args().any(|a| a == "--emit-schema") {
        println!("{}", convex::library::request_schema());
        return Ok(());
    }
    let mut rows = Vec::new();
    for case in fixture::cases() {
        let built = fixture::pure(&case)?;
        rows.push(serde_json::json!({"id":case["id"],"inputs":built.inputs(),"result":built.diagnostics()}));
    }
    println!(
        "{}",
        serde_json::json!({"certified":false,"qualification":"Fictional full-product calibration diagnostics","cases":rows})
    );
    Ok(())
}
