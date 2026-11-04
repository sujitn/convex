#[path = "support/futures_conformance.rs"]
mod conformance;
use convex_ports::reference_master::*;
use convex_ports::reference_repository::*;

#[test]
fn futures_basket_and_cash_master_plans_are_complete_and_bounded() {
    let command = conformance::command();
    command.validate().unwrap();
    let commit = plan_reference_commit(&command, conformance::first_time(), |_| Ok(None)).unwrap();
    let master = plan_master_changes(&command.master_changes, conformance::first_time(), |_| {
        Ok(None)
    })
    .unwrap();
    assert_eq!(commit.records.len(), 10);
    assert_eq!(master.len(), 8);
    let mut commit = commit;
    commit.master_records = master;
    validate_reference_links(&commit, |_| Ok(None), |_, _| Ok(None), |_, _| Ok(None)).unwrap();
    let mut bad = command.clone();
    if let MasterDefinition::Basket(v) = &mut bad.master_changes[4].definition {
        v.basket.bonds[0].instrument.version = convex_core::contracts::Version::new(99).unwrap();
    }
    let mut planned = plan_reference_commit(&bad, conformance::first_time(), |_| Ok(None)).unwrap();
    planned.master_records =
        plan_master_changes(&bad.master_changes, conformance::first_time(), |_| Ok(None)).unwrap();
    assert!(
        validate_reference_links(&planned, |_| Ok(None), |_, _| Ok(None), |_, _| Ok(None)).is_err()
    );
}

#[test]
fn master_version_known_at_and_missing_pin_validation_prevent_relabeling() {
    let mut command = conformance::command();
    if let MasterDefinition::Futures(v) = &mut command.master_changes[3].definition {
        v.contract.stamp.version = convex_core::contracts::Version::new(9).unwrap();
    }
    assert!(
        plan_master_changes(&command.master_changes, conformance::first_time(), |_| Ok(
            None
        ))
        .is_err()
    );
    assert!(plan_master_changes(
        &conformance::command().master_changes,
        conformance::t("2026-09-24T00:00:00Z"),
        |_| Ok(None)
    )
    .is_err());
    let mut command = conformance::command();
    if let MasterDefinition::Futures(v) = &mut command.master_changes[3].definition {
        v.contract.stamp.evidence = "different-evidence".into();
    }
    let mut planned =
        plan_reference_commit(&command, conformance::first_time(), |_| Ok(None)).unwrap();
    planned.master_records =
        plan_master_changes(&command.master_changes, conformance::first_time(), |_| {
            Ok(None)
        })
        .unwrap();
    assert!(
        validate_reference_links(&planned, |_| Ok(None), |_, _| Ok(None), |_, _| Ok(None)).is_err()
    );
}
