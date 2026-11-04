//! Thin exact calendar resolution; original native owners retain all economics.
use super::inputs::ResolvedInputs;
use super::*;
use convex_analytics::{cash_calibration as c, cash_quotation as q, workflow as w};

fn pins(p: &w::IcmaCalendarPins) -> BondCalendarPins {
    BondCalendarPins {
        payment: p.payment.clone(),
        reset: p.reset.clone(),
        observation: p.observation.clone(),
        exercise: p.exercise.clone(),
    }
}
fn resources<'a>(
    securities: &'a [q::IcmaGovernmentSecurityQuote],
    r: &'a ResolvedInputs<'_>,
) -> LibraryResult<Vec<c::CashHelperResources<'a>>> {
    if securities.len() > 128 {
        return Err(LibraryError::limit(
            "at most128 original government securities",
        ));
    }
    securities
        .iter()
        .map(|s| super::cash::resources(&s.quote.helper.terms, &pins(&s.icma.calendars), r))
        .collect()
}
fn inputs<'a>(
    securities: &'a [q::IcmaGovernmentSecurityQuote],
    resources: &'a [c::CashHelperResources<'a>],
) -> Vec<q::IcmaGovernmentSecurityInputs<'a>> {
    securities
        .iter()
        .zip(resources)
        .map(|(quote, resources)| q::IcmaGovernmentSecurityInputs { quote, resources })
        .collect()
}
pub(super) fn spread(
    target: &q::IcmaCashSecurityQuote,
    securities: &[q::IcmaGovernmentSecurityQuote],
    reference: Date,
    settings: &q::IcmaGovernmentSpreadSettings,
    r: &ResolvedInputs<'_>,
) -> LibraryResult<Value> {
    let bound = resources(securities, r)?;
    let target_resources =
        super::cash::resources(&target.helper.terms, &pins(&target.icma.calendars), r)?;
    value(q::icma_government_spread(
        target,
        &target_resources,
        &inputs(securities, &bound),
        reference,
        settings,
    )?)
}
pub(super) fn price(
    target: &q::IcmaGovernmentSpreadTarget,
    securities: &[q::IcmaGovernmentSecurityQuote],
    reference: Date,
    settings: &q::IcmaGovernmentSpreadPriceSettings,
    quote: &q::GovernmentSpreadQuote,
    r: &ResolvedInputs<'_>,
) -> LibraryResult<Value> {
    let bound = resources(securities, r)?;
    let target_resources =
        super::cash::resources(&target.target.terms, &pins(&target.icma.calendars), r)?;
    value(q::price_icma_government_spread(
        target,
        &target_resources,
        &inputs(securities, &bound),
        reference,
        settings,
        quote,
    )?)
}
