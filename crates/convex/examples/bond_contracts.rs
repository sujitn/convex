//! Standalone CVX-055 dated fixed/floating cash, model PV and missing-history proof.
#![forbid(unsafe_code)]
#![deny(
    unused_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]
use convex::{
    bond_contract,
    bond_contracts::*,
    cashflows::*,
    contracts::{IndexId, UtcTimestamp},
    curve_contracts::*,
    fixings::*,
    Bond, BusinessDayConvention as Bdc, CalendarId, Compounding, Currency, Date,
    DayCountConvention as Dc, ExtrapolationMethod as Ex, FixedRateBond, Frequency,
    InterpolationMethod as Int,
};
fn date(s: &str) -> Result<Date, Box<dyn std::error::Error>> {
    Ok(Date::parse(s)?)
}
fn require(ok: bool, reason: &str) -> Result<(), Box<dyn std::error::Error>> {
    if ok {
        Ok(())
    } else {
        Err(CashflowError::InvalidTerms {
            field: "example",
            reason: reason.into(),
        }
        .into())
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let start = date("2024-01-01")?;
    let step = date("2024-04-01")?;
    let maturity = date("2024-07-01")?;
    let fixed = FixedRateBond::builder()
        .cusip_unchecked("SUPPLIED-CVX055")
        .coupon_rate("0.04".parse()?)
        .issue_date(start)
        .maturity(maturity)
        .frequency(Frequency::SemiAnnual)
        .day_count(Dc::Act360)
        .face_value(5000.into())
        .redemption_value(5000.into())
        .calendar(CalendarId::weekend_only())
        .business_day_convention(Bdc::Unadjusted)
        .end_of_month(false)
        .build()?;
    let mut profiles = BondProfiles::constant(
        start,
        maturity,
        "0.04".parse()?,
        0.into(),
        0.into(),
        fixed.face_value(),
    );
    profiles.coupon = vec![
        DatedValue {
            start,
            end: step,
            value: "0.04".parse()?,
        },
        DatedValue {
            start: step,
            end: maturity,
            value: "0.08".parse()?,
        },
    ];
    profiles.notional = vec![
        DatedValue {
            start,
            end: step,
            value: 5000.into(),
        },
        DatedValue {
            start: step,
            end: maturity,
            value: 3000.into(),
        },
    ];
    let bond = fixed.with_profiles(profiles)?;
    let curve_recipe = |id: &str, role: CurveRole, rate: f64| CurveRecipe {
        key: CurveKey {
            id: id.into(),
            version: "supplied-v1".into(),
        },
        role,
        currency: Currency::USD,
        reference_date: start,
        time_basis: Dc::Act365Fixed,
        representation: CurveRepresentation::ZeroRates {
            compounding: Compounding::Continuous,
        },
        interpolation: Int::Linear,
        extrapolation: Ex::Flat,
        dependencies: vec![],
        calibration: CalibrationMode::Direct,
        source: CurveSource::Nodes(vec![
            CurveNode {
                pillar: CurvePoint::Tenor(0.01),
                quote: MarketQuote {
                    value: rate,
                    unit: QuoteUnit::DecimalRate,
                },
            },
            CurveNode {
                pillar: CurvePoint::Tenor(1.0),
                quote: MarketQuote {
                    value: rate,
                    unit: QuoteUnit::DecimalRate,
                },
            },
        ]),
    };
    let discount = curve_recipe(
        "discount",
        CurveRole::Discount {
            collateral_currency: Currency::USD,
        },
        0.03,
    );
    let projection = curve_recipe(
        "projection",
        CurveRole::Projection {
            index_id: "SUPPLIED.TERM".into(),
        },
        0.04,
    );
    let built = build_curve_set(&[discount.clone(), projection.clone()], &CurveSet::new())?;
    let discount = built.curves.get(&discount.key)?;
    let projection = built.curves.get(&projection.key)?;
    let quoted =
        bond_contract::price_fixed(&bond, start, PaymentDateEntitlement::Exclude, discount)?;
    require(
        quoted.projection.principal.len() == 2,
        "actual step principal plus final residual",
    )?;
    let mut terms = bond.terms().clone();
    terms.profiles.coupon = vec![DatedValue {
        start,
        end: maturity,
        value: 0.into(),
    }];
    terms.profiles.gearing = vec![DatedValue {
        start,
        end: maturity,
        value: 1.into(),
    }];
    terms.profiles.spread = vec![DatedValue {
        start,
        end: maturity,
        value: "0.002".parse()?,
    }];
    let mut resets = terms.schedule.clone();
    resets.frequency = Frequency::Quarterly;
    terms.coupon = BondCoupon::Term(TermRateTerms {
        index_id: IndexId::new("SUPPLIED.TERM")?,
        resets,
        reset_adjustment: Bdc::Unadjusted,
        fixing_offset: DateOffset::CalendarDays(0),
        projection_day_count: Dc::Act360,
    });
    let floating = terms.build()?;
    let context = BondProjectionContext {
        settlement: start,
        on_payment: PaymentDateEntitlement::Exclude,
        fixing: FixingContext {
            valuation_date: date("2023-12-31")?,
            known_at: UtcTimestamp::parse("2023-12-31T18:00:00Z")?,
            unpublished: UnpublishedFixingPolicy::RequirePublished,
            final_only: true,
        },
        overnight_accrual_cutoffs: vec![],
    };
    let publication = |_: &IndexId, d: Date| {
        UtcTimestamp::parse(&format!("{d}T16:00:00Z")).map_err(|e| CashflowError::InvalidTerms {
            field: "publication",
            reason: e.to_string(),
        })
    };
    let mut forecast =
        |r: &ProjectionRequest| bond_contract::project_from_curve(r, Currency::USD, projection);
    let floated = bond_contract::price_floating(
        &floating,
        &context,
        &FixingSeries::default(),
        publication,
        BondProjectionPolicy::Supplied(&mut forecast),
        discount,
    )?;
    let mut due = context;
    due.fixing.valuation_date = start;
    due.fixing.known_at = UtcTimestamp::parse("2024-01-01T18:00:00Z")?;
    let missing = floating.project(
        &due,
        &FixingSeries::default(),
        publication,
        BondProjectionPolicy::Supplied(&mut forecast),
    );
    require(
        matches!(
            missing,
            Err(BondContractError::Cashflow(CashflowError::MissingFixings(
                _
            )))
        ),
        "due history must not forecast",
    )?;
    println!("fixed_clean={:.12} floating_clean={:.12} principal_steps={} missing_history=explicit certified=false",
        quoted.clean_price,floated.clean_price,quoted.projection.principal.len());
    Ok(())
}
