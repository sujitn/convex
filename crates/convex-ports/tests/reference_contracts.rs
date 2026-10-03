use convex_core::contracts::{RequestId, UtcTimestamp};
use convex_ports::reference_data::BondReferenceData;
use convex_ports::reference_repository::*;

fn fixture() -> serde_json::Value {
    serde_json::from_str(include_str!("fixtures/reference_bond.json")).unwrap()
}

#[test]
fn reference_terms_are_exact_and_strict_at_wire_ingress() {
    let mut value = fixture();
    value["coupon_rate"] = "-0.1234567890123456789012345678".into();
    value["face_value"] = "79228162514264337593543950335".into();
    let bond: BondReferenceData = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(bond).unwrap(), value);
    for malformed in [
        serde_json::json!(0.05),
        serde_json::json!("1e-2"),
        serde_json::json!("0.12345678901234567890123456789"),
    ] {
        let mut value = fixture();
        value["coupon_rate"] = malformed;
        assert!(serde_json::from_value::<BondReferenceData>(value).is_err());
    }
    let mut unknown = fixture();
    unknown["unexpected"] = true.into();
    assert!(serde_json::from_value::<BondReferenceData>(unknown).is_err());
    let mut missing = fixture();
    missing.as_object_mut().unwrap().remove("day_count");
    assert!(serde_json::from_value::<BondReferenceData>(missing).is_err());
}

#[test]
fn command_requires_typed_versions_and_bounded_identity() {
    let mut value = serde_json::json!({"idempotency_key":"key","changes":[{"kind":"update","bond":fixture(),"expected_version":0}]});
    assert!(serde_json::from_value::<ReferenceCommand>(value.clone()).is_err());
    value["changes"][0]["expected_version"] = 1.into();
    let command: ReferenceCommand = serde_json::from_value(value.clone()).unwrap();
    command.validate().unwrap();
    value["changes"][0]["instrument_id"] = "other".into();
    assert!(serde_json::from_value::<ReferenceCommand>(value).is_err());
    assert!(RequestId::new("").is_err());
    let empty = ReferenceCommand {
        idempotency_key: RequestId::new("empty").unwrap(),
        changes: vec![],
    };
    assert!(empty.validate().is_err());
}

#[test]
fn planning_rejects_invalid_reference_without_clock_or_io() {
    let mut bond: BondReferenceData = serde_json::from_value(fixture()).unwrap();
    bond.issue_date = bond.maturity_date;
    let command = ReferenceCommand {
        idempotency_key: RequestId::new("invalid").unwrap(),
        changes: vec![ReferenceChange::Create {
            bond: Box::new(bond),
        }],
    };
    let time = UtcTimestamp::parse("2026-10-03T00:00:00Z").unwrap();
    assert!(matches!(
        plan_reference_commit(&command, time, |_| Ok(None)),
        Err(ReferenceError::InvalidInput(_))
    ));
}
