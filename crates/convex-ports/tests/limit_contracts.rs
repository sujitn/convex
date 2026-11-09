#![forbid(unsafe_code)]
//! P14a (D82–D84): the limit types' wire form and their validation.

use convex_core::contracts::InstrumentId;
use convex_ports::limits::{
    validate_limits, InstrumentList, LimitDefinition, LimitEvaluation, LimitStatus, UnknownReason,
    MAX_BOOK_LIMITS, MAX_LISTED_INSTRUMENTS,
};
use convex_ports::reference_repository::ReferenceError;
use serde_json::json;

fn limit(value: serde_json::Value) -> LimitDefinition {
    serde_json::from_value(value).unwrap()
}

fn refused(value: serde_json::Value) -> &'static str {
    match limit(value).validate() {
        Err(ReferenceError::InvalidInput(reason)) => reason,
        other => panic!("expected an invalid limit, got {other:?}"),
    }
}

/// Each measure's wire form parses and serializes back to the same JSON.
#[test]
fn every_measure_has_one_wire_form() {
    let limits = json!([
        {"limit_id":"face-one","measure":{"kind":"face","instrument":{"one":"UST-10Y"}},"upper":"5000000"},
        {"limit_id":"face-each","measure":{"kind":"face","instrument":"each"},"lower":"-1000000","upper":"1000000"},
        {"limit_id":"mv","measure":{"kind":"market_value"},"upper":"100000000","warning_fraction":"0.8"},
        {"limit_id":"gross","measure":{"kind":"gross_market_value"},"upper":"250000000"},
        {"limit_id":"dv01","measure":{"kind":"dv01"},"lower":"-50000","upper":"50000","warning_fraction":"0.75"},
        {"limit_id":"cs01","measure":{"kind":"cs01"},"upper":"20000"},
        {"limit_id":"krd-10y","measure":{"kind":"key_rate_dv01","tenor":"10Y"},"lower":"-15000","upper":"15000"},
        {"limit_id":"duration","measure":{"kind":"modified_duration"},"lower":"2","upper":"7"},
        {"limit_id":"issuer-each","measure":{"kind":"group_market_value","by":"issuer","group":"each"},"upper":"10000000"},
        {"limit_id":"country-share","measure":{"kind":"group_share","by":"country","group":{"one":"IT"}},"upper":"0.2"},
        {"limit_id":"rating-share","measure":{"kind":"group_share","by":"rating","group":"each"},"upper":"0.5"},
        {"limit_id":"sector","measure":{"kind":"group_market_value","by":"sector","group":{"one":"Banks"}},"lower":"0"}
    ]);
    let parsed: Vec<LimitDefinition> = serde_json::from_value(limits.clone()).unwrap();
    validate_limits(&parsed).unwrap();
    assert_eq!(serde_json::to_value(&parsed).unwrap(), limits);
}

/// An unknown field is refused, for a measure with fields, one without, and the limit.
#[test]
fn unknown_fields_are_refused() {
    for value in [
        json!({"limit_id":"a","measure":{"kind":"dv01","tenor":"5Y"},"upper":"1"}),
        json!({"limit_id":"a","measure":{"kind":"key_rate_dv01","tenor":"5Y","x":1},"upper":"1"}),
        json!({"limit_id":"a","measure":{"kind":"dv01"},"upper":"1","soft":true}),
        json!({"limit_id":"a","measure":{"kind":"risk"},"upper":"1"}),
        json!({"limit_id":"a","measure":{"kind":"dv01"},"upper":1}),
    ] {
        assert!(
            serde_json::from_value::<LimitDefinition>(value.clone()).is_err(),
            "{value}"
        );
    }
}

#[test]
fn a_limit_is_refused_unless_its_bounds_and_band_are_well_formed() {
    let dv01 = |extra: serde_json::Value| {
        let mut value = json!({"limit_id":"dv01","measure":{"kind":"dv01"}});
        value
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        value
    };
    assert_eq!(
        refused(dv01(json!({}))),
        "a limit needs a lower or an upper bound"
    );
    assert_eq!(
        refused(dv01(json!({"lower":"2","upper":"1"}))),
        "a limit's lower bound exceeds its upper bound"
    );
    for fraction in ["0", "1", "1.5", "-0.5"] {
        assert_eq!(
            refused(dv01(json!({"upper":"10","warning_fraction":fraction}))),
            "a warning fraction lies strictly between 0 and 1"
        );
    }
    for bounds in [
        json!({"upper":"0","warning_fraction":"0.8"}),
        json!({"upper":"-10","warning_fraction":"0.8"}),
        json!({"lower":"0","upper":"10","warning_fraction":"0.8"}),
        json!({"lower":"5","warning_fraction":"0.8"}),
    ] {
        assert_eq!(
            refused(dv01(bounds)),
            "a warning band needs each bound away from zero: upper above 0, lower below 0"
        );
    }
    // Equal bounds hold one value (a restricted instrument: face exactly 0).
    limit(json!({"limit_id":"none","measure":{"kind":"face","instrument":{"one":"X"}},"lower":"0","upper":"0"}))
        .validate()
        .unwrap();
    limit(dv01(
        json!({"lower":"-10","upper":"10","warning_fraction":"0.9"}),
    ))
    .validate()
    .unwrap();

    for tenor in ["spot", "parallel"] {
        assert_eq!(
            refused(
                json!({"limit_id":"k","measure":{"kind":"key_rate_dv01","tenor":tenor},"upper":"1"})
            ),
            "a key-rate limit needs a dated tenor"
        );
    }
    for key in ["", " Banks", "Banks ", "Ba\nnks", &"x".repeat(129)] {
        assert_eq!(
            refused(
                json!({"limit_id":"g","measure":{"kind":"group_share","by":"sector","group":{"one":key}},"upper":"1"})
            ),
            "a group key is 1 to 128 bytes, without control characters or outer spaces"
        );
    }
    limit(json!({"limit_id":"g","measure":{"kind":"group_share","by":"sector","group":{"one":"x".repeat(128)}},"upper":"1"}))
        .validate()
        .unwrap();
}

#[test]
fn a_set_holds_at_most_64_limits_with_unique_ids() {
    let dv01 = |id: String| limit(json!({"limit_id":id,"measure":{"kind":"dv01"},"upper":"1"}));
    validate_limits(&[]).unwrap();
    let full: Vec<_> = (0..MAX_BOOK_LIMITS)
        .map(|i| dv01(format!("l{i}")))
        .collect();
    validate_limits(&full).unwrap();
    let mut over = full.clone();
    over.push(dv01("extra".into()));
    assert!(matches!(
        validate_limits(&over),
        Err(ReferenceError::InvalidInput(
            "a book holds at most 64 limits"
        ))
    ));
    assert!(matches!(
        validate_limits(&[dv01("a".into()), dv01("a".into())]),
        Err(ReferenceError::InvalidInput(
            "limit ids are unique in a set"
        ))
    ));
    // A limit's own validation runs inside the set's.
    let bad = limit(json!({"limit_id":"b","measure":{"kind":"dv01"}}));
    assert!(validate_limits(&[dv01("a".into()), bad]).is_err());
}

#[test]
fn statuses_order_by_severity_and_reasons_list_a_bounded_prefix() {
    use LimitStatus::*;
    assert!(Within < Warning && Warning < Unknown && Unknown < Breach);

    let ids: Vec<_> = (0..20)
        .map(|i| InstrumentId::new(format!("I{i:02}")).unwrap())
        .collect();
    let list = InstrumentList::collect(&ids).unwrap();
    assert_eq!(list.count, 20);
    assert_eq!(list.first.len(), MAX_LISTED_INSTRUMENTS);
    assert_eq!(list.first, ids[..MAX_LISTED_INSTRUMENTS]);
    assert!(InstrumentList::collect(&[]).is_none());

    let evaluation = LimitEvaluation {
        limit_id: convex_ports::limits::LimitId::new("cs01").unwrap(),
        status: Unknown,
        value: None,
        member: None,
        utilisation: None,
        reason: Some(UnknownReason::MeasureNotComputed {
            measure: "cs01_mid".into(),
            instruments: InstrumentList::collect(&ids[..1]).unwrap(),
        }),
    };
    let wire = json!({"limit_id":"cs01","status":"unknown","reason":{"code":"measure_not_computed",
        "measure":"cs01_mid","instruments":{"count":1,"first":["I00"]}}});
    assert_eq!(serde_json::to_value(&evaluation).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<LimitEvaluation>(wire).unwrap(),
        evaluation
    );
    assert_eq!(
        serde_json::to_value(UnknownReason::ZeroNetValue {}).unwrap(),
        json!({"code":"zero_net_value"})
    );
}

/// P14b (D82, D85): a limit set and a proposal are master records of their own kinds.
#[test]
fn limit_sets_and_proposals_have_their_own_keys_and_wire_forms() {
    use convex_ports::limits::{LimitProposalDefinition, LimitSetDefinition};
    use convex_ports::reference_master::{MasterDefinition, MasterKey, MasterKind};
    let set = json!({
        "book_id": "BOOK-A",
        "limits": [{"limit_id": "face", "measure": {"kind": "face", "instrument": "each"}, "upper": "5"}],
        "proposed_by": "access:aaaa",
        "approved_by": "access:bbbb",
        "proposal_version": 1,
        "effective": {"from": "2026-11-04T00:00:00Z", "until": null},
        "provenance": {"source": "limits-api", "evidence_reference": "limits-api:k"},
    });
    let parsed: LimitSetDefinition = serde_json::from_value(set.clone()).unwrap();
    parsed.validate().unwrap();
    let definition = MasterDefinition::LimitSet(Box::new(parsed));
    let key = definition.key();
    assert_eq!(key.storage_key(), "limit_set/BOOK-A");
    assert_eq!(key.kind(), MasterKind::LimitSet);
    assert_eq!(MasterKind::LimitProposal.prefix(), "limit_proposal");
    let book = convex_core::contracts::ReferenceDataId::new("BOOK-A").unwrap();
    assert_eq!(
        MasterKey::LimitProposal(book.clone()).storage_key(),
        "limit_proposal/BOOK-A"
    );
    assert_ne!(MasterKey::LimitSet(book.clone()), MasterKey::Book(book));
    let wire = serde_json::to_value(&definition).unwrap();
    assert_eq!(wire["kind"], "limit_set");
    assert_eq!(wire["definition"], set);

    let set_refused = |edit: &dyn Fn(&mut serde_json::Value)| {
        let mut value = set.clone();
        edit(&mut value);
        let parsed: LimitSetDefinition = serde_json::from_value(value).unwrap();
        assert!(matches!(
            parsed.validate(),
            Err(ReferenceError::InvalidInput(_))
        ));
    };
    set_refused(&|v| v["proposed_by"] = json!(""));
    set_refused(&|v| v["approved_by"] = json!("access: spaced"));
    set_refused(&|v| v["approved_by"] = json!("a".repeat(129)));
    set_refused(&|v| v["effective"]["until"] = json!("2026-11-05T00:00:00Z"));
    set_refused(&|v| v["limits"][0]["upper"] = json!(null));
    // D85: a set approved from a proposal names two subjects; one committed in one step
    // names one.
    set_refused(&|v| v["approved_by"] = json!("access:aaaa"));
    set_refused(&|v| {
        v.as_object_mut().unwrap().remove("proposal_version");
    });
    let mut one_step = set.clone();
    one_step.as_object_mut().unwrap().remove("proposal_version");
    one_step["approved_by"] = json!("access:aaaa");
    serde_json::from_value::<LimitSetDefinition>(one_step)
        .unwrap()
        .validate()
        .unwrap();

    let proposal = json!({
        "book_id": "BOOK-A",
        "base_version": 1,
        "limits": [],
        "proposed_by": "access:aaaa",
        "closed": {"outcome": "approved", "by": "access:bbbb", "limit_set_version": 2},
        "effective": {"from": "2026-11-04T00:00:00Z", "until": "2026-11-04T00:00:01Z"},
        "provenance": {"source": "limits-api", "evidence_reference": "limits-api:k"},
    });
    let parsed: LimitProposalDefinition = serde_json::from_value(proposal.clone()).unwrap();
    parsed.validate().unwrap();
    assert!(!parsed.is_open());
    assert_eq!(serde_json::to_value(&parsed).unwrap(), proposal);
    let proposal_refused = |edit: &dyn Fn(&mut serde_json::Value)| {
        let mut value = proposal.clone();
        edit(&mut value);
        let parsed: LimitProposalDefinition = serde_json::from_value(value).unwrap();
        assert!(matches!(
            parsed.validate(),
            Err(ReferenceError::InvalidInput(_))
        ));
    };
    // A proposal ends exactly when it is closed.
    proposal_refused(&|v| v["effective"]["until"] = json!(null));
    proposal_refused(&|v| {
        v.as_object_mut().unwrap().remove("closed");
    });
    proposal_refused(&|v| v["closed"] = json!({"outcome": "withdrawn", "by": ""}));
    // Its proposer cannot approve it (D85), though it may withdraw it.
    proposal_refused(&|v| v["closed"]["by"] = json!("access:aaaa"));
    let mut withdrawn = proposal.clone();
    withdrawn["closed"] = json!({"outcome": "withdrawn", "by": "access:aaaa"});
    serde_json::from_value::<LimitProposalDefinition>(withdrawn)
        .unwrap()
        .validate()
        .unwrap();
    let mut open = proposal.clone();
    open.as_object_mut().unwrap().remove("closed");
    open["effective"]["until"] = json!(null);
    let open: LimitProposalDefinition = serde_json::from_value(open).unwrap();
    open.validate().unwrap();
    assert!(open.is_open());
    let mut unknown = proposal;
    unknown["closed"]["note"] = json!("x");
    assert!(serde_json::from_value::<LimitProposalDefinition>(unknown).is_err());
}

/// P15c (D94, D95): a book's acknowledgements are a master record of their own kind; each
/// entry lasts at most seven days, and each version changes one limit's entry.
#[test]
fn acknowledgements_have_their_own_key_and_wire_form_and_bounded_entries() {
    use convex_ports::limits::LimitAcknowledgementsDefinition;
    use convex_ports::reference_master::{MasterDefinition, MasterKey, MasterKind};
    let limit = json!({"limit_id": "face", "measure": {"kind": "face", "instrument": "each"}, "upper": "5"});
    let record = json!({
        "book_id": "BOOK-A",
        "acknowledgements": [{
            "limit": limit,
            "limit_set_version": 3,
            "acknowledged_by": "access:aaaa",
            "acknowledged_at": "2026-11-09T00:00:00Z",
            "expires_at": "2026-11-16T00:00:00Z",
            "note": "seen; desk reducing\nby Friday",
            "record_id": 42,
        }],
        "change": {"action": "acknowledged", "limit_id": "face"},
        "effective": {"from": "2026-11-09T00:00:00Z", "until": null},
        "provenance": {"source": "limits-api", "evidence_reference": "limits-api:k"},
    });
    let parsed: LimitAcknowledgementsDefinition = serde_json::from_value(record.clone()).unwrap();
    parsed.validate().unwrap();
    let definition = MasterDefinition::LimitAcknowledgements(Box::new(parsed.clone()));
    let key = definition.key();
    assert_eq!(key.storage_key(), "limit_acknowledgements/BOOK-A");
    assert_eq!(key.kind(), MasterKind::LimitAcknowledgements);
    assert_eq!(
        MasterKind::LimitAcknowledgements.prefix(),
        "limit_acknowledgements"
    );
    let book = convex_core::contracts::ReferenceDataId::new("BOOK-A").unwrap();
    assert_ne!(
        MasterKey::LimitAcknowledgements(book.clone()),
        MasterKey::LimitSet(book)
    );
    let wire = serde_json::to_value(&definition).unwrap();
    assert_eq!(wire["kind"], "limit_acknowledgements");
    assert_eq!(wire["definition"], record);

    // In force before its expiry, and only while the set holds the limit as acknowledged.
    let entry = &parsed.acknowledgements[0];
    let at = |text: &str| convex_core::contracts::UtcTimestamp::parse(text).unwrap();
    let same: Vec<convex_ports::limits::LimitDefinition> =
        vec![serde_json::from_value(limit.clone()).unwrap()];
    let mut moved = limit.clone();
    moved["upper"] = json!("6");
    let moved: Vec<convex_ports::limits::LimitDefinition> =
        vec![serde_json::from_value(moved).unwrap()];
    assert!(entry.in_force(at("2026-11-15T23:59:59.999999999Z"), &same));
    assert!(!entry.in_force(at("2026-11-16T00:00:00Z"), &same));
    assert!(!entry.in_force(at("2026-11-10T00:00:00Z"), &moved));
    assert!(!entry.in_force(at("2026-11-10T00:00:00Z"), &[]));

    let refused = |edit: &dyn Fn(&mut serde_json::Value)| {
        let mut value = record.clone();
        edit(&mut value);
        let parsed: LimitAcknowledgementsDefinition = serde_json::from_value(value).unwrap();
        assert!(matches!(
            parsed.validate(),
            Err(ReferenceError::InvalidInput(_))
        ));
    };
    // D95: at most seven days, and after it is made.
    refused(&|v| v["acknowledgements"][0]["expires_at"] = json!("2026-11-16T00:00:00.000001Z"));
    refused(&|v| v["acknowledgements"][0]["expires_at"] = json!("2026-11-09T00:00:00Z"));
    refused(&|v| v["acknowledgements"][0]["expires_at"] = json!("2026-11-08T00:00:00Z"));
    // A note is 1 to 1,024 bytes, not blank, with no control character but a line feed.
    refused(&|v| v["acknowledgements"][0]["note"] = json!(""));
    refused(&|v| v["acknowledgements"][0]["note"] = json!(" \n "));
    refused(&|v| v["acknowledgements"][0]["note"] = json!("a\tb"));
    refused(&|v| v["acknowledgements"][0]["note"] = json!("a".repeat(1025)));
    refused(&|v| v["acknowledgements"][0]["acknowledged_by"] = json!("access: spaced"));
    refused(&|v| v["acknowledgements"][0]["record_id"] = json!(0));
    refused(&|v| v["acknowledgements"][0]["limit"]["upper"] = json!(null));
    refused(&|v| v["effective"]["until"] = json!("2026-11-10T00:00:00Z"));
    // One entry a limit, and a change that matches the entries.
    refused(&|v| {
        let entry = v["acknowledgements"][0].clone();
        v["acknowledgements"].as_array_mut().unwrap().push(entry);
    });
    refused(&|v| v["change"] = json!({"action": "acknowledged", "limit_id": "other"}));
    refused(&|v| {
        v["change"] = json!({"action": "withdrawn", "limit_id": "face", "by": "access:bbbb"})
    });
    refused(&|v| v["change"] = json!({"action": "withdrawn", "limit_id": "other", "by": ""}));
    let mut withdrawn = record.clone();
    withdrawn["acknowledgements"] = json!([]);
    withdrawn["change"] = json!({"action": "withdrawn", "limit_id": "face", "by": "access:bbbb"});
    serde_json::from_value::<LimitAcknowledgementsDefinition>(withdrawn)
        .unwrap()
        .validate()
        .unwrap();
    let mut longest = record.clone();
    longest["acknowledgements"][0]["note"] = json!("é".repeat(512));
    serde_json::from_value::<LimitAcknowledgementsDefinition>(longest)
        .unwrap()
        .validate()
        .unwrap();
    let mut unknown = record;
    unknown["acknowledgements"][0]["status"] = json!("breach");
    assert!(serde_json::from_value::<LimitAcknowledgementsDefinition>(unknown).is_err());
}
