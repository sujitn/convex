//! Offline supplied synthetic resources shared by the standalone example/tests.
#![allow(dead_code)]

use convex_analytics::{
    futures::{FuturesCalendar, FuturesFixingSelection, FuturesFixings},
    rate_calibration::*,
};
use convex_core::{
    calendars::{Calendar, DynamicCalendar, WeekendType},
    cashflows::{CashflowError, PaymentDateEntitlement},
    contracts::{ExactDecimal, IndexId, UtcTimestamp, Version},
    fixings::{FixingContext, FixingObservation, FixingSeries, UnpublishedFixingPolicy},
    futures::ReferenceStamp,
    Date,
};
use convex_curves::{contracts::*, InterpolationMethod};
use convex_rates::{
    basis::{BasisLegCalendars, BasisSwapCalendars},
    CalendarBinding, CashflowContext, FloatingLegTerms, LegCalendarPins, LegCalendars,
    ReferencePin, SwapCalendars, SwapTerms,
};
use serde_json::Value;

pub type Error = Box<dyn std::error::Error>;
pub struct Fixture {
    pub data: Value,
    pub row: Value,
    pub recipes: Vec<RateCurveRecipe>,
    pub calendar: DynamicCalendar,
    pub calendar_stamp: ReferenceStamp,
    pub observations: Vec<FixingObservation>,
    pub history: FixingSeries,
    pub futures_history: FuturesFixings,
}
pub fn text<'a>(v: &'a Value, field: &str) -> Result<&'a str, Error> {
    v[field]
        .as_str()
        .ok_or_else(|| format!("missing supplied {field}").into())
}
impl Fixture {
    pub fn load(id: &str) -> Result<Self, Error> {
        let data: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/curves/mixed-rates.json"
        ))?;
        let row = data["cases"]
            .as_array()
            .ok_or("missing reference cases")?
            .iter()
            .find(|c| c["id"] == id)
            .ok_or("unknown supplied case")?
            .clone();
        let recipes = serde_json::from_value(row["recipes"].clone())?;
        let mut calendar = DynamicCalendar::new(
            "explicit synthetic CVX-057 calendar",
            WeekendType::SaturdaySunday,
        );
        calendar.add_holidays(
            data["holidays"]
                .as_array()
                .ok_or("missing holidays")?
                .iter()
                .map(|d| {
                    d.as_str()
                        .ok_or("invalid date")
                        .map_err(Error::from)
                        .and_then(|s| Date::parse(s).map_err(Error::from))
                })
                .collect::<Result<Vec<_>, _>>()?,
        );
        let calendar_stamp = serde_json::from_value(data["calendar_stamp"].clone())?;
        let observations = row["history"]
            .as_array()
            .ok_or("missing explicit history")?
            .iter()
            .map(|o| {
                Ok(FixingObservation {
                    index_id: IndexId::new(text(o, "index_id")?)?,
                    observation_date: Date::parse(text(o, "observation_date")?)?,
                    rate: ExactDecimal::parse(text(o, "rate")?)?.decimal(),
                    published_at: UtcTimestamp::parse(text(o, "published_at")?)?,
                    known_at: UtcTimestamp::parse(text(o, "known_at")?)?,
                    revision: Version::new(o["revision"].as_u64().ok_or("missing revision")?)?,
                    status: serde_json::from_value(o["status"].clone())?,
                    source_id: text(o, "source_id")?.to_string().try_into()?,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        let history = FixingSeries::new(observations.clone())?;
        let futures_history =
            FuturesFixings::new(observations.clone(), FuturesFixingSelection::FirstPublished)?;
        Ok(Self {
            data,
            row,
            recipes,
            calendar,
            calendar_stamp,
            observations,
            history,
            futures_history,
        })
    }
    pub fn context(&self) -> Result<RateCalibrationContext, Error> {
        let as_of = UtcTimestamp::parse(text(&self.data, "as_of")?)?;
        Ok(RateCalibrationContext {
            as_of,
            cashflows: CashflowContext {
                fixings: FixingContext {
                    valuation_date: Date::parse(text(&self.data, "reference_date")?)?,
                    known_at: as_of,
                    unpublished: UnpublishedFixingPolicy::Forecast,
                    final_only: true,
                },
                on_payment: PaymentDateEntitlement::Include,
            },
        })
    }
    pub fn binding<'a>(&'a self, pin: &'a ReferencePin) -> CalendarBinding<'a> {
        CalendarBinding {
            pin,
            calendar: &self.calendar,
        }
    }
    pub fn leg<'a>(&'a self, pins: &'a LegCalendarPins) -> LegCalendars<'a> {
        LegCalendars {
            accrual: self.binding(&pins.accrual),
            payment: self.binding(&pins.payment),
        }
    }
    pub fn float_calendars<'a>(&'a self, t: &'a FloatingLegTerms) -> BasisLegCalendars<'a> {
        match t {
            FloatingLegTerms::Term(t) => BasisLegCalendars {
                leg: self.leg(&t.leg.calendars),
                reset: Some(self.binding(&t.resets.reset_calendar)),
                fixing: Some(self.binding(&t.resets.fixing_calendar)),
                observation: None,
            },
            FloatingLegTerms::Overnight(t) => BasisLegCalendars {
                leg: self.leg(&t.leg.calendars),
                reset: None,
                fixing: None,
                observation: Some(self.binding(&t.observation_calendar)),
            },
        }
    }
    pub fn swap_calendars<'a>(&'a self, t: &'a SwapTerms) -> SwapCalendars<'a> {
        let f = self.float_calendars(&t.floating);
        SwapCalendars {
            fixed: self.leg(&t.fixed.leg.calendars),
            floating: f.leg,
            reset: f.reset,
            fixing: f.fixing,
            observation: f.observation,
        }
    }
    pub fn resources<'a>(
        &'a self,
        recipes: &'a [RateCurveRecipe],
    ) -> Vec<Vec<RateHelperResources<'a>>> {
        recipes
            .iter()
            .map(|r| {
                r.helpers
                    .iter()
                    .map(|h| match &h.terms {
                        RateCalibrationTerms::MoneyMarket(t) => {
                            RateHelperResources::MoneyMarket(self.binding(&t.calendar))
                        }
                        RateCalibrationTerms::Swap(t) => {
                            RateHelperResources::Swap(self.swap_calendars(t))
                        }
                        RateCalibrationTerms::BasisSwap { terms, .. } => {
                            RateHelperResources::BasisSwap(BasisSwapCalendars {
                                pay: self.float_calendars(&terms.pay),
                                receive: self.float_calendars(&terms.receive),
                            })
                        }
                        RateCalibrationTerms::RateFuture(_) => RateHelperResources::RateFuture {
                            calendar: FuturesCalendar {
                                stamp: &self.calendar_stamp,
                                calendar: &self.calendar,
                            },
                            fixings: &self.futures_history,
                        },
                    })
                    .collect()
            })
            .collect()
    }
    pub fn publication(&self, _: &IndexId, at: Date) -> Result<UtcTimestamp, CashflowError> {
        let mut next = at;
        for _ in 0..3660 {
            next = next
                .checked_add_days(1)
                .map_err(|e| CashflowError::Date(e.to_string()))?;
            if self.calendar.is_business_day(next) {
                return UtcTimestamp::parse(&format!("{next}T08:00:00Z"))
                    .map_err(|e| CashflowError::Date(e.to_string()));
            }
        }
        Err(CashflowError::BoundExceeded(
            "synthetic fixture publication",
        ))
    }
    pub fn fit(&self, recipes: &[RateCurveRecipe]) -> Result<RateCurveBuildOutput, Error> {
        let resources = self.resources(recipes);
        let inputs: Vec<_> = recipes
            .iter()
            .zip(&resources)
            .map(|(recipe, resources)| RateCurveBuildInput { recipe, resources })
            .collect();
        Ok(build_rate_curve_set(
            &inputs,
            &CurveSet::new(),
            &self.history,
            self.context()?,
            FitNumericalControls::default(),
            |i, d| self.publication(i, d),
        )?)
    }
    pub fn reference_curves(&self) -> Result<CurveSet, Error> {
        let mut recipes = Vec::new();
        for original in &self.recipes {
            let mut r = original.curve.clone();
            let zeros: Vec<f64> = self.row["expected_zeros"][&r.key.id]
                .as_array()
                .ok_or("missing zeros")?
                .iter()
                .map(|v| {
                    v.as_str()
                        .ok_or("missing reference zero")
                        .map_err(Error::from)
                        .and_then(|s| s.parse().map_err(Error::from))
                })
                .collect::<Result<_, _>>()?;
            r.dependencies.clear();
            r.calibration = CalibrationMode::Direct;
            r.source = CurveSource::Nodes(
                std::iter::once((r.reference_date, zeros[0]))
                    .chain(original.pillars.iter().copied().zip(zeros))
                    .map(|(date, value)| CurveNode {
                        pillar: CurvePoint::Date(date),
                        quote: MarketQuote {
                            value,
                            unit: QuoteUnit::DecimalRate,
                        },
                    })
                    .collect(),
            );
            recipes.push(r);
        }
        let curves = build_curve_set(&recipes, &CurveSet::new())?.curves;
        let d = &self.recipes[0];
        let p = d
            .overnight_projection
            .as_ref()
            .ok_or("missing explicit OIS view")?;
        let mut alias = d.curve.clone();
        alias.key = p.key.clone();
        alias.role = CurveRole::Projection {
            index_id: p.index_id.clone(),
        };
        alias.dependencies = vec![d.curve.key.clone()];
        alias.calibration = CalibrationMode::Direct;
        alias.source = CurveSource::OvernightProjection {
            discount: d.curve.key.clone(),
            index_id: p.index_id.clone(),
        };
        Ok(build_curve_set(&[alias], &curves)?.curves)
    }
    pub fn global(&self, interpolation: InterpolationMethod) -> Vec<RateCurveRecipe> {
        let mut recipes = self.recipes.clone();
        for r in &mut recipes {
            r.bootstrap = None;
            r.curve.interpolation = interpolation;
            r.curve.calibration = CalibrationMode::Global(GlobalFitSettings {
                max_iterations: 100,
                objective_tolerance: 1e-12,
                initial_zeros: vec![0.02; r.pillars.len()],
                lower_zero: -0.08,
                upper_zero: 0.15,
                quote_scales: r
                    .helpers
                    .iter()
                    .map(|h| match h.quote.unit {
                        QuoteUnit::PercentRate | QuoteUnit::FuturePrice100MinusRate => 100.0,
                        QuoteUnit::BasisPoints => 10000.0,
                        _ => 1.0,
                    })
                    .collect(),
                weights: vec![1.0; r.helpers.len()],
            });
        }
        recipes
    }
}
pub fn compare_reference(f: &Fixture, built: &RateCurveBuildOutput) -> Result<(), Error> {
    for c in &built.calibrations {
        let expected = f.row["expected_zeros"][&c.recipe.curve.key.id]
            .as_array()
            .ok_or("missing zeros")?;
        let (_, zeros) = c.curve.interpolation_grid()?;
        for (value, expected) in zeros[1..].iter().zip(expected) {
            let expected: f64 = expected.as_str().ok_or("missing zero")?.parse()?;
            if (value - expected).abs() > 1e-9 + 1e-10 * expected.abs() {
                return Err(format!("independent zero differs: {value} vs {expected}").into());
            }
        }
        for q in &c.curve.diagnostics().quotes {
            if !q.residual.is_finite() || q.residual.abs() > q.tolerance {
                return Err("original native repricing failed".into());
            }
        }
    }
    Ok(())
}
