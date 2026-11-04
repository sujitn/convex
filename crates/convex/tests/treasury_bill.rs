//! Actual supplied-date bill economics, exact roundtrip and native/shared parity.
#[path = "../../../tests/fixtures/library/treasury_bill_contract.rs"]
mod contract;
use convex::{library::*, treasury_bill::*};
use serde_json::json;

#[test]
fn complete_native_shared_terms_and_all_typed_errors() {
    for case in contract::cases() {
        let input = contract::request(&case);
        contract::check(&case, &execute_json(&input));
        if !case["expected_error"].is_null() {
            continue;
        }
        let typed = decode_request(&input).unwrap();
        assert_eq!(serde_json::to_value(&typed).unwrap(), case["request"]);
        if let LibraryOperation::Zero {
            input,
            action: ZeroOperation::TreasuryBill { settlement, quote },
        } = typed.operation
        {
            let terms = TreasuryBillTerms {
                issue_date: input.issue_date,
                maturity: input.maturity,
                currency: input.currency,
                face_value: input.face_value,
                redemption_value: input.redemption_value,
            };
            let native = quote_treasury_bill(&terms, settlement, &quote).unwrap();
            let actual = contract::native(&contract::request(&case));
            assert_eq!(actual["result"]["data"]["terms"], json!(input));
            assert_eq!(actual["result"]["data"]["calculation"], json!(native));
            assert!(
                native
                    .investment_risk
                    .d_price_per100_d_decimal_rate
                    .decimal()
                    < rust_decimal::Decimal::ZERO
            );
            assert!(
                native
                    .investment_risk
                    .d2_price_per100_d_decimal_rate2
                    .decimal()
                    > rust_decimal::Decimal::ZERO
            );
            assert_eq!(
                native
                    .bank_discount_risk
                    .d2_price_per100_d_decimal_rate2
                    .decimal(),
                rust_decimal::Decimal::ZERO
            );
        } else {
            panic!("declared Treasury operation changed");
        }
    }
}
