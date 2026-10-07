//! Complete owned economics and immutable supplied resource values.
use super::*;
use convex_bonds::contracts::{BondCalendarIds, BondCalendars, BondTerms};
use convex_core::{calendars::Calendar, contracts::ReferenceDataId, fixings::*};
use convex_rates::{CalendarBinding, LegCalendars, ReferencePin, SwapCalendars};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Actual finite holiday table and explicitly selected weekend days (Monday=0).
/// Identity/version is a caller supplied reference pin, never a preset lookup.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CalendarInput {
    /// Exact internal reference identity and revision.
    pub pin: ReferencePin,
    /// Complete known holiday dates; duplicates and unsorted input fail.
    pub holidays: Vec<Date>,
    /// Explicit weekday numbers, strictly increasing in 0..=6.
    pub weekend_days: Vec<u8>,
}
impl Calendar for CalendarInput {
    fn name(&self) -> &'static str {
        "SuppliedLibraryCalendar"
    }
    fn is_business_day(&self, date: Date) -> bool {
        !self
            .weekend_days
            .contains(&(date.weekday().num_days_from_monday() as u8))
            && self.holidays.binary_search(&date).is_err()
    }
}
/// Four independent exact calendar revisions for full bond construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BondCalendarPins {
    /// Accrual, payment and principal calendar.
    pub payment: ReferencePin,
    /// Independent reset calendar.
    pub reset: ReferencePin,
    /// Overnight observation calendar.
    pub observation: ReferencePin,
    /// Notice/exercise calendar.
    pub exercise: ReferencePin,
}
/// Lossless native terms with the calendar versions absent from the old BondSpec.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BondInput {
    /// Original full profiles, reset, principal, exercise and quotation data.
    pub terms: BondTerms,
    /// Actual independent calendar revisions, checked against original names.
    pub calendars: BondCalendarPins,
}
/// Full supplied market values. No object registry, ID lookup outside this bundle
/// or default publication rule is available to the pure request API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct LibraryInputs {
    /// Complete explicitly versioned holiday tables.
    pub calendars: Vec<CalendarInput>,
    /// Exact opaque helper revisions bound explicitly to actual calendar values.
    /// Older helper contracts use string revisions; no numeric version is guessed.
    #[serde(default)]
    pub helper_calendars: Vec<HelperCalendarInput>,
    /// Full checked native recipes including dependencies, methods and bounds.
    pub curves: Vec<CurveRecipe>,
    /// Optional complete original product calibration graph, built locally on
    /// supplied direct dependencies before any operation. Cached state is not decoded.
    #[serde(default)]
    pub calibration: Option<Box<super::CalibrationInput>>,
    /// Every actual supplied revision; no forecast is substituted for due history.
    pub fixings: Vec<FixingObservation>,
    /// Exact administrator publication decisions per index/observation date.
    pub publications: Vec<FixingRequirement>,
}
impl LibraryInputs {
    /// Explicit empty resources for operations which do not need market data.
    pub fn empty() -> Self {
        Self {
            calendars: vec![],
            helper_calendars: vec![],
            curves: vec![],
            calibration: None,
            fixings: vec![],
            publications: vec![],
        }
    }
}
#[derive(Clone)]
pub(super) struct ResolvedInputs<'a> {
    input: &'a LibraryInputs,
    calendars: BTreeMap<(ReferenceDataId, convex_core::contracts::Version), &'a CalendarInput>,
    pub curves: CurveSet,
    pub calibration: Option<Value>,
    pub history: FixingSeries,
    pub publications: convex_analytics::workflow::PublicationSchedule,
}
impl<'a> ResolvedInputs<'a> {
    pub fn new(input: &'a LibraryInputs) -> LibraryResult<Self> {
        if input.calendars.len() > 256 || input.curves.len() > 256 {
            return Err(LibraryError::limit("at most 256 calendars/curves"));
        }
        let mut calendars = BTreeMap::new();
        for c in &input.calendars {
            if c.holidays.len() > 4096
                || c.weekend_days.len() > 6
                || c.weekend_days.iter().any(|d| *d > 6)
                || c.weekend_days.windows(2).any(|p| p[0] >= p[1])
                || c.holidays.windows(2).any(|p| p[0] >= p[1])
                || c.holidays.iter().any(|d| !(1..=9999).contains(&d.year()))
            {
                return Err(LibraryError::invalid(
                    "calendar",
                    "bounded strictly sorted actual dates and weekday numbers required",
                ));
            }
            if calendars
                .insert((c.pin.id.clone(), c.pin.version), c)
                .is_some()
            {
                return Err(LibraryError::invalid(
                    "calendar",
                    "duplicate exact revision",
                ));
            }
        }
        // Every native recipe remains whole; no tenor-only helper substitution.
        let curves = if input.curves.is_empty() {
            CurveSet::new()
        } else {
            build_curve_set(&input.curves, &CurveSet::new())?.curves
        };
        let history = FixingSeries::new(input.fixings.clone())?;
        let publications =
            convex_analytics::workflow::PublicationSchedule::new(input.publications.clone())?;
        let mut resolved = Self {
            input,
            calendars,
            curves,
            calibration: None,
            history,
            publications,
        };
        if let Some(calibration) = &input.calibration {
            let (curves, diagnostics) = super::curves::calibration_market(calibration, &resolved)?;
            resolved.curves = curves;
            resolved.calibration = Some(diagnostics);
        }
        Ok(resolved)
    }
    pub fn calendar(&self, pin: &ReferencePin) -> LibraryResult<&'a dyn Calendar> {
        self.calendars
            .get(&(pin.id.clone(), pin.version))
            .map(|c| *c as &dyn Calendar)
            .ok_or_else(|| LibraryError::missing("calendar", serde_json::json!(pin)))
    }
    pub fn bond_calendars(&self, input: &BondInput) -> LibraryResult<BondCalendars<'a>> {
        let BondCalendarIds {
            payment,
            reset,
            observation,
            exercise,
        } = &input.terms.calendars;
        let p = &input.calendars;
        for (id, pin) in [
            (payment, &p.payment),
            (reset, &p.reset),
            (observation, &p.observation),
            (exercise, &p.exercise),
        ] {
            if id.as_str() != pin.id.as_str() {
                return Err(LibraryError::invalid(
                    "bond calendar",
                    "native name and supplied exact pin differ",
                ));
            }
        }
        Ok(BondCalendars {
            payment: self.calendar(&p.payment)?,
            reset: self.calendar(&p.reset)?,
            observation: self.calendar(&p.observation)?,
            exercise: self.calendar(&p.exercise)?,
        })
    }
    pub fn bond(&self, input: &BondInput) -> LibraryResult<convex_bonds::contracts::BondContract> {
        Ok(input
            .terms
            .clone()
            .build_with_calendars(&self.bond_calendars(input)?)?)
    }
    pub fn rate_binding<'b>(&self, pin: &'b ReferencePin) -> LibraryResult<CalendarBinding<'b>>
    where
        'a: 'b,
    {
        Ok(CalendarBinding {
            pin,
            calendar: self.calendar(pin)?,
        })
    }
    pub fn leg_calendars<'b>(
        &self,
        pins: &'b convex_rates::LegCalendarPins,
    ) -> LibraryResult<LegCalendars<'b>>
    where
        'a: 'b,
    {
        Ok(LegCalendars {
            accrual: self.rate_binding(&pins.accrual)?,
            payment: self.rate_binding(&pins.payment)?,
        })
    }
    pub fn floating_calendars<'b>(
        &self,
        terms: &'b convex_rates::FloatingLegTerms,
    ) -> LibraryResult<convex_rates::basis::BasisLegCalendars<'b>>
    where
        'a: 'b,
    {
        let leg = self.leg_calendars(&terms.leg().calendars)?;
        Ok(match terms {
            convex_rates::FloatingLegTerms::Term(t) => convex_rates::basis::BasisLegCalendars {
                leg,
                reset: Some(self.rate_binding(&t.resets.reset_calendar)?),
                fixing: Some(self.rate_binding(&t.resets.fixing_calendar)?),
                observation: None,
            },
            convex_rates::FloatingLegTerms::Overnight(t) => {
                convex_rates::basis::BasisLegCalendars {
                    leg,
                    reset: None,
                    fixing: None,
                    observation: Some(self.rate_binding(&t.observation_calendar)?),
                }
            }
        })
    }
    pub fn swap_calendars<'b>(
        &self,
        terms: &'b convex_rates::SwapTerms,
    ) -> LibraryResult<SwapCalendars<'b>>
    where
        'a: 'b,
    {
        let f = self.floating_calendars(&terms.floating)?;
        Ok(SwapCalendars {
            fixed: self.leg_calendars(&terms.fixed.leg.calendars)?,
            floating: f.leg,
            reset: f.reset,
            fixing: f.fixing,
            observation: f.observation,
        })
    }
    pub fn basis_calendars<'b>(
        &self,
        terms: &'b convex_rates::basis::BasisSwapTerms,
    ) -> LibraryResult<convex_rates::basis::BasisSwapCalendars<'b>>
    where
        'a: 'b,
    {
        Ok(convex_rates::basis::BasisSwapCalendars {
            pay: self.floating_calendars(&terms.pay)?,
            receive: self.floating_calendars(&terms.receive)?,
        })
    }
    pub fn credit_calendars<'b>(
        &self,
        terms: &'b convex_credit::CdsTerms,
    ) -> LibraryResult<convex_credit::CdsCalendars<'b>>
    where
        'a: 'b,
    {
        let p = &terms.premium_calendar;
        let s = &terms.settlement_calendar;
        let rate = |p: &convex_credit::ReferencePin| ReferencePin {
            id: p.id.clone(),
            version: p.version,
        };
        Ok(convex_credit::CdsCalendars {
            premium: convex_credit::CalendarBinding {
                pin: p,
                calendar: self.calendar(&rate(p))?,
            },
            settlement: convex_credit::CalendarBinding {
                pin: s,
                calendar: self.calendar(&rate(s))?,
            },
        })
    }
    pub fn market(&self) -> convex_analytics::workflow::SuppliedMarket<'_> {
        convex_analytics::workflow::SuppliedMarket {
            curves: &self.curves,
            fixings: &self.history,
            publications: &self.publications,
        }
    }
    pub fn fixing_observations(&self) -> &[FixingObservation] {
        &self.input.fixings
    }
    pub fn input(&self) -> &LibraryInputs {
        self.input
    }
    pub fn with_curves(&self, curves: CurveSet) -> Self {
        Self {
            curves,
            ..self.clone()
        }
    }
}
