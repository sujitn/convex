#[path = "../examples/standalone_book.rs"]
mod example;

#[test]
fn quotes_to_curves_to_full_mixed_book_risk_and_indicative_hedge_preserves_missing(
) -> Result<(), Box<dyn std::error::Error>> {
    let result = example::run()?;
    assert_eq!(result["certified"], false);
    assert_eq!(
        result["prices"].as_array().ok_or("prices missing")?.len(),
        5
    );
    assert_eq!(
        result["built_curves"]
            .as_array()
            .ok_or("curves missing")?
            .len(),
        8
    );
    assert_eq!(result["complete_risk"]["status"], "complete");
    assert_eq!(result["incomplete_risk"]["status"], "incomplete");
    assert_eq!(result["unknown_gross_risk"]["status"], "incomplete");
    assert_eq!(
        result["unknown_gross_risk"]["local"],
        result["complete_risk"]["local"]
    );
    assert_eq!(
        result["complete_risk"]["coverage"][0]["unknown_exposure_trades"],
        0
    );
    assert_eq!(result["prices"][4]["basis"], "FuturesQuoteLevel");
    assert!(
        result["unknown_gross_risk"]["coverage"][0]["unknown_exposure_trades"]
            .as_u64()
            .ok_or("missing gross coverage")?
            > 0
    );
    let y = result["yield_inverse"]["decimal_rate"]
        .as_f64()
        .ok_or("missing inverse")?;
    assert!((y - 0.031).abs() < 1e-10);
    Ok(())
}

#[test]
fn standalone_executable_entry_point_uses_public_native_facade(
) -> Result<(), Box<dyn std::error::Error>> {
    example::main()
}
