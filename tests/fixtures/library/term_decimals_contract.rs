//! Complete metadata controls declared independently of the native decoders.
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};

fn rebuild<T: DeserializeOwned + Serialize>(terms: &Value) -> Result<Value, serde_json::Error> {
    serde_json::to_value(serde_json::from_value::<T>(terms.clone())?)
}

fn decode(kind: &str, terms: &Value) -> Result<Value, serde_json::Error> {
    match kind {
        "bond" => rebuild::<convex::bond_contracts::BondTerms>(terms),
        "principal" => rebuild::<convex::bond_contracts::PrincipalPrice>(terms),
        "exercise" => rebuild::<convex::bond_contracts::ExerciseTerms>(terms),
        "profile" => rebuild::<convex::cashflows::DatedValue>(terms),
        "fixing" => rebuild::<convex::fixings::FixingObservation>(terms),
        "swap" => rebuild::<convex::rates::SwapTerms>(terms),
        other => panic!("undeclared metadata kind: {other}"),
    }
}

fn duplicate_rejected<T: DeserializeOwned>(text: &str) -> bool {
    serde_json::from_str::<T>(text).is_err()
}

pub fn run() -> Value {
    let mut roundtrips = 0;
    let mut rejections = 0;
    let mut duplicates = 0;
    let mut aliases = 0;
    for source in [
        include_str!("term-decimals-reference.json"),
        include_str!("swap-term-decimals-reference.json"),
    ] {
        let pack: Value = serde_json::from_str(source).unwrap();
        assert_eq!(pack["certified"], false);
        for case in pack["positive_cases"].as_array().unwrap() {
            let rebuilt = decode(case["kind"].as_str().unwrap(), &case["terms"]).unwrap();
            assert_eq!(
                rebuilt, case["terms"],
                "{}: metadata precision or fields lost",
                case["id"]
            );
            let text = serde_json::to_string(&rebuilt).unwrap();
            let input: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(
                decode(case["kind"].as_str().unwrap(), &input).unwrap(),
                case["terms"]
            );
            roundtrips += 1;
        }
        for case in pack["negative_cases"].as_array().unwrap() {
            assert!(
                decode(case["kind"].as_str().unwrap(), &case["terms"]).is_err(),
                "{}: invalid or lossy term accepted",
                case["id"]
            );
            rejections += 1;
        }
        for case in pack["alias_cases"].as_array().unwrap() {
            let rebuilt = decode(case["kind"].as_str().unwrap(), &case["terms"]).unwrap();
            assert_eq!(
                rebuilt, case["canonical_terms"],
                "{}: exact alias value/scale lost",
                case["id"]
            );
            assert_eq!(
                decode(case["kind"].as_str().unwrap(), &rebuilt).unwrap(),
                rebuilt
            );
            aliases += 1;
        }
        for case in pack["duplicate_cases"].as_array().unwrap() {
            let text = case["json"].as_str().unwrap();
            assert!(match case["kind"].as_str().unwrap() {
                "bond" => duplicate_rejected::<convex::bond_contracts::BondTerms>(text),
                "principal" => duplicate_rejected::<convex::bond_contracts::PrincipalPrice>(text),
                "exercise" => duplicate_rejected::<convex::bond_contracts::ExerciseTerms>(text),
                "profile" => duplicate_rejected::<convex::cashflows::DatedValue>(text),
                "fixing" => duplicate_rejected::<convex::fixings::FixingObservation>(text),
                other => panic!("undeclared duplicate kind: {other}"),
            });
            duplicates += 1;
        }
    }
    json!({"status":"passed","roundtrips":roundtrips,"rejections":rejections,
           "duplicate_rejections":duplicates,"compatible_aliases":aliases,"certified":false})
}
