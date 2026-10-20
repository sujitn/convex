#![forbid(unsafe_code)]
#[cfg(test)]
mod tests {
    use convex_core::{
        ids::{InstrumentId, PortfolioId},
        Currency,
    };
    use convex_engine::{
        error::EngineError,
        portfolio_analytics::{Portfolio, PortfolioAnalyzer, Position},
        ports::output::{BondQuoteOutput, PortfolioAnalyticsOutput},
    };
    use rust_decimal::Decimal;
    use std::str::FromStr;
    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }
    fn position(id: &str, quantity: &str) -> Position {
        Position {
            instrument_id: InstrumentId::new(id),
            notional: d(quantity),
            sector: Some("Government".into()),
            rating: Some("AAA".into()),
        }
    }
    fn book(positions: Vec<Position>) -> Portfolio {
        Portfolio {
            portfolio_id: PortfolioId::new("original-engine-book"),
            name: "declared supplied quote book".into(),
            currency: Currency::USD,
            positions,
        }
    }
    fn quote(id: &str) -> BondQuoteOutput {
        serde_json::from_value(serde_json::json!({"instrument_id":id,"currency":"USD","settlement_date":"2025-01-15",
          "clean_price_mid":"99","accrued_interest":"1","modified_duration":"5","convexity":"30",
          "ytm_mid":"0.05","z_spread_mid":"100","dv01":"0.05","key_rate_durations":[["2Y","1"],["5Y","4"]],
          "timestamp":1736899200000i64,"pricing_spec":"actual-supplied-original-test","source":"independent-input","is_stale":false,"quality":100})).unwrap()
    }
    fn run(
        book: &Portfolio,
        quotes: &[BondQuoteOutput],
    ) -> Result<PortfolioAnalyticsOutput, EngineError> {
        PortfolioAnalyzer::new().calculate(book, quotes)
    }
    #[test]
    fn healthy_complete_quote() {
        let result = run(&book(vec![position("a", "1000000")]), &[quote("a")]).unwrap();
        assert_eq!(result.market_value, d("1000000"));
        assert_eq!(result.duration, d("5"));
        assert_eq!(result.dv01, d("500"));
    }
    #[test]
    fn healthy_zero_metric_is_actual_zero() {
        let mut q = quote("a");
        q.modified_duration = Some(Decimal::ZERO);
        q.dv01 = Some(Decimal::ZERO);
        let result = run(&book(vec![position("a", "1000000")]), &[q]).unwrap();
        assert_eq!(result.market_value, d("1000000"));
        assert_eq!(result.duration, Decimal::ZERO);
        assert_eq!(result.dv01, Decimal::ZERO);
    }
    #[test]
    fn healthy_signed_metric() {
        let mut q = quote("a");
        q.modified_duration = Some(d("-5"));
        q.dv01 = Some(d("-0.05"));
        let result = run(&book(vec![position("a", "1000000")]), &[q]).unwrap();
        assert_eq!(result.duration, d("-5"));
        assert_eq!(result.dv01, d("-500"));
    }
    #[test]
    fn missing_quote_does_not_become_complete_zero() {
        assert!(run(&book(vec![position("a", "1000000")]), &[]).is_err());
    }
    #[test]
    fn missing_duration_does_not_become_zero() {
        let mut q = quote("a");
        q.modified_duration = None;
        assert!(run(&book(vec![position("a", "1000000")]), &[q]).is_err());
    }
    #[test]
    fn missing_accrued_does_not_drop_position() {
        let mut q = quote("a");
        q.accrued_interest = None;
        assert!(run(&book(vec![position("a", "1000000")]), &[q]).is_err());
    }
    #[test]
    fn duplicate_quote_does_not_choose_last_price() {
        let first = quote("a");
        let mut second = first.clone();
        second.clean_price_mid = Some(d("49"));
        assert!(run(&book(vec![position("a", "1000000")]), &[first, second]).is_err());
    }
    #[test]
    fn foreign_currency_requires_actual_fx() {
        let mut q = quote("a");
        q.currency = Currency::EUR;
        assert!(run(&book(vec![position("a", "1000000")]), &[q]).is_err());
    }
    #[test]
    fn mixed_settlement_dates_are_not_one_valuation_context() {
        let first = quote("a");
        let mut second = quote("b");
        second.settlement_date = convex_core::Date::from_ymd(2025, 1, 16).unwrap();
        assert!(run(
            &book(vec![position("a", "1000000"), position("b", "1000000")]),
            &[first, second]
        )
        .is_err());
    }
    #[test]
    fn negative_nav_retains_weighted_measure() {
        let result = run(&book(vec![position("a", "-1000000")]), &[quote("a")]).unwrap();
        assert_eq!(result.market_value, d("-1000000"));
        assert_eq!(result.duration, d("5"));
        assert_eq!(result.dv01, d("-500"));
    }
    #[test]
    fn representable_final_currency_is_not_intermediate_panic() {
        let mut q = quote("a");
        q.clean_price_mid = Some(d("1"));
        q.accrued_interest = Some(d("1"));
        let p = book(vec![position("a", "79228162514264337593543950335")]);
        let result = std::panic::catch_unwind(|| run(&p, &[q]));
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap().unwrap().market_value,
            d("1584563250285286751870879006.7")
        );
    }
    #[test]
    fn incomplete_second_position_is_not_renormalized() {
        let mut second = quote("b");
        second.modified_duration = None;
        assert!(run(
            &book(vec![position("a", "1000000"), position("b", "9000000")]),
            &[quote("a"), second]
        )
        .is_err());
    }
    #[test]
    fn exact_wire_and_partial_factors_retain_original_coverage() {
        let first = quote("a");
        let mut second = quote("b");
        second.key_rate_durations = Some(vec![("2Y".into(), d("1"))]);
        let p = book(vec![position("a", "1000000"), position("b", "1000000")]);
        let output = run(&p, &[first.clone(), second.clone()]).unwrap();
        assert_eq!(output.num_positions, 2);
        assert_eq!(output.timestamp, 1736899200000i64);
        assert_eq!(output.settlement_date, first.settlement_date);
        assert_eq!(
            output.key_rate_coverage,
            vec![("2Y".into(), 2), ("5Y".into(), 1)]
        );
        assert!(output.key_rate_durations[0].1.is_some());
        assert!(output.key_rate_durations[1].1.is_none());
        let json = serde_json::to_value(&output).unwrap();
        for key in [
            "market_value",
            "duration",
            "convexity",
            "yield_value",
            "spread",
            "dv01",
        ] {
            assert!(json[key].is_string(), "{key}");
        }
        assert_eq!(d(json["market_value"].as_str().unwrap()), d("2000000"));
        let again = run(&p, &[first, second]).unwrap();
        assert_eq!(serde_json::to_value(again).unwrap(), json);
    }
    #[test]
    fn missing_factor_duplicate_factor_and_stale_inputs_are_distinct() {
        let p = book(vec![position("a", "1000000")]);
        let mut q = quote("a");
        q.key_rate_durations = None;
        assert!(run(&p, &[q.clone()]).unwrap().key_rate_durations.is_empty());
        q.key_rate_durations = Some(vec![("2Y".into(), d("1")), ("2Y".into(), d("2"))]);
        assert!(run(&p, &[q.clone()]).is_err());
        q = quote("a");
        q.is_stale = true;
        assert!(run(&p, &[q.clone()]).is_err());
        q = quote("a");
        q.key_rate_durations = Some(vec![("bad".into(), d("1"))]);
        assert!(run(&p, &[q]).is_err());
    }
}
