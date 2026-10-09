//! Thin full native cash quotation and actual-security benchmark composition.
use super::inputs::ResolvedInputs;
use super::*;
use convex_analytics::{cash_calibration as c, cash_quotation as q, workflow as w};

/// Full original cash helper and independent exact calendar bindings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CashSecurityInput {
    /// Complete security, observed quote, settlement and source revision.
    pub helper: c::CashBondHelper,
    /// Independent payment/reset/observation/exercise versions.
    pub calendars: BondCalendarPins,
}
/// Government security reference/quote and independently bound calendars.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct GovernmentSecurityInput {
    /// Original reference classification, quote and economic terms.
    pub quote: q::GovernmentSecurityQuote,
    /// Independently supplied exact calendars for this security.
    pub calendars: BondCalendarPins,
}
/// Full reverse-spread target and independently supplied calendar bindings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct GovernmentSpreadTargetInput {
    /// Complete actual target/reference state without an observed price proxy.
    pub target: q::GovernmentSpreadTarget,
    /// Independent exact original calendars.
    pub calendars: BondCalendarPins,
}
/// Full quotation or signed original-price consideration, without posting.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CashSecurityOperation {
    /// Original clean/dirty/accrued and full-cash yield inverse.
    Quote {},
    /// Signed monetary consideration with the original reference face.
    Invoice {
        /// Actual notional units, reference size and native currency.
        position: convex_analytics::quote_risk::RiskPosition,
    },
}
fn resources<'a>(
    terms: &'a convex_bonds::contracts::BondTerms,
    calendars: &BondCalendarPins,
    r: &'a ResolvedInputs<'_>,
) -> LibraryResult<c::CashHelperResources<'a>> {
    let bound = r.bond_calendars(&BondInput {
        terms: terms.clone(),
        calendars: calendars.clone(),
    })?;
    Ok(c::CashHelperResources::Bond {
        ids: &terms.calendars,
        calendars: bound,
    })
}
pub(super) fn security(
    input: &CashSecurityInput,
    reference: Date,
    convention: w::DatedYieldConvention,
    settings: w::BondInverseSettings,
    action: &CashSecurityOperation,
    r: &ResolvedInputs<'_>,
) -> LibraryResult<Value> {
    let resources = resources(&input.helper.terms, &input.calendars, r)?;
    match action {
        CashSecurityOperation::Quote {} => value(q::quote_cash_security(
            &input.helper,
            &resources,
            reference,
            convention,
            settings,
        )?),
        CashSecurityOperation::Invoice { position } => value(q::cash_security_invoice(
            &input.helper,
            &resources,
            reference,
            convention,
            settings,
            position,
        )?),
    }
}
pub(super) fn government(
    target: &CashSecurityInput,
    securities: &[GovernmentSecurityInput],
    reference: Date,
    settings: &q::GovernmentSpreadSettings,
    r: &ResolvedInputs<'_>,
) -> LibraryResult<Value> {
    if securities.len() > 128 {
        return Err(LibraryError::limit(
            "at most128 original government securities",
        ));
    }
    let target_resources = resources(&target.helper.terms, &target.calendars, r)?;
    let bound = securities
        .iter()
        .map(|s| resources(&s.quote.helper.terms, &s.calendars, r))
        .collect::<LibraryResult<Vec<_>>>()?;
    let inputs: Vec<_> = securities
        .iter()
        .zip(&bound)
        .map(|(s, resources)| q::GovernmentSecurityInputs {
            quote: &s.quote,
            resources,
        })
        .collect();
    value(q::government_spread(
        &target.helper,
        &target_resources,
        &inputs,
        reference,
        settings,
    )?)
}
pub(super) fn government_price(
    target: &GovernmentSpreadTargetInput,
    securities: &[GovernmentSecurityInput],
    reference: Date,
    settings: &q::GovernmentSpreadPriceSettings,
    quote: &q::GovernmentSpreadQuote,
    position: &convex_analytics::quote_risk::RiskPosition,
    r: &ResolvedInputs<'_>,
) -> LibraryResult<Value> {
    if securities.len() > 128 {
        return Err(LibraryError::limit(
            "at most128 original government securities",
        ));
    }
    let target_resources = resources(&target.target.terms, &target.calendars, r)?;
    let bound = securities
        .iter()
        .map(|s| resources(&s.quote.helper.terms, &s.calendars, r))
        .collect::<LibraryResult<Vec<_>>>()?;
    let inputs: Vec<_> = securities
        .iter()
        .zip(&bound)
        .map(|(s, resources)| q::GovernmentSecurityInputs {
            quote: &s.quote,
            resources,
        })
        .collect();
    value(q::price_government_spread(
        &target.target,
        &target_resources,
        &inputs,
        reference,
        settings,
        quote,
        position,
    )?)
}
