//! Immutable term coupon model view with frozen original publication decisions.
use super::*;
use convex_curves::contracts::{CurveKey, CurveRecipe, CurveSet};

/// Original immutable coupon model and curve-role bindings for repeated pricing.
pub struct TermCouponModelInputs<'a> {
    /// Original complete checked economics/calendars.
    pub bond: &'a BondContract,
    /// Original settlement, entitlement and information cutoff.
    pub context: &'a BondProjectionContext,
    /// Original eligible revisions, never scenario-generated fixing history.
    pub history: &'a FixingSeries,
    /// Original segment or whole-coupon model law and source revisions.
    pub options: &'a TermCouponOptionInputs,
    /// Actual projection role, required only when forecasts are permitted.
    pub projection: Option<CurveKey>,
    /// Actual same-currency collateral discount role.
    pub discount: CurveKey,
}

/// Prepared baseline is private; every scenario revalues the complete native
/// coupon law against actual supplied scenario curves with the same history.
pub struct PreparedTermCouponModel<'a> {
    input: TermCouponModelInputs<'a>,
    publications: BTreeMap<(IndexId, Date), UtcTimestamp>,
    baseline: TermCouponOptionPrice,
    curve_recipes: Vec<CurveRecipe>,
    curve_grids: Vec<(Vec<f64>, Vec<f64>)>,
}

impl PreparedTermCouponModel<'_> {
    /// Actual original complete baseline model price and input proof.
    pub fn baseline(&self) -> &TermCouponOptionPrice {
        &self.baseline
    }
    /// Original face used once for positive native reference positions.
    pub fn face_value(&self) -> Decimal {
        self.input.bond.terms().face_value
    }
    /// Original native currency.
    pub fn currency(&self) -> convex_core::Currency {
        self.input.bond.terms().currency
    }
    /// Actual settlement boundary.
    pub fn settlement(&self) -> Date {
        self.input.context.settlement
    }
    /// Reject reuse against another curve information set before any risk call.
    pub fn admit_baseline(&self, curves: &CurveSet) -> Result<()> {
        for (recipe, (times, values)) in self.curve_recipes.iter().zip(&self.curve_grids) {
            let actual = curves.get(&recipe.key)?;
            let grid = actual.interpolation_grid()?;
            if actual.recipe() != recipe || grid.0 != times || grid.1 != values {
                return Err(invalid(
                    "coupon baseline",
                    "original full curve recipe differs",
                ));
            }
        }
        Ok(())
    }
    /// Reprice every actual reset/profile/fixing/option/principal component.
    /// Model laws and published history remain frozen; forward rates do not.
    pub fn price(&self, curves: &CurveSet) -> Result<TermCouponOptionPrice> {
        price_input(&self.input, &self.publications, curves)
    }
}

fn price_input(
    input: &TermCouponModelInputs<'_>,
    publications: &BTreeMap<(IndexId, Date), UtcTimestamp>,
    curves: &CurveSet,
) -> Result<TermCouponOptionPrice> {
    let discount = curves.get(&input.discount)?;
    let projection = input
        .projection
        .as_ref()
        .map(|key| curves.get(key))
        .transpose()?;
    if let Some(curve) = projection {
        let index = match &input.bond.terms().coupon {
            BondCoupon::Term(terms) => terms.index_id.as_str(),
            _ => return Err(invalid("projection", "actual term index required")),
        };
        if curve.recipe().currency != input.bond.terms().currency
            || !matches!(&curve.recipe().role, CurveRole::Projection { index_id } if index_id==index)
        {
            return Err(invalid(
                "projection",
                "original index/currency/projection role mismatch",
            ));
        }
    }
    price_term_coupon_options(
        TermCouponPricing {
            bond: input.bond,
            context: input.context,
            history: input.history,
            discount,
            options: input.options,
        },
        |index, date| {
            publications
                .get(&(index.clone(), date))
                .copied()
                .ok_or_else(|| CashflowError::InvalidTerms {
                    field: "coupon publication",
                    reason: "outside frozen original requirements".into(),
                })
        },
        |request| {
            let curve = projection.ok_or_else(|| CashflowError::InvalidTerms {
                field: "projection",
                reason: "original required curve absent".into(),
            })?;
            crate::bond_contract::project_from_curve(request, input.bond.terms().currency, curve)
        },
    )
}

/// Freeze actual administrator decisions and validate the full original baseline.
/// Every required missing published fixing fails before a forward is evaluated.
pub fn prepare_term_coupon_model<'a, P>(
    input: TermCouponModelInputs<'a>,
    curves: &CurveSet,
    mut publication: P,
) -> Result<PreparedTermCouponModel<'a>>
where
    P: FnMut(&IndexId, Date) -> std::result::Result<UtcTimestamp, CashflowError>,
{
    let mut publications = BTreeMap::new();
    input.bond.fixing_requirements(
        input.context.settlement,
        input.context.on_payment,
        |index, date| {
            if let Some(time) = publications.get(&(index.clone(), date)) {
                return Ok(*time);
            }
            let time = publication(index, date)?;
            publications.insert((index.clone(), date), time);
            Ok(time)
        },
    )?;
    let mut recipes = Vec::new();
    let mut grids = Vec::new();
    for key in std::iter::once(&input.discount).chain(input.projection.iter()) {
        let curve = curves.get(key)?;
        recipes.push(curve.recipe().clone());
        let (times, values) = curve.interpolation_grid()?;
        grids.push((times.to_vec(), values.to_vec()));
    }
    let baseline = price_input(&input, &publications, curves)?;
    Ok(PreparedTermCouponModel {
        input,
        publications,
        baseline,
        curve_recipes: recipes,
        curve_grids: grids,
    })
}
