//! Exact full-input causal admission; serialization compares representations,
//! never prices. Native owners remain responsible for financial validation.
use super::*;
use std::collections::BTreeMap;
fn bad(reason: &str) -> LibraryError {
    LibraryError::invalid("causal information set", reason)
}
fn unsupported(reason: &str) -> LibraryError {
    LibraryError {
        code: LibraryErrorCode::Unsupported,
        message: reason.into(),
        details: Value::Null,
    }
}
fn equal<A: Serialize + ?Sized, B: Serialize + ?Sized>(a: &A, b: &B) -> LibraryResult<bool> {
    Ok(value(a)? == value(b)?)
}
fn differences(a: &Value, b: &Value, path: &str, out: &mut Vec<String>) {
    if a == b {
        return;
    }
    match (a, b) {
        (Value::Object(a), Value::Object(b)) if a.keys().eq(b.keys()) => {
            for (k, v) in a {
                differences(v, &b[k], &format!("{path}.{k}"), out);
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (i, (a, b)) in a.iter().zip(b).enumerate() {
                differences(a, b, &format!("{path}.{i}"), out);
            }
        }
        _ => out.push(path.into()),
    }
}
fn point_context(set: &CausalPnlInformationSet) -> LibraryResult<()> {
    if set.holdings.len() != set.point.positions.len() {
        return Err(bad("complete exact holding coverage required"));
    }
    for (h, p) in set.holdings.iter().zip(&set.point.positions) {
        if h.id != p.identity.id || h.lifecycle.trade_id != h.id {
            return Err(bad(
                "same exact lifecycle/product/quantity identity in original order required",
            ));
        }
        for step in &h.lifecycle.events {
            if step.event.known_at > set.point.known_at
                || step.event.effective_date > set.point.valuation_date
            {
                return Err(bad("events unavailable at supplied point"));
            }
            if let convex_analytics::lifecycle::LifecycleAction::Fixing(o) = &step.event.action {
                if !set.inputs.fixings.iter().any(|x| x == o) {
                    return Err(bad(
                        "actual lifecycle fixing absent from exact market history",
                    ));
                }
            }
        }
        if h.lifecycle
            .history
            .iter()
            .any(|o| !set.inputs.fixings.contains(o))
        {
            return Err(bad(
                "original opening history absent from point's exact market history",
            ));
        }
        let date = set.point.valuation_date;
        let known = set.point.known_at;
        let fixing = |c: &convex_core::fixings::FixingContext| {
            c.valuation_date == date && c.known_at == known
        };
        match &h.product {
            BookScenarioProductInput::CashBasis { security, .. } => {
                if security.helper.settlement != date
                    || security.helper.on_payment
                        != convex_core::cashflows::PaymentDateEntitlement::Exclude
                {
                    return Err(bad(
                        "cash settlement root and excluded boundary cash required",
                    ));
                }
            }
            BookScenarioProductInput::Product { product } => match product.as_ref() {
                RiskProductInput::Bond { context, .. } => {
                    if context.settlement != date
                        || !fixing(&context.fixing)
                        || context.on_payment
                            != convex_core::cashflows::PaymentDateEntitlement::Exclude
                    {
                        return Err(bad(
                            "bond exact root/cutoff and excluded boundary cash required",
                        ));
                    }
                }
                RiskProductInput::Swap { context, .. } => {
                    if !fixing(&context.fixings)
                        || context.on_payment
                            != convex_core::cashflows::PaymentDateEntitlement::Exclude
                    {
                        return Err(bad(
                            "swap exact root/cutoff and excluded boundary cash required",
                        ));
                    }
                }
                RiskProductInput::Cds { context, .. } => {
                    if context.valuation_date != date {
                        return Err(bad("CDS exact model root required"));
                    }
                }
                RiskProductInput::OptionalBond { input, .. } => {
                    if input.context.settlement != date
                        || input.projection_context.settlement != date
                        || !fixing(&input.projection_context.fixing)
                        || input.context.on_payment
                            != convex_core::cashflows::PaymentDateEntitlement::Exclude
                    {
                        return Err(bad(
                            "optional exact root/cutoff and excluded boundary cash required",
                        ));
                    }
                }
                RiskProductInput::RateFuture { input } => {
                    if input.context.as_of.datetime().date_naive() != date.as_naive_date()
                        || input.context.known_at != known
                    {
                        return Err(bad("rate future exact economic instant/cutoff required"));
                    }
                }
                RiskProductInput::BondFuture { input, .. } => {
                    if input.settlement.date() != date
                        || input.context.as_of.datetime().date_naive() != date.as_naive_date()
                        || input.context.known_at != known
                    {
                        return Err(bad("delivery exact root/instant/cutoff required"));
                    }
                }
            },
        }
    }
    if set
        .inputs
        .fixings
        .iter()
        .any(|o| o.known_at > set.point.known_at || o.published_at > set.point.known_at)
    {
        return Err(bad(
            "unavailable future received/published history cannot enter an actual information set",
        ));
    }
    Ok(())
}
fn metadata(path: &str) -> bool {
    path == ".point.snapshot_id"
        || path == ".point.manifest.id"
        || path == ".point.manifest.version"
        || (path.starts_with(".point.positions.")
            && (path.contains(".identity.economics.") || path.contains(".identity.calculation.")))
        || (path.starts_with(".point.fx.") && path.ends_with(".snapshot_id"))
}
fn clock(path: &str) -> bool {
    path == ".point.known_at"
        || (path.starts_with(".holdings.")
            && path.contains(".product.")
            && (path.ends_with(".known_at") || path.ends_with(".context.as_of")))
}
fn time(path: &str) -> bool {
    if path.starts_with(".holdings.")
        && path.contains(".product.product.input.members.")
        && [
            ".financing.start",
            ".own_cash.start",
            ".coupon_carry.rate.start",
            ".financing.stamp.version",
            ".own_cash.stamp.version",
            ".coupon_carry.rate.stamp.version",
        ]
        .iter()
        .any(|suffix| path.ends_with(suffix))
    {
        return true;
    }
    path == ".point.valuation_date"
        || (path.starts_with(".holdings.")
            && path.contains(".product.")
            && (path.ends_with(".context.settlement")
                || path.ends_with(".context.valuation_date")
                || path.ends_with(".fixing.valuation_date")
                || path.ends_with(".fixings.valuation_date")
                || path.ends_with(".projection_context.settlement")
                || path.ends_with(".helper.settlement")
                || path.ends_with(".helper.trade_date")
                || path.ends_with(".input.settlement")
                || path.ends_with(".context.step_in_date")
                || path.ends_with(".context.cash_settlement_date")))
}
fn market(path: &str) -> bool {
    if path.starts_with(".holdings.")
        && path.contains(".lifecycle.instrument.")
        && (path.contains(".discount_curve.")
            || path.contains(".projection_curve.")
            || path.contains(".survival_curve."))
    {
        return true;
    }
    path.starts_with(".inputs.curves")
        || path.starts_with(".inputs.calibration")
        || (path.starts_with(".point.positions.") && path.contains(".cash_discount."))
        || (path.starts_with(".holdings.")
            && path.contains(".product.")
            && (path.contains(".method.decimal_rate")
                || path.contains(".method.decimal_spread")
                || path.contains(".method.curve.")
                || path.contains(".discount.")
                || path.contains(".discount_curve.")
                || path.contains(".projection.")
                || path.contains(".projection_curve.")
                || path.contains(".survival.")
                || path.contains(".survival_curve.")
                || path.contains(".basis.")
                || path.contains(".model.recovery")
                || path.contains(".helper.quote.")
                || path.ends_with(".helper.quote_version")
                || path.contains(".members.")
                    && (path.ends_with(".financing.repo_decimal")
                        || path.ends_with(".financing.haircut_fraction")
                        || path.contains(".financing.stamp.")
                        || path.ends_with(".own_cash.decimal_rate")
                        || path.contains(".own_cash.stamp.")
                        || path.ends_with(".coupon_carry.rate.decimal_rate")
                        || path.contains(".coupon_carry.rate.stamp.")
                        || path.ends_with(".clean_price_per100")
                        || path.contains(".quote_stamp."))
                || path.contains(".input.market.")))
}
fn model(path: &str) -> bool {
    path.starts_with(".holdings.")
        && path.contains(".product.product.")
        && (path.contains(".input.model.")
            || path.ends_with(".oas_decimal")
            || path.contains(".convexity_model."))
}
fn amendment(path: &str) -> bool {
    path.starts_with(".inputs.calendars")
        || path.starts_with(".inputs.helper_calendars")
        || (path.starts_with(".point.positions.") && path.contains(".identity.instrument."))
        || (path.starts_with(".holdings.")
            && (path.contains(".terms.")
                || path.contains(".bond.calendars.")
                || path.contains(".lifecycle.instrument.")
                || path.ends_with(".lifecycle.version")))
}
fn prefix_events(
    a: &CausalPnlInformationSet,
    b: &CausalPnlInformationSet,
    cause: w::PnlCause,
) -> LibraryResult<()> {
    use convex_analytics::lifecycle::LifecycleAction as A;
    for (x, y) in a.holdings.iter().zip(&b.holdings) {
        if y.lifecycle.events.len() < x.lifecycle.events.len()
            || !equal(
                &x.lifecycle.events,
                &y.lifecycle.events[..x.lifecycle.events.len()],
            )?
        {
            return Err(bad(
                "original replay prefix cannot disappear, change or reorder",
            ));
        }
        for s in &y.lifecycle.events[x.lifecycle.events.len()..] {
            let admitted=match cause{
                w::PnlCause::Time=>match &s.event.action{A::ContractStep|A::ContractualCash=>true,A::Fixing(o)=>a.inputs.fixings.contains(o),A::Exercise{provision,..}=>x.lifecycle.events.iter().any(|n|matches!(n.event.action,A::ExerciseNotice{provision:p,exercise_date:d} if p==*provision&&d==s.event.effective_date)),_=>false},
                w::PnlCause::Fixing=>matches!(s.event.action,A::Fixing(_)),
                w::PnlCause::Event=>!matches!(s.event.action,A::Fixing(_)|A::ContractStep|A::ContractualCash|A::BondAmendment{..}),
                w::PnlCause::Amendment=>matches!(s.event.action,A::BondAmendment{..}),
                _=>false,
            };
            if !admitted {
                return Err(bad(
                    "new replay event belongs to a different declared cause",
                ));
            }
        }
    }
    Ok(())
}
fn step(
    a: &CausalPnlInformationSet,
    b: &CausalPnlInformationSet,
    cause: w::PnlCause,
    physical_assets: &[usize],
    legal_cash_transfer: bool,
) -> LibraryResult<()> {
    if a.holdings.len() != b.holdings.len() {
        return Err(bad("full original expected coverage cannot change"));
    }
    prefix_events(a, b, cause)?;
    if cause == w::PnlCause::Amendment {
        for (x, y) in a.holdings.iter().zip(&b.holdings) {
            let changed = !equal(&x.lifecycle.instrument, &y.lifecycle.instrument)?;
            if changed && (!x.lifecycle.events.is_empty() || !y.lifecycle.events.is_empty()) {
                return Err(unsupported("replayed opening economics cannot be replaced; supply the original full boundary amendment event"));
            }
            if !equal(&x.product, &y.product)?
                && !x.lifecycle.events.is_empty()
                && !y.lifecycle.events[x.lifecycle.events.len()..]
                    .iter()
                    .any(|s| {
                        matches!(
                            s.event.action,
                            convex_analytics::lifecycle::LifecycleAction::BondAmendment { .. }
                        )
                    })
            {
                return Err(unsupported("amended replayed valuation requires its original complete boundary amendment event"));
            }
        }
    }
    let mut paths = Vec::new();
    differences(&value(a)?, &value(b)?, "", &mut paths);
    let mut economic = legal_cash_transfer && cause == w::PnlCause::Position;
    for path in paths {
        if metadata(&path) {
            continue;
        }
        if clock(&path) {
            if matches!(
                cause,
                w::PnlCause::Time
                    | w::PnlCause::Fixing
                    | w::PnlCause::Event
                    | w::PnlCause::Position
                    | w::PnlCause::Amendment
            ) {
                continue;
            }
            return Err(bad(
                "unrelated knowledge/economic clock change under this cause",
            ));
        }
        let allowed = match cause {
            w::PnlCause::Time => {
                time(&path)
                    || (path.starts_with(".holdings.") && path.ends_with(".lifecycle.events"))
            }
            w::PnlCause::Fixing => {
                path.starts_with(".inputs.fixings")
                    || path.starts_with(".inputs.publications")
                    || path.starts_with(".holdings.") && path.ends_with(".lifecycle.events")
                    || path.starts_with(".holdings.") && path.contains(".fixing_stamp.")
            }
            w::PnlCause::Market => market(&path),
            w::PnlCause::Model => model(&path),
            w::PnlCause::Fx => path.starts_with(".point.fx"),
            w::PnlCause::Amendment => {
                amendment(&path)
                    || (path.starts_with(".holdings.") && path.ends_with(".lifecycle.events"))
            }
            w::PnlCause::Position => {
                path.starts_with(".point.fills")
                    || path.starts_with(".point.positions.")
                        && (path.ends_with(".current.quantity")
                            || path.ends_with(".pending_quantity"))
            }
            w::PnlCause::Event => {
                path.starts_with(".holdings.") && path.ends_with(".lifecycle.events")
                    || path.ends_with(".context.initial_payment_state")
                    || physical_assets
                        .iter()
                        .any(|i| path == format!(".point.positions.{i}.current.quantity"))
            }
        };
        if !allowed {
            return Err(LibraryError {
                code: LibraryErrorCode::InvalidInput,
                message: "changed data does not belong to declared cause".into(),
                details: serde_json::json!({"cause":cause,"path":path}),
            });
        }
        economic = true;
    }
    if !economic {
        return Err(bad(
            "metadata-only or clock-only phase has no declared causal input",
        ));
    }
    if b.inputs.fixings.len() < a.inputs.fixings.len()
        || !equal(
            &a.inputs.fixings,
            &b.inputs.fixings[..a.inputs.fixings.len()],
        )?
        || b.inputs.publications.len() < a.inputs.publications.len()
        || !equal(
            &a.inputs.publications,
            &b.inputs.publications[..a.inputs.publications.len()],
        )?
    {
        return Err(bad(
            "original immutable fixing/publication records cannot disappear or change",
        ));
    }
    Ok(())
}
fn pin(map: &mut BTreeMap<String, Value>, key: String, record: Value) -> LibraryResult<()> {
    if let Some(old) = map.get(&key) {
        if *old != record {
            return Err(LibraryError {
                code: LibraryErrorCode::InvalidInput,
                message: "changed source values reuse an original immutable revision".into(),
                details: serde_json::json!({"key":key}),
            });
        }
    } else {
        map.insert(key, record);
    }
    Ok(())
}
fn numerical_policies(record: &Value, path: &str, policies: &mut BTreeMap<String, Value>) {
    match record {
        Value::Object(fields) => {
            for (key, value) in fields {
                let path = format!("{path}.{key}");
                if matches!(
                    key.as_str(),
                    "controls"
                        | "bootstrap"
                        | "settings"
                        | "inverse"
                        | "tolerance"
                        | "price_tolerance"
                        | "overlap_policy"
                        | "overlap"
                ) {
                    policies.insert(path, value.clone());
                } else {
                    numerical_policies(value, &path, policies);
                }
            }
        }
        Value::Array(rows) => {
            for (i, row) in rows.iter().enumerate() {
                numerical_policies(row, &format!("{path}.{i}"), policies);
            }
        }
        _ => (),
    }
}
/// Full input bounds and source identity/cause admission precede all native builds.
pub(super) fn validate(
    input: &MixedBookPnlInput,
    outer: &LibraryInputs,
    sets: &[CausalPnlInformationSet],
    cohorts: Option<&CohortBookPnlInput>,
) -> LibraryResult<()> {
    if !equal(outer, &LibraryInputs::empty())? {
        return Err(bad(
            "outer inputs must be explicitly empty; all values bind to their exact point",
        ));
    }
    if !(1..=16).contains(&input.steps.len()) || input.standalone.len() != input.steps.len() {
        return Err(bad(
            "one complete standalone/dependent declaration per1..16 causes required",
        ));
    }
    let baseline = &sets[0];
    let physical_assets = cohorts.map_or(Vec::new(), |c| {
        baseline
            .point
            .positions
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                c.physical_deliveries
                    .iter()
                    .any(|l| l.asset == p.identity.id)
                    || c.physical_partitions
                        .iter()
                        .any(|a| a.split.children.contains(&p.identity.id))
            })
            .map(|(i, _)| i)
            .collect()
    });
    let mut original_controls = BTreeMap::new();
    numerical_policies(&value(baseline)?, "", &mut original_controls);
    for set in &sets[1..] {
        let mut controls = BTreeMap::new();
        numerical_policies(&value(set)?, "", &mut controls);
        if controls != original_controls {
            return Err(unsupported("original numerical controls, brackets, tolerances, inverse and convergence policies cannot change within this reconciliation"));
        }
    }
    for (i, s) in input.steps.iter().enumerate() {
        let legal = cohorts.is_some_and(|c| {
            c.dated_entitlements
                .iter()
                .any(|d| d.applied_points.contains(&(i + 1)) && !d.applied_points.contains(&i))
                || c.claim_transfers
                    .iter()
                    .any(|t| t.applied_points.contains(&(i + 1)) && !t.applied_points.contains(&i))
                || c.physical_partitions.iter().any(|p| {
                    p.split.applied_points.contains(&(i + 1))
                        && !p.split.applied_points.contains(&i)
                })
                || c.physical_disposals
                    .iter()
                    .any(|d| d.applied_points.contains(&(i + 1)) && !d.applied_points.contains(&i))
        });
        step(&sets[i], &sets[i + 1], s.cause, &physical_assets, legal)?;
    }
    let mut next = input.steps.len() + 1;
    for (s, c) in input.steps.iter().zip(&input.standalone) {
        if matches!(c, CausalPnlCounterfactual::Available { .. }) {
            let legal = cohorts.is_some_and(|c| {
                c.dated_entitlements
                    .iter()
                    .any(|d| d.applied_points.contains(&next))
                    || c.claim_transfers
                        .iter()
                        .any(|t| t.applied_points.contains(&next))
                    || c.physical_partitions
                        .iter()
                        .any(|p| p.split.applied_points.contains(&next))
                    || c.physical_disposals
                        .iter()
                        .any(|d| d.applied_points.contains(&next))
            });
            step(baseline, &sets[next], s.cause, &physical_assets, legal)?;
            next += 1;
        }
    }
    let mut bindings = vec![&input.before];
    bindings.extend(input.steps.iter().map(|s| &s.after));
    bindings.extend(input.standalone.iter().filter_map(|s| match s {
        CausalPnlCounterfactual::Available { information } => Some(information.as_ref()),
        _ => None,
    }));
    for i in 0..input.markets.len() {
        if !bindings.iter().any(|b| b.market == i) {
            return Err(bad("unused complete market bundle"));
        }
    }
    for i in 0..input.holdings.len() {
        if !bindings.iter().any(|b| b.holdings.contains(&i)) {
            return Err(bad("unused complete holding bundle"));
        }
    }
    let mut pins = BTreeMap::new();
    let mut curve_policy: BTreeMap<String, Value> = BTreeMap::new();
    for set in sets {
        point_context(set)?;
        for c in &set.inputs.curves {
            pin(
                &mut pins,
                format!("curve:{}:{}", c.key.id, c.key.version),
                value(c)?,
            )?;
            let mut policy = value(c)?;
            for field in ["key", "source"] {
                policy
                    .as_object_mut()
                    .ok_or_else(|| bad("complete curve object required"))?
                    .remove(field);
            }
            if let Some(role) = policy.get_mut("role").and_then(Value::as_object_mut) {
                role.remove("recovery");
            }
            if let Some(old) = curve_policy.get(&c.key.id) {
                if *old != policy {
                    return Err(unsupported("curve model/time/domain/extension policies cannot change under market data; declare a separately qualified complete model transition"));
                }
            } else {
                curve_policy.insert(c.key.id.clone(), policy);
            }
        }
        for c in &set.inputs.calendars {
            pin(
                &mut pins,
                format!("calendar:{}:{}", c.pin.id, c.pin.version.get()),
                value(c)?,
            )?;
        }
        for o in &set.inputs.fixings {
            pin(
                &mut pins,
                format!(
                    "fixing:{}:{}:{}",
                    o.index_id,
                    o.observation_date,
                    o.revision.get()
                ),
                value(o)?,
            )?;
        }
        for x in &set.point.fx {
            pin(
                &mut pins,
                format!("FX:{}:{}", x.observation.id, x.observation.version.get()),
                serde_json::json!({"from":x.from,"to":x.to,"rate":x.rate}),
            )?;
        }
        for h in &set.holdings {
            for step in &h.lifecycle.events {
                pin(
                    &mut pins,
                    format!("lifecycle event:{}:{}", h.id, step.event.event_id.as_str()),
                    value(step)?,
                )?;
                if let convex_analytics::lifecycle::LifecycleAction::BondAmendment {
                    version,
                    terms,
                    ..
                } = &step.event.action
                {
                    pin(
                        &mut pins,
                        format!("bond amendment:{}:{}", h.id, version.get()),
                        value(terms)?,
                    )?;
                }
            }
            let actual = value(&h.lifecycle.instrument)?;
            let economic = &set
                .point
                .positions
                .iter()
                .find(|p| p.identity.id == h.id)
                .ok_or_else(|| bad("coverage"))?
                .identity
                .economics;
            pin(
                &mut pins,
                format!(
                    "economics:{}:{}:{}",
                    h.id,
                    economic.id,
                    economic.version.get()
                ),
                actual,
            )?;
            if let BookScenarioProductInput::Product { product } = &h.product {
                if let RiskProductInput::OptionalBond { input, .. } = product.as_ref() {
                    let model = value(&input.model)?;
                    let key = model
                        .get("key")
                        .ok_or_else(|| bad("actual immutable model key required"))?;
                    pin(&mut pins, format!("optional model:{key}"), model)?;
                }
            }
            if let BookScenarioProductInput::Product { product } = &h.product {
                if let RiskProductInput::BondFuture { input, .. } = product.as_ref() {
                    for m in &input.members {
                        pin(
                            &mut pins,
                            format!(
                                "basket quote:{}:{}",
                                m.quote_stamp.id,
                                m.quote_stamp.version.get()
                            ),
                            serde_json::json!({"stamp":m.quote_stamp,"instrument":m.instrument,"price":m.clean_price_per100}),
                        )?;
                        pin(
                            &mut pins,
                            format!(
                                "basket repo:{}:{}",
                                m.financing.stamp.id,
                                m.financing.stamp.version.get()
                            ),
                            value(&m.financing)?,
                        )?;
                        for (kind, rate) in [
                            ("own cash", m.own_cash.as_ref()),
                            (
                                "reinvestment",
                                match &m.coupon_carry {
                                    convex_analytics::futures::FuturesCouponCarry::Reinvested {
                                        rate,
                                    } => Some(rate),
                                    _ => None,
                                },
                            ),
                        ] {
                            if let Some(rate) = rate {
                                pin(
                                    &mut pins,
                                    format!(
                                        "basket {kind}:{}:{}",
                                        rate.stamp.id,
                                        rate.stamp.version.get()
                                    ),
                                    value(rate)?,
                                )?;
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
