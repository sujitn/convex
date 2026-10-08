//! Native helper inspection/quote conversion and full dated repo financing.
use super::inputs::ResolvedInputs;
use super::*;
use convex_analytics::{cash_calibration as c, credit_calibration as d, curve_inputs as h};
use serde::{Deserialize, Serialize};

/// Explicit original opaque helper pin and the supplied exact calendar it uses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HelperCalendarInput {
    /// Full original native helper identity/revision.
    pub pin: h::ReferencePin,
    /// Exact calendar values in LibraryInputs.calendars.
    pub calendar: convex_rates::ReferencePin,
}

/// Native helper operations keep original quotes, full instruments and guards.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HelperInput {
    /// Complete original government/issuer-basis/repo diagnostics; no fit.
    CashReprice {
        /// Full native recipe and exact source-order calendar revisions.
        input: Box<CashCalibrationInput>,
    },
    /// Full original CDS-helper diagnostics on exact supplied curves; no fit.
    CreditReprice {
        /// Complete original legal, quote, calendar, cash-state and curve terms.
        recipe: Box<d::CdsCurveRecipe>,
    },
    /// Original rate-helper diagnostics on exact supplied curves; no curve fit.
    RateReprice {
        /// Full native recipe, original quote revisions and futures source pins.
        input: Box<RateCalibrationInput>,
        /// Actual economic and publication/history eligibility instant.
        context: CalibrationContext,
    },
    /// Original checked helper requirements/pillar/capability, including pending models.
    Describe {
        /// Full original helper terms.
        helper: h::CurveHelperInput,
        /// Actual supplied valuation origin.
        reference_date: Date,
    },
    /// Original full helper composition; unimplemented helpers fail explicitly.
    Build {
        /// Complete original recipes and helper terms.
        recipes: Vec<h::HelperCurveRecipe>,
    },
    /// Full clean/dirty/yield/par conversion without a fabricated market curve.
    CashQuote {
        /// Complete original security/quote/settlement.
        helper: c::CashBondHelper,
        /// Exact original independent calendars.
        calendars: BondCalendarPins,
        /// Original supplied information date.
        reference_date: Date,
    },
    /// Original issuer residual-basis pricing with independent credit/reference data.
    BasisBond {
        /// Full original security and quote.
        helper: c::CashBondHelper,
        /// Independent exact security calendars.
        calendars: BondCalendarPins,
        /// Original residual basis model and meaning.
        model: c::BondBasisModel,
        /// Actual separately built basis curve.
        basis: CurveKey,
    },
    /// Actual repo-role/collateral funding over independently dated cash periods.
    RepoRate {
        /// Full original funding identity/collateral.
        funding: c::RepoFundingCurve,
        /// Original currency.
        currency: Currency,
        /// Actual cash start.
        start: Date,
        /// Actual cash end.
        end: Date,
        /// Explicit rate quotation.
        day_count: convex_core::daycounts::DayCountConvention,
    },
}
fn resources<'a>(r: &'a ResolvedInputs<'_>) -> LibraryResult<h::HelperResources<'a>> {
    if r.input().helper_calendars.len() > 256 {
        return Err(LibraryError::limit(
            "at most256 original helper calendar bindings",
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    let calendars = r
        .input()
        .helper_calendars
        .iter()
        .map(|c| {
            if c.pin.id != c.calendar.id.as_str()
                || c.pin.version.is_empty()
                || !seen.insert((&c.pin.id, &c.pin.version))
            {
                return Err(LibraryError::invalid(
                    "helper calendar",
                    "explicit unique original revision and same calendar identity required",
                ));
            }
            Ok(h::CalendarBinding {
                pin: c.pin.clone(),
                calendar: r.calendar(&c.calendar)?,
            })
        })
        .collect::<LibraryResult<Vec<_>>>()?;
    Ok(h::HelperResources {
        calendars,
        fixings: &r.history,
    })
}
pub(super) fn helper(input: &HelperInput, r: &ResolvedInputs<'_>) -> LibraryResult<Value> {
    match input {
        HelperInput::CashReprice { input } => {
            let resources = super::curves::cash_resources(input, r)?;
            value(c::reprice_cash_helpers(
                &c::CashCurveBuildInput {
                    recipe: &input.recipe,
                    resources: &resources,
                },
                &r.curves,
            )?)
        }
        HelperInput::CreditReprice { recipe } => {
            if recipe.helpers.len() > d::MAX_CREDIT_FIT_HELPERS {
                return Err(LibraryError::limit("at most128 complete CDS helpers"));
            }
            let calendars = recipe
                .helpers
                .iter()
                .map(|h| r.credit_calendars(&h.terms))
                .collect::<LibraryResult<Vec<_>>>()?;
            value(d::reprice_cds_helpers(
                &d::CdsCurveBuildInput {
                    recipe,
                    calendars: &calendars,
                },
                &r.curves,
            )?)
        }
        HelperInput::RateReprice { input, context } => {
            super::curves::reprice_rate_helpers(input, context, r)
        }
        HelperInput::Describe {
            helper,
            reference_date,
        } => value(h::describe_helper(helper, *reference_date, &resources(r)?)?),
        HelperInput::Build { recipes } => {
            let out = h::build_helper_curve_set(recipes, &resources(r)?, &r.curves)?;
            Ok(
                serde_json::json!({"curves":super::curves::describe_curves(&out.curves)?,"build_order":out.build_order}),
            )
        }
        HelperInput::CashQuote {
            helper,
            calendars,
            reference_date,
        } => {
            let calendars = r.bond_calendars(&BondInput {
                terms: helper.terms.clone(),
                calendars: calendars.clone(),
            })?;
            value(c::convert_cash_quote(
                helper,
                &c::CashHelperResources::Bond {
                    ids: &helper.terms.calendars,
                    calendars,
                },
                *reference_date,
            )?)
        }
        HelperInput::BasisBond {
            helper,
            calendars,
            model,
            basis,
        } => {
            let calendars = r.bond_calendars(&BondInput {
                terms: helper.terms.clone(),
                calendars: calendars.clone(),
            })?;
            value(c::price_basis_bond(
                helper,
                &c::CashHelperResources::Bond {
                    ids: &helper.terms.calendars,
                    calendars,
                },
                model,
                r.curves.get(basis)?.as_ref(),
                &r.curves,
            )?)
        }
        HelperInput::RepoRate {
            funding,
            currency,
            start,
            end,
            day_count,
        } => value(c::repo_rate_over_period(
            &r.curves, funding, *currency, *start, *end, *day_count,
        )?),
    }
}
/// Exact original nominal-bond purchase and independent repo/haircut/coupon funding.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RepoForwardInput {
    /// Complete original native carry terms.
    pub terms: c::RepoBondForwardTerms,
    /// Actual independently pinned security calendars.
    pub calendars: BondCalendarPins,
}
pub(super) fn repo_forward(
    input: &RepoForwardInput,
    r: &ResolvedInputs<'_>,
) -> LibraryResult<Value> {
    let calendars = r.bond_calendars(&BondInput {
        terms: input.terms.spot.terms.clone(),
        calendars: input.calendars.clone(),
    })?;
    value(c::price_repo_bond_forward(&c::RepoBondForwardInputs {
        terms: &input.terms,
        resources: &c::CashHelperResources::Bond {
            ids: &input.terms.spot.terms.calendars,
            calendars,
        },
        delivery_calendar: r.rate_binding(&input.terms.delivery_calendar)?,
        curves: &r.curves,
    })?)
}
