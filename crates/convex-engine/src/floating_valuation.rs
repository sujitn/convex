//! Floating rate notes through `convex::library::execute`: the frozen-projection discount
//! margin at the observed clean mid. Each coupon mechanic comes from the bond's explicit
//! terms, then its rate index's declared conventions, then the bond's own fields; a value
//! none of them supplies is refused with the field's name. Nothing is defaulted in code.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use crate::optional_valuation::{failure, holiday_table, library_failure, Failure};
use crate::ports::{
    fixing::FixingKey,
    reference_data::{BondReferenceData, BondType},
    reference_master::{
        BondContractTerms, CalendarDefinition, MasterDefinition, RateIndexKind,
        ResolvedBondReference,
    },
    snapshot::SnapshotFixing,
};
use crate::pricing_router::{calendar_id, PricingRouter};
use convex::library::{
    execute, BondCalendarPins, BondInput, BondOperation, BondReferenceRole, CalendarInput,
    LibraryInputs, LibraryOperation, LibraryRequest, LIBRARY_SCHEMA_VERSION,
};
use convex_analytics::workflow::{BondInverseSettings, BondPriceTarget};
use convex_bonds::contracts::{
    BondCalendars, BondCoupon, BondProfiles, BondProjectionContext, BondTerms, OvernightRateTerms,
    TermRateTerms,
};
use convex_core::{
    calendars::checked::add_business_days,
    cashflows::{CashflowError, DateOffset, OvernightConvention, PaymentDateEntitlement},
    contracts::{BusinessDate, ErrorCode, IndexId, ReferenceDataId, SourceId, UtcTimestamp},
    fixings::{FixingContext, FixingRequirement, UnpublishedFixingPolicy},
    Date,
};
use convex_curves::contracts::{CurveKey, CurveRecipe};
use rust_decimal::{prelude::ToPrimitive, Decimal};
use serde_json::Value;

/// Quantitative model for floating rate notes: the library's frozen-projection DM inverse.
pub const FLOATING_SNAPSHOT_MODEL: &str = "convex-floating-dm-v1";
/// Measures a floating rate note publishes, each from the solved discount margin.
pub const FLOATING_MEASURES: [&str; 4] = [
    "clean_price_mid",
    "dirty_price_mid",
    "accrued_interest",
    "discount_margin_mid",
];

// The repository's qualified floating DM inverse controls
// (`tests/fixtures/library/floating-workout-qualified.json`).
const DM_SOLVE: BondInverseSettings = BondInverseSettings {
    lower: -0.5,
    upper: 0.5,
    price_tolerance: 1e-9,
    parameter_tolerance: 1e-13,
    max_iterations: 128,
};
/// Holiday tables cover the bond's life plus this margin on each side, enough for 30
/// business days of lookback, fixing offset or payment lag.
const CALENDAR_MARGIN_DAYS: i64 = 92;

/// A floating rate note's native terms, with the calendars and publication rule they use.
pub(crate) struct FloatingNote {
    terms: BondTerms,
    pins: BondCalendarPins,
    payment: CalendarInput,
    index: CalendarInput,
    index_id: IndexId,
    fixing_source: SourceId,
    publication_lag: i32,
    publication_seconds: u32,
}

/// One floating rate note at its observed clean mid.
pub(crate) struct FloatingQuote {
    /// Model dirty price per 100 face at the solved margin.
    pub dirty: f64,
    /// Model accrued per 100 face, from the known and forecast fixings.
    pub accrued: f64,
    /// Unrounded basis points over the frozen projection, discounted on the collateral curve.
    pub discount_margin_bps: f64,
}

impl FloatingNote {
    /// Build the note from its pinned reference: the bullet schedule with an overnight or
    /// term coupon, the bond calendar for payments and the index calendar for fixings.
    pub(crate) fn new(reference: &ResolvedBondReference) -> Result<Self, Failure> {
        let unsupported = |reason: String| failure(ErrorCode::UnsupportedConvention, reason);
        let invalid = |reason: String| failure(ErrorCode::InvalidInput, reason);
        let bond = reference
            .record
            .bond
            .as_ref()
            .ok_or_else(|| invalid("tombstone reference".into()))?;
        let terms = bond.contractual_terms.as_ref().ok_or_else(|| {
            unsupported("floating rate note needs complete contractual terms".into())
        })?;
        let mechanics = terms.floating_mechanics.as_ref().ok_or_else(|| {
            unsupported("floating rate note needs floating mechanics pinning its rate index".into())
        })?;
        let floating = bond
            .floating_terms
            .as_ref()
            .filter(|_| bond.bond_type == BondType::FloatingRate)
            .ok_or_else(|| unsupported("only floating rate notes have floating coupons".into()))?;
        if floating.cap.is_some() || floating.floor.is_some() {
            return Err(unsupported(
                "floating caps and floors need an option model".into(),
            ));
        }
        let resolved = reference
            .floating
            .as_ref()
            .ok_or_else(|| invalid("missing pinned rate index".into()))?;
        let (MasterDefinition::RateIndex(index), MasterDefinition::Calendar(index_calendar)) =
            (&resolved.index.definition, &resolved.calendar.definition)
        else {
            return Err(invalid("wrong rate index resources".into()));
        };
        let MasterDefinition::Calendar(bond_calendar) = &reference.calendar.definition else {
            return Err(invalid("wrong calendar resource".into()));
        };
        let own = &mechanics.overrides;
        let declared = &index.conventions;
        let mut native = native_terms(bond, terms, floating.spread, index.calendar_code)?;
        native.coupon = match index.kind {
            RateIndexKind::Overnight => {
                let convention = OvernightConvention {
                    lookback_business_days: required(
                        own.lookback_business_days
                            .or(declared.lookback_business_days),
                        "lookback",
                    )?,
                    observation_shift: required(
                        own.observation_shift.or(declared.observation_shift),
                        "observation shift",
                    )?,
                    lockout_business_days: required(
                        own.lockout_business_days.or(declared.lockout_business_days),
                        "lockout",
                    )?,
                };
                if convention.observation_shift {
                    return Err(unsupported(
                        "observation-shifted accrual needs an explicit accrual cutoff the reference terms do not carry".into(),
                    ));
                }
                BondCoupon::Overnight(OvernightRateTerms {
                    index_id: index.index_id.clone(),
                    convention,
                })
            }
            RateIndexKind::Term => {
                if floating.reset_frequency != bond.frequency {
                    return Err(unsupported(
                        "term resets that differ from the coupon frequency need their own schedule"
                            .into(),
                    ));
                }
                let fixing_offset = required(
                    own.fixing_offset_business_days
                        .or(declared.fixing_offset_business_days),
                    "fixing offset",
                )?;
                // The index's quotation basis, else the bond's own accrual basis.
                let projection_day_count = own
                    .projection_day_count
                    .or(declared.projection_day_count)
                    .unwrap_or(terms.settings.day_count);
                // Resets follow the coupon periods, adjusted like the bond's payments.
                BondCoupon::Term(TermRateTerms {
                    index_id: index.index_id.clone(),
                    resets: native.schedule.clone(),
                    reset_adjustment: terms.settings.business_day_convention,
                    fixing_offset: DateOffset::BusinessDays(fixing_offset),
                    projection_day_count,
                })
            }
        };
        let (payment, index_table) = calendars(
            terms,
            bond.maturity_date,
            (bond_calendar, reference.calendar.version),
            (index_calendar, resolved.calendar.version),
            &native,
        )?;
        Ok(Self {
            pins: BondCalendarPins {
                payment: payment.pin.clone(),
                reset: index_table.pin.clone(),
                observation: index_table.pin.clone(),
                exercise: payment.pin.clone(),
            },
            terms: native,
            payment,
            index: index_table,
            index_id: index.index_id.clone(),
            fixing_source: index.fixing_source.clone(),
            publication_lag: index.publication_lag_business_days as i32,
            publication_seconds: index.publication_utc_seconds,
        })
    }

    /// The administrator's expected publication of one observation: the rate index's
    /// business-day lag on its calendar, at its UTC time of day.
    fn publication(&self, index: &IndexId, date: Date) -> Result<UtcTimestamp, CashflowError> {
        let error = |reason: &str| CashflowError::InvalidTerms {
            field: "publication",
            reason: reason.into(),
        };
        if index != &self.index_id {
            return Err(error("observation of another index"));
        }
        let day = add_business_days(date, self.publication_lag, &self.index)
            .map_err(|e| CashflowError::Date(e.to_string()))?;
        let at = day
            .as_naive_date()
            .and_hms_opt(
                self.publication_seconds / 3600,
                self.publication_seconds / 60 % 60,
                self.publication_seconds % 60,
            )
            .ok_or_else(|| error("invalid publication time"))?
            .and_utc();
        UtcTimestamp::from_datetime(at)
            .map_err(|_| error("publication outside the timestamp range"))
    }

    /// Every unpaid fixing at `settlement`, each with its expected publication.
    pub(crate) fn requirements(&self, settlement: Date) -> Result<Vec<FixingRequirement>, Failure> {
        let invalid = |e: String| failure(ErrorCode::InvalidInput, e);
        self.terms
            .clone()
            .build_with_calendars(&BondCalendars {
                payment: &self.payment,
                reset: &self.index,
                observation: &self.index,
                exercise: &self.payment,
            })
            .map_err(|e| invalid(e.to_string()))?
            .fixing_requirements(
                settlement,
                PaymentDateEntitlement::Exclude,
                |index, date| self.publication(index, date),
            )
            .map_err(|e| invalid(e.to_string()))
    }

    /// The fixings a snapshot at `as_of` must hold: observed by then and due for
    /// publication by `known_at`. Later observations are forecast from the projection.
    pub(crate) fn due_fixings(
        &self,
        as_of: Date,
        known_at: UtcTimestamp,
    ) -> Result<Vec<FixingKey>, Failure> {
        self.requirements(as_of)?
            .into_iter()
            .filter(|r| r.observation_date <= as_of && r.expected_publication <= known_at)
            .map(|r| {
                Ok(FixingKey {
                    index_id: r.index_id,
                    observation_date: BusinessDate::try_from(r.observation_date).map_err(|_| {
                        failure(ErrorCode::InvalidInput, "invalid observation date")
                    })?,
                    source: self.fixing_source.clone(),
                })
            })
            .collect()
    }

    /// The index identity a projection curve must carry for this note.
    pub(crate) fn index_id(&self) -> &IndexId {
        &self.index_id
    }
}

/// A mechanic from the bond's terms or its index's declarations, else refused by name.
fn required<T>(value: Option<T>, field: &str) -> Result<T, Failure> {
    value.ok_or_else(|| {
        failure(
            ErrorCode::UnsupportedConvention,
            format!(
                "floating {field} is neither in the bond's terms nor declared by its rate index"
            ),
        )
    })
}

/// The bullet's own schedule and dates, with a constant spread over gearing 1 on the face
/// notional, the contractual payment lag and the index calendar. The caller sets the coupon.
fn native_terms(
    bond: &BondReferenceData,
    terms: &BondContractTerms,
    spread_bps: Decimal,
    index_calendar: crate::ports::reference_master::BondCalendar,
) -> Result<BondTerms, Failure> {
    let invalid = |reason: String| failure(ErrorCode::InvalidInput, reason);
    let mut fixed = bond.clone();
    fixed.coupon_rate = Some(Decimal::ZERO);
    let mut native = PricingRouter { bump_bps: 1.0 }
        .to_fixed_rate_bond(&fixed)
        .map_err(|e| invalid(e.to_string()))?
        .contract_terms()
        .map_err(|e| invalid(e.to_string()))?;
    let period = native
        .profiles
        .coupon
        .first()
        .ok_or_else(|| invalid("empty coupon schedule".into()))?;
    let (start, end) = (period.start, period.end);
    let spread = spread_bps
        .checked_div(Decimal::from(10_000))
        .ok_or_else(|| invalid("unrepresentable floating spread".into()))?;
    native.profiles = BondProfiles::constant(
        start,
        end,
        Decimal::ZERO,
        spread,
        Decimal::ONE,
        bond.face_value,
    );
    native.dates.payment_delay = DateOffset::BusinessDays(terms.payment_lag as i32);
    let index = calendar_id(index_calendar);
    native.calendars.reset = index.clone();
    native.calendars.observation = index;
    Ok(native)
}

/// Holiday tables for the bond's and the index's pinned calendars over the bond's life.
fn calendars(
    terms: &BondContractTerms,
    maturity: Date,
    bond: (&CalendarDefinition, convex_core::contracts::Version),
    index: (&CalendarDefinition, convex_core::contracts::Version),
    native: &BondTerms,
) -> Result<(CalendarInput, CalendarInput), Failure> {
    let start = terms.dated_date.date().add_days(-CALENDAR_MARGIN_DAYS);
    let end = maturity.add_days(CALENDAR_MARGIN_DAYS);
    let table = |(definition, version): (&CalendarDefinition, convex_core::contracts::Version),
                 id: &convex_bonds::types::CalendarId| {
        if start < definition.coverage_from.date() || end > definition.coverage_until.date() {
            return Err(failure(
                ErrorCode::UnsupportedConvention,
                "a pinned calendar's coverage does not span the bond's life",
            ));
        }
        let native = id
            .try_to_calendar()
            .map_err(|e| failure(ErrorCode::InvalidInput, e.to_string()))?;
        let pin = convex::rates::ReferencePin {
            id: ReferenceDataId::new(id.as_str()).map_err(|_| {
                failure(ErrorCode::InvalidInput, "invalid native calendar identity")
            })?,
            version,
        };
        holiday_table(native.as_ref(), pin, start, end)
    };
    Ok((
        table(bond, &native.calendars.payment)?,
        table(index, &native.calendars.observation)?,
    ))
}

/// Value one floating rate note: the discount margin over `projection`, discounted on the
/// collateral curve `discount`, that reprices its observed clean mid.
#[allow(clippy::too_many_arguments)]
pub(crate) fn value_floating(
    note: &FloatingNote,
    recipes: &[CurveRecipe],
    discount: &CurveKey,
    projection: &CurveKey,
    settlement: Date,
    valuation_date: Date,
    known_at: UtcTimestamp,
    fixings: &[SnapshotFixing],
    clean_mid: Decimal,
) -> Result<FloatingQuote, Failure> {
    let invalid = |reason: &str| failure(ErrorCode::InvalidInput, reason);
    let observations = fixings
        .iter()
        .filter_map(|selected| selected.fixing.as_ref())
        .filter(|f| f.key.index_id == note.index_id && f.key.source == note.fixing_source)
        .map(crate::core_workflow::fixing_observation)
        .collect();
    let mut calendars = vec![note.payment.clone()];
    if note.index.pin != note.payment.pin {
        calendars.push(note.index.clone());
    }
    let request = LibraryRequest {
        schema_version: LIBRARY_SCHEMA_VERSION,
        inputs: LibraryInputs {
            calendars,
            helper_calendars: vec![],
            curves: recipes.to_vec(),
            calibration: None,
            fixings: observations,
            publications: note.requirements(settlement)?,
        },
        operation: LibraryOperation::Bond {
            bond: Box::new(BondInput {
                terms: note.terms.clone(),
                calendars: note.pins.clone(),
            }),
            context: BondProjectionContext {
                settlement,
                on_payment: PaymentDateEntitlement::Exclude,
                fixing: FixingContext {
                    valuation_date,
                    known_at,
                    unpublished: UnpublishedFixingPolicy::Forecast,
                    final_only: true,
                },
                overnight_accrual_cutoffs: vec![],
            },
            projection: Some(projection.clone()),
            action: BondOperation::SpreadInverse {
                target: BondPriceTarget::Clean(
                    clean_mid
                        .to_f64()
                        .ok_or_else(|| invalid("unrepresentable clean mid"))?,
                ),
                curve: discount.clone(),
                role: BondReferenceRole::Collateral {},
                settings: DM_SOLVE,
            },
        },
    };
    let solved = execute(&request).map_err(library_failure)?;
    let number = |path: &[&str]| {
        path.iter()
            .try_fold(&solved, |node, key| node.get(key))
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite())
            .ok_or_else(|| invalid("incomplete library response"))
    };
    Ok(FloatingQuote {
        dirty: number(&["data", "valuation", "dirty_per100"])?,
        accrued: number(&["data", "valuation", "accrued_per100"])?,
        discount_margin_bps: number(&["data", "decimal_parameter"])? * 1e4,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::ports::{
        fixing::PublishedFixing,
        reference_data::FloatingRateTerms,
        reference_master::{
            FloatingConventions, FloatingMechanics, MasterRecord, RateIndexDefinition,
            ReferencePin, ResolvedFloating,
        },
        reference_repository::ReferenceRecord,
    };
    use convex_analytics::workflow::{
        solve_bond_spread, BondDiscountRole, PreparedBondCash, PublicationSchedule, SuppliedMarket,
    };
    use convex_core::{
        contracts::{DecimalRate, ExactDecimal, SchemaVersion, Version},
        fixings::FixingSeries,
        ids::FloatingRateIndex,
    };
    use convex_curves::contracts::{build_curve_set, CurveSet};
    use std::str::FromStr;

    const AS_OF: &str = "2026-10-02";
    const SETTLEMENT: &str = "2026-10-06";
    const KNOWN_AT: &str = "2026-10-02T15:00:00Z";

    fn record(definition: MasterDefinition) -> MasterRecord {
        MasterRecord {
            schema_version: SchemaVersion::V2,
            version: Version::new(1).unwrap(),
            recorded_at: UtcTimestamp::parse("2025-01-01T00:00:00Z").unwrap(),
            definition,
        }
    }
    /// Fictional quarterly SOFR note: 45 bp over the index, paying two business days late.
    fn frn() -> BondReferenceData {
        let mut bond: BondReferenceData = serde_json::from_str(include_str!(
            "../../convex-ports/tests/fixtures/reference_bond.json"
        ))
        .unwrap();
        bond.bond_type = BondType::FloatingRate;
        bond.coupon_rate = None;
        bond.frequency = 4;
        bond.day_count = "ACT/360".into();
        bond.floating_terms = Some(FloatingRateTerms {
            index: FloatingRateIndex::Sofr,
            spread: Decimal::from(45),
            reset_frequency: 4,
            cap: None,
            floor: None,
        });
        let terms = bond.contractual_terms.as_mut().unwrap();
        terms.settings.day_count = convex_core::daycounts::DayCountConvention::Act360;
        terms.payment_lag = 2;
        terms.floating_mechanics = Some(FloatingMechanics {
            index: ReferencePin {
                id: ReferenceDataId::new("usd-sofr").unwrap(),
                version: Version::new(1).unwrap(),
            },
            overrides: FloatingConventions::default(),
        });
        bond
    }
    /// Fictional administrator rule: published one index business day later, 13:00 UTC.
    fn index(conventions: FloatingConventions) -> RateIndexDefinition {
        let terms = frn().contractual_terms.unwrap();
        RateIndexDefinition {
            rate_index_id: ReferenceDataId::new("usd-sofr").unwrap(),
            index_id: IndexId::new("USD-SOFR").unwrap(),
            currency: convex_core::Currency::USD,
            kind: RateIndexKind::Overnight,
            fixing_source: SourceId::new("fixture-administrator").unwrap(),
            calendar: terms.calendar.clone(),
            calendar_code: terms.calendar_code,
            publication_lag_business_days: 1,
            publication_utc_seconds: 13 * 3600,
            conventions,
            effective: terms.effective,
            provenance: terms.provenance,
        }
    }
    fn sofr() -> FloatingConventions {
        FloatingConventions {
            lookback_business_days: Some(2),
            observation_shift: Some(false),
            lockout_business_days: Some(0),
            ..FloatingConventions::default()
        }
    }
    fn reference(bond: BondReferenceData, index: RateIndexDefinition) -> ResolvedBondReference {
        let resources = crate::reference_terms::resources(&bond)
            .into_iter()
            .map(|change| record(change.definition))
            .collect::<Vec<_>>();
        ResolvedBondReference {
            record: ReferenceRecord {
                schema_version: SchemaVersion::V2,
                instrument_id: bond.instrument_id.clone(),
                version: Version::new(1).unwrap(),
                recorded_at: UtcTimestamp::parse("2025-01-01T00:00:00Z").unwrap(),
                bond: Some(bond),
            },
            issuer: resources[0].clone(),
            calendar: resources[1].clone(),
            convention: resources[2].clone(),
            floating: Some(ResolvedFloating {
                index: record(MasterDefinition::RateIndex(Box::new(index))),
                calendar: resources[1].clone(),
            }),
        }
    }
    fn curve(id: &str, role: serde_json::Value, rate: f64) -> CurveRecipe {
        serde_json::from_value(serde_json::json!({
            "key":{"id":id,"version":"supplied.recipe.v1"},
            "reference_date":AS_OF, "currency":"USD", "time_basis":"Act365Fixed",
            "role":role, "representation":{"ZeroRates":{"compounding":"Continuous"}},
            "interpolation":"Linear", "extrapolation":"Flat", "dependencies":[],
            "calibration":"Direct", "source":{"Nodes":[
                {"pillar":{"Tenor":0.0},"quote":{"unit":"DecimalRate","value":rate}},
                {"pillar":{"Tenor":50.0},"quote":{"unit":"DecimalRate","value":rate}}
            ]}
        }))
        .unwrap()
    }
    fn curves() -> Vec<CurveRecipe> {
        vec![
            curve(
                "USD-OIS",
                serde_json::json!({"kind":"Discount","collateral_currency":"USD"}),
                0.039,
            ),
            curve(
                "USD-SOFR-PROJECTION",
                serde_json::json!({"kind":"Projection","index_id":"USD-SOFR"}),
                0.040,
            ),
        ]
    }
    fn date(text: &str) -> Date {
        Date::parse(text).unwrap()
    }
    /// Every due observation at 4.30%, published and recorded on the rule's schedule.
    fn fixings(note: &FloatingNote) -> Vec<SnapshotFixing> {
        let known_at = UtcTimestamp::parse(KNOWN_AT).unwrap();
        let published = note
            .requirements(date(AS_OF))
            .unwrap()
            .into_iter()
            .map(|r| (r.observation_date, r.expected_publication))
            .collect::<std::collections::BTreeMap<_, _>>();
        note.due_fixings(date(AS_OF), known_at)
            .unwrap()
            .into_iter()
            .map(|key| {
                let at = published[&key.observation_date.date()];
                SnapshotFixing {
                    fixing: Some(PublishedFixing {
                        schema_version: 1,
                        key: key.clone(),
                        version: Version::new(1).unwrap(),
                        rate: DecimalRate::new(ExactDecimal::new(
                            Decimal::from_str("0.043").unwrap(),
                        )),
                        published_at: at,
                        recorded_at: at,
                        is_preliminary: false,
                        entitled: true,
                        evidence_reference: "fictional".into(),
                    }),
                    key,
                }
            })
            .collect()
    }
    fn quote(
        note: &FloatingNote,
        fixings: &[SnapshotFixing],
        clean: &str,
    ) -> Result<FloatingQuote, Failure> {
        let recipes = curves();
        value_floating(
            note,
            &recipes,
            &recipes[0].key,
            &recipes[1].key,
            date(SETTLEMENT),
            date(AS_OF),
            UtcTimestamp::parse(KNOWN_AT).unwrap(),
            fixings,
            Decimal::from_str(clean).unwrap(),
        )
    }

    #[test]
    fn the_library_request_matches_the_native_workflow_on_named_calendars() {
        let note = FloatingNote::new(&reference(frn(), index(sofr()))).unwrap();
        let fixings = fixings(&note);
        let ours = quote(&note, &fixings, "100.10").unwrap();
        // The same terms on the named SIFMA calendar, rule and curves, without the library.
        let sifma = convex_bonds::types::CalendarId::sifma()
            .try_to_calendar()
            .unwrap();
        let contract = note
            .terms
            .clone()
            .build_with_calendars(&BondCalendars {
                payment: sifma.as_ref(),
                reset: sifma.as_ref(),
                observation: sifma.as_ref(),
                exercise: sifma.as_ref(),
            })
            .unwrap();
        let publications = contract
            .fixing_requirements(
                date(SETTLEMENT),
                PaymentDateEntitlement::Exclude,
                |_, observed| {
                    let day = sifma.add_business_days(observed, 1);
                    Ok(UtcTimestamp::parse(&format!("{day}T13:00:00Z")).unwrap())
                },
            )
            .unwrap();
        let recipes = curves();
        let built = build_curve_set(&recipes, &CurveSet::new()).unwrap().curves;
        let history = FixingSeries::new(
            fixings
                .iter()
                .map(|f| crate::core_workflow::fixing_observation(f.fixing.as_ref().unwrap()))
                .collect(),
        )
        .unwrap();
        let schedule = PublicationSchedule::new(publications).unwrap();
        let context = BondProjectionContext {
            settlement: date(SETTLEMENT),
            on_payment: PaymentDateEntitlement::Exclude,
            fixing: FixingContext {
                valuation_date: date(AS_OF),
                known_at: UtcTimestamp::parse(KNOWN_AT).unwrap(),
                unpublished: UnpublishedFixingPolicy::Forecast,
                final_only: true,
            },
            overnight_accrual_cutoffs: vec![],
        };
        let cash = PreparedBondCash::floating(
            &contract,
            &context,
            &SuppliedMarket {
                curves: &built,
                fixings: &history,
                publications: &schedule,
            },
            Some(&recipes[1].key),
        )
        .unwrap();
        let native = solve_bond_spread(
            &cash,
            built.get(&recipes[0].key).unwrap().as_ref(),
            BondDiscountRole::Collateral,
            BondPriceTarget::Clean(100.10),
            DM_SOLVE,
        )
        .unwrap();
        assert!((ours.discount_margin_bps - native.decimal_parameter * 1e4).abs() < 1e-9);
        assert!((ours.dirty - native.valuation.dirty_per100).abs() < 1e-12);
        assert!((ours.accrued - native.valuation.accrued_per100).abs() < 1e-12);
        // The 45 bp margin plus the 10 bp projection-over-discount basis, less the 0.10
        // premium spread over the remaining life.
        assert!(
            (45.0..55.0).contains(&ours.discount_margin_bps),
            "{}",
            ours.discount_margin_bps
        );
    }

    #[test]
    fn mechanics_come_from_the_bond_then_the_index_and_missing_ones_are_named() {
        // The bond's own lookback overrides the index's.
        let mut bond = frn();
        let terms = bond.contractual_terms.as_mut().unwrap();
        terms
            .floating_mechanics
            .as_mut()
            .unwrap()
            .overrides
            .lookback_business_days = Some(5);
        let note = FloatingNote::new(&reference(bond, index(sofr()))).unwrap();
        let BondCoupon::Overnight(coupon) = &note.terms.coupon else {
            panic!("overnight coupon")
        };
        assert_eq!(coupon.convention.lookback_business_days, 5);
        assert_eq!(coupon.convention.lockout_business_days, 0);
        // Neither the bond nor its index declares a lockout.
        let declared = FloatingConventions {
            lockout_business_days: None,
            ..sofr()
        };
        let error = FloatingNote::new(&reference(frn(), index(declared)))
            .err()
            .unwrap();
        assert_eq!(error.code, ErrorCode::UnsupportedConvention);
        assert!(
            error.message.contains("floating lockout"),
            "{}",
            error.message
        );
        // Observation shift and caps are refused with their reasons.
        let shifted = FloatingConventions {
            observation_shift: Some(true),
            ..sofr()
        };
        let error = FloatingNote::new(&reference(frn(), index(shifted)))
            .err()
            .unwrap();
        assert!(
            error.message.contains("observation-shifted"),
            "{}",
            error.message
        );
        let mut capped = frn();
        capped.floating_terms.as_mut().unwrap().cap = Some(Decimal::from_str("0.08").unwrap());
        let error = FloatingNote::new(&reference(capped, index(sofr())))
            .err()
            .unwrap();
        assert!(
            error.message.contains("caps and floors"),
            "{}",
            error.message
        );
    }

    #[test]
    fn due_fixings_follow_the_publication_rule_and_missing_ones_fail_the_note() {
        let note = FloatingNote::new(&reference(frn(), index(sofr()))).unwrap();
        let known_at = UtcTimestamp::parse(KNOWN_AT).unwrap();
        let due = note.due_fixings(date(AS_OF), known_at).unwrap();
        // Thursday is published Friday 13:00, before the 15:00 cutoff. Friday itself is
        // not due until Monday, so it is forecast.
        assert_eq!(
            due.last().unwrap().observation_date.date(),
            date("2026-10-01")
        );
        assert!(due
            .iter()
            .all(|k| k.source.as_str() == "fixture-administrator"));
        // The current period from 15 July, looked back two business days.
        assert_eq!(
            due.first().unwrap().observation_date.date(),
            date("2026-07-13")
        );
        let mut fixings = fixings(&note);
        fixings.pop();
        let error = quote(&note, &fixings, "100.10").err().unwrap();
        assert_eq!(
            error.code,
            ErrorCode::MissingMarketData,
            "{}",
            error.message
        );
    }
}
