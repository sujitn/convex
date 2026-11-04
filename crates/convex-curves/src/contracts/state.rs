//! Exact immutable valuation-state comparison; no query sampling or resampling.
use super::*;
use crate::contracts::structured::{StructuredCurve, StructuredValues};

impl BuiltCurve {
    /// Compare the complete validated native valuation state, including actual
    /// fitted grids/parameters and recursively bound original dependencies.
    /// Independently rebuilt identical state is admitted. Diagnostics, recipes
    /// and all native domain/tail policies participate. No approximate equality,
    /// finite query sample or pointer-only acceptance represents curve identity.
    pub fn same_valuation_state(&self, other: &Self) -> Result<bool> {
        const LIMIT: usize = MAX_CURVES * MAX_CURVES;
        let mut pending = vec![(self, other)];
        let mut seen = BTreeSet::new();
        while let Some((left, right)) = pending.pop() {
            if !seen.insert((std::ptr::from_ref(left), std::ptr::from_ref(right))) {
                continue;
            }
            if seen.len() > LIMIT || pending.len() > LIMIT {
                return Err(Error::BoundExceeded("exact curve-state comparison"));
            }
            if left.recipe != right.recipe
                || left.diagnostics != right.diagnostics
                || left.depth != right.depth
            {
                return Ok(false);
            }
            let same = match (&left.values, &right.values) {
                (CurveValues::Direct(a), CurveValues::Direct(b)) => {
                    a.tenors() == b.tenors() && a.values() == b.values()
                }
                (CurveValues::Grid(a), CurveValues::Grid(b)) => a.grid() == b.grid(),
                (CurveValues::Monotone(a), CurveValues::Monotone(b)) => {
                    a.grid() == b.grid() && a.diagnostics == b.diagnostics
                }
                (CurveValues::Basis(a), CurveValues::Basis(b)) => {
                    a.tenors == b.tenors && a.values == b.values
                }
                (CurveValues::Ufr(a), CurveValues::Ufr(b)) => {
                    pending.push((a.original_base(), b.original_base()));
                    a.diagnostics == b.diagnostics
                }
                (CurveValues::Structured(a), CurveValues::Structured(b)) => {
                    same_structured_state(a, b, &mut pending)
                }
                (
                    CurveValues::Spread {
                        base: ab,
                        spread: av,
                    },
                    CurveValues::Spread {
                        base: bb,
                        spread: bv,
                    },
                ) => {
                    pending.push((ab, bb));
                    av == bv
                }
                (CurveValues::Equivalent { base: a }, CurveValues::Equivalent { base: b }) => {
                    pending.push((a, b));
                    true
                }
                (
                    CurveValues::Rolled {
                        base: ab,
                        offset: ao,
                        anchor: aa,
                        policy: ap,
                    },
                    CurveValues::Rolled {
                        base: bb,
                        offset: bo,
                        anchor: ba,
                        policy: bp,
                    },
                ) => {
                    pending.push((ab, bb));
                    ao == bo && aa == ba && ap == bp
                }
                _ => false,
            };
            if !same {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

fn same_structured_state<'a>(
    a: &'a StructuredCurve,
    b: &'a StructuredCurve,
    pending: &mut Vec<(&'a BuiltCurve, &'a BuiltCurve)>,
) -> bool {
    if a.diagnostics != b.diagnostics {
        return false;
    }
    match (&a.values, &b.values) {
        (
            StructuredValues::Parametric {
                model: am,
                horizon: ah,
            },
            StructuredValues::Parametric {
                model: bm,
                horizon: bh,
            },
        ) => am == bm && ah == bh,
        (
            StructuredValues::Delegated {
                base: ab,
                range: ar,
                fallback: af,
                ..
            },
            StructuredValues::Delegated {
                base: bb,
                range: br,
                fallback: bf,
                ..
            },
        ) => {
            pending.push((ab, bb));
            ar == br && af == bf
        }
        (
            StructuredValues::Segmented { bases: ab, .. },
            StructuredValues::Segmented { bases: bb, .. },
        ) => {
            if ab.len() != bb.len() {
                return false;
            }
            for ((ac, af), (bc, bf)) in ab.iter().zip(bb) {
                if af != bf {
                    return false;
                }
                pending.push((ac, bc));
            }
            true
        }
        _ => false,
    }
}
