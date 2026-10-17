//! Small deterministic active-set/lot search; every bound and stopping policy is supplied.
use super::evidence::AdmittedSensitivity;
use super::{
    admission::{number, prepare, Prepared},
    *,
};
use crate::quote_risk::HedgeSensitivityEvidence;
use convex_core::contracts::ExactDecimal;
use nalgebra::{linalg::SVD, DMatrix, DVector};
use rust_decimal::Decimal;

/// Aggregate every expected current and pending quantity on complete original keys.
/// Source validation and FX admission are identical to the proposal solver.
pub fn aggregate_hedge_risk(problem: &HedgeProblem) -> Result<HedgeExposure, HedgeError> {
    Ok(prepare(problem)?.exposure)
}
#[derive(Clone)]
struct Constraint {
    row: DVector<f64>,
    rhs: f64,
}
#[derive(Debug, Clone, Copy)]
enum SearchFailure {
    Exhausted,
    Numerical,
}
type Search<T> = Result<T, SearchFailure>;
fn spend(work: &mut HedgeWork, settings: &HedgeSolveSettings, discrete: bool) -> Search<()> {
    if work.active_sets + work.discrete_points >= settings.max_work {
        return Err(SearchFailure::Exhausted);
    }
    if discrete {
        work.discrete_points += 1;
    } else {
        work.active_sets += 1;
    }
    Ok(())
}
fn svd(
    matrix: DMatrix<f64>,
    vectors: bool,
    s: &HedgeSolveSettings,
    work: &mut HedgeWork,
) -> Search<SVD<f64, nalgebra::Dyn, nalgebra::Dyn>> {
    if matrix.iter().any(|v| !v.is_finite()) {
        return Err(SearchFailure::Numerical);
    }
    work.svd_decompositions += 1;
    let value = SVD::try_new(
        matrix,
        vectors,
        vectors,
        5.0 * f64::EPSILON,
        s.svd_iterations,
    )
    .ok_or(SearchFailure::Numerical)?;
    if value.singular_values.iter().any(|v| !v.is_finite()) {
        return Err(SearchFailure::Numerical);
    }
    Ok(value)
}
fn combinations<F>(
    count: usize,
    size: usize,
    start: usize,
    active: &mut Vec<usize>,
    visit: &mut F,
) -> Search<bool>
where
    F: FnMut(&[usize]) -> Search<bool>,
{
    if active.len() == size {
        return visit(active);
    }
    for i in start..count.saturating_sub(size - active.len() - 1) {
        active.push(i);
        let stop = combinations(count, size, i + 1, active, visit)?;
        active.pop();
        if stop {
            return Ok(true);
        }
    }
    Ok(false)
}
fn add_constraint(
    constraints: &mut Vec<Constraint>,
    mut row: DVector<f64>,
    mut rhs: f64,
    tolerance: f64,
) -> Search<bool> {
    if !rhs.is_finite() || row.iter().any(|v| !v.is_finite()) {
        return Err(SearchFailure::Numerical);
    }
    let scale = row.norm();
    if !scale.is_finite() {
        return Err(SearchFailure::Numerical);
    }
    if scale == 0.0 {
        return Ok(rhs >= -tolerance);
    }
    row /= scale;
    rhs /= scale;
    if !rhs.is_finite() || row.iter().any(|v| !v.is_finite()) {
        return Err(SearchFailure::Numerical);
    }
    constraints.push(Constraint { row, rhs });
    Ok(true)
}
fn objective<S: AdmittedSensitivity>(p: &Prepared<S>, x: &DVector<f64>) -> Search<f64> {
    let residual = &p.design * x + &p.offset;
    let costs: f64 = x
        .iter()
        .enumerate()
        .map(|(i, q)| {
            if *q >= 0.0 {
                q * p.buy[i]
            } else {
                -q * p.sell[i]
            }
        })
        .sum();
    let value = 0.5 * residual.norm_squared()
        + p.exposure.problem.settings.cost_weight * costs / p.exposure.problem.settings.cost_scale;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(SearchFailure::Numerical)
    }
}
/// Solve all sign regions. Each is a strictly convex QP; independent active
/// constraint sets up to the free dimension suffice for its KKT optimum.
fn quadratic<S: AdmittedSensitivity>(
    p: &Prepared<S>,
    fixed: &[Option<f64>],
    work: &mut HedgeWork,
) -> Search<Option<DVector<f64>>> {
    let problem = &p.exposure.problem;
    let settings = &problem.settings;
    let n = problem.candidates.len();
    let free: Vec<_> = (0..n).filter(|i| fixed[*i].is_none()).collect();
    let k = free.len();
    let mut origin = DVector::zeros(n);
    for (i, value) in fixed.iter().enumerate() {
        if let Some(v) = value {
            origin[i] = *v;
        }
    }
    let design = DMatrix::from_fn(p.design.nrows(), k, |i, j| p.design[(i, free[j])]);
    let base = &p.design * &origin + &p.offset;
    let hessian = design.transpose() * &design;
    let gradient = design.transpose() * base;
    let mut best: Option<DVector<f64>> = None;
    let mut best_objective = f64::INFINITY;
    for mask in 0..(1usize << k) {
        let mut constraints = Vec::new();
        let mut admissible = true;
        for i in 0..n {
            let reference = number(p.reference[i]).map_err(|_| SearchFailure::Numerical)?;
            let mut lo = number(p.lower[i]).map_err(|_| SearchFailure::Numerical)? / reference;
            let mut hi = number(p.upper[i]).map_err(|_| SearchFailure::Numerical)? / reference;
            if let Some(j) = free.iter().position(|index| *index == i) {
                if mask & (1 << j) == 0 {
                    lo = lo.max(0.0);
                } else {
                    hi = hi.min(0.0);
                }
            }
            if lo > hi {
                admissible = false;
                break;
            }
            let row = DVector::from_iterator(
                k,
                free.iter().map(|index| if *index == i { 1.0 } else { 0.0 }),
            );
            admissible &= add_constraint(
                &mut constraints,
                row.clone(),
                hi - origin[i],
                settings.solver_tolerance,
            )?;
            admissible &= add_constraint(
                &mut constraints,
                -row,
                origin[i] - lo,
                settings.solver_tolerance,
            )?;
        }
        if !admissible {
            continue;
        }
        for (i, f) in problem.factors.iter().enumerate() {
            let original = p.exposure.before[i] + p.risk.row(i).transpose().dot(&origin);
            let row = DVector::from_iterator(k, free.iter().map(|j| p.risk[(i, *j)] / f.scale));
            admissible &= add_constraint(
                &mut constraints,
                row.clone(),
                (f.upper - original) / f.scale,
                settings.solver_tolerance,
            )?;
            admissible &= add_constraint(
                &mut constraints,
                -row,
                (original - f.lower) / f.scale,
                settings.solver_tolerance,
            )?;
        }
        if !admissible {
            continue;
        }
        let mut cost = DVector::zeros(k);
        let mut constant_cost = 0.0;
        for i in 0..n {
            if let Some(j) = free.iter().position(|index| *index == i) {
                cost[j] = if mask & (1 << j) == 0 {
                    p.buy[i]
                } else {
                    -p.sell[i]
                };
            } else {
                constant_cost += if origin[i] >= 0.0 {
                    origin[i] * p.buy[i]
                } else {
                    -origin[i] * p.sell[i]
                };
            }
        }
        admissible &= add_constraint(
            &mut constraints,
            &cost / settings.cost_scale,
            (problem.max_cost - constant_cost) / settings.cost_scale,
            settings.solver_tolerance,
        )?;
        if !admissible {
            continue;
        }
        if k == 0 {
            let value = objective(p, &origin)?;
            if value < best_objective {
                best_objective = value;
                best = Some(origin.clone());
            }
            continue;
        }
        let g = &gradient + &cost * (settings.cost_weight / settings.cost_scale);
        if hessian.iter().chain(g.iter()).any(|v| !v.is_finite()) {
            return Err(SearchFailure::Numerical);
        }
        let mut winner = None;
        let mut visit = |active: &[usize]| -> Search<bool> {
            spend(work, settings, false)?;
            // Dependent constraints cannot form an independent active basis.
            // Decomposition failure is unresolved, never proof of infeasibility.
            if !active.is_empty() {
                let c = DMatrix::from_fn(active.len(), k, |i, j| constraints[active[i]].row[j]);
                let singular = svd(c, false, settings, work)?.singular_values;
                let largest = singular.iter().copied().fold(0.0, f64::max);
                if singular.iter().any(|v| *v <= largest * 64.0 * f64::EPSILON) {
                    return Ok(false);
                }
            }
            let dimension = k + active.len();
            let mut system = DMatrix::zeros(dimension, dimension);
            let mut rhs = DVector::zeros(dimension);
            for i in 0..k {
                rhs[i] = -g[i];
                for j in 0..k {
                    system[(i, j)] = hessian[(i, j)];
                }
            }
            for (a, index) in active.iter().enumerate() {
                rhs[k + a] = constraints[*index].rhs;
                for j in 0..k {
                    system[(k + a, j)] = constraints[*index].row[j];
                    system[(j, k + a)] = constraints[*index].row[j];
                }
            }
            // Complete pivoting with at most three residual corrections using
            // the same decomposition. Every solve must meet the unchanged
            // original-system backward bound; no pseudoinverse or ridge.
            let decomposition = system.clone().full_piv_lu();
            let mut solution = decomposition.solve(&rhs).ok_or(SearchFailure::Numerical)?;
            for refinement in 0..=3 {
                if solution.iter().any(|v| !v.is_finite()) {
                    return Err(SearchFailure::Numerical);
                }
                let error = &system * &solution - &rhs;
                let mut within_bound = true;
                for i in 0..dimension {
                    let magnitude = rhs[i].abs()
                        + system
                            .row(i)
                            .iter()
                            .zip(solution.iter())
                            .map(|(a, b)| (a * b).abs())
                            .sum::<f64>();
                    if !magnitude.is_finite() || !error[i].is_finite() {
                        return Err(SearchFailure::Numerical);
                    }
                    within_bound &= error[i].abs() <= settings.solver_tolerance * (1.0 + magnitude);
                }
                if within_bound {
                    break;
                }
                if refinement == 3 {
                    return Err(SearchFailure::Numerical);
                }
                let correction = decomposition
                    .solve(&error)
                    .ok_or(SearchFailure::Numerical)?;
                solution -= correction;
            }
            let x = solution.rows(0, k).into_owned();
            if constraints
                .iter()
                .any(|c| c.row.dot(&x) > c.rhs + settings.solver_tolerance * (1.0 + c.rhs.abs()))
                || solution
                    .rows(k, active.len())
                    .iter()
                    .any(|lambda| *lambda < -settings.solver_tolerance)
            {
                return Ok(false);
            }
            let mut full = origin.clone();
            for (j, index) in free.iter().enumerate() {
                full[*index] = x[j];
            }
            winner = Some(full);
            Ok(true)
        };
        for size in 0..=k {
            if combinations(constraints.len(), size, 0, &mut Vec::new(), &mut visit)? {
                break;
            }
        }
        if let Some(solution) = winner {
            let value = objective(p, &solution)?;
            if value < best_objective {
                best_objective = value;
                best = Some(solution);
            }
        }
    }
    Ok(best)
}
fn check(name: String, violation: f64, tolerance: f64) -> HedgeConstraintCheck {
    HedgeConstraintCheck {
        name,
        satisfied: violation <= tolerance,
        violation,
        tolerance,
    }
}
fn decimal_interval(
    name: String,
    value: Decimal,
    lo: Decimal,
    hi: Decimal,
) -> Result<HedgeConstraintCheck, HedgeError> {
    Ok(check(
        name,
        number((lo - value).max(value - hi).max(Decimal::ZERO))?,
        0.0,
    ))
}
fn solution<S: AdmittedSensitivity>(
    p: &Prepared<S>,
    x: &DVector<f64>,
    exact: &[Option<Decimal>],
    lots: bool,
) -> Result<HedgeSolution, HedgeError> {
    let problem = &p.exposure.problem;
    let mut quantities = Vec::new();
    let mut final_holdings = Vec::new();
    let mut multiples = Vec::new();
    let mut constraints = Vec::new();
    let mut candidate_costs = Vec::new();
    for (i, c) in problem.candidates.iter().enumerate() {
        let quantity = if let Some(q) = exact[i] {
            q
        } else {
            // Continuous model solution is converted once. Exact closed endpoints
            // are restored only within the explicit dimensionless solve bound;
            // every original constraint is checked again after conversion.
            let lo = number(p.lower[i])? / number(p.reference[i])?;
            let hi = number(p.upper[i])? / number(p.reference[i])?;
            if (x[i] - lo).abs() <= problem.settings.solver_tolerance {
                p.lower[i]
            } else if (x[i] - hi).abs() <= problem.settings.solver_tolerance {
                p.upper[i]
            } else {
                Decimal::from_f64_retain(x[i])
                    .filter(|value| x[i] == 0.0 || !value.is_zero())
                    .and_then(|v| v.checked_mul(p.reference[i]))
                    .filter(|value| x[i] == 0.0 || !value.is_zero())
                    .ok_or_else(|| {
                        invalid("proposal.quantity", "continuous Decimal conversion failed")
                    })?
            }
        };
        let holding = quantity
            .checked_add(p.held[i])
            .ok_or_else(|| invalid("proposal.holding", "Decimal overflow"))?;
        let ratio = quantity
            .checked_div(p.reference[i])
            .ok_or_else(|| invalid("proposal.quantity", "Decimal ratio failed"))?;
        if !quantity.is_zero() && ratio.is_zero() {
            return Err(invalid(
                "proposal.quantity",
                "nonzero original quantity ratio underflow",
            ));
        }
        let multiple = number(ratio)?;
        constraints.push(decimal_interval(
            format!("trade.{i}"),
            quantity,
            c.trade_lower.decimal(),
            c.trade_upper.decimal(),
        )?);
        constraints.push(decimal_interval(
            format!("holding.{i}"),
            holding,
            c.holding_lower.decimal(),
            c.holding_upper.decimal(),
        )?);
        constraints.push(decimal_interval(
            format!("liquidity.{i}"),
            quantity,
            -c.sell_available.decimal(),
            c.buy_available.decimal(),
        )?);
        if lots {
            if let Some(lot) = c.lot {
                let remainder = quantity
                    .checked_rem(lot.decimal())
                    .ok_or_else(|| invalid("lot", "Decimal remainder failed"))?;
                constraints.push(check(format!("lot.{i}"), number(remainder.abs())?, 0.0));
            }
        }
        candidate_costs.push(finite(
            if multiple >= 0.0 {
                multiple * p.buy[i]
            } else {
                -multiple * p.sell[i]
            },
            "proposal costs",
        )?);
        quantities.push(ExactDecimal::new(quantity));
        final_holdings.push(ExactDecimal::new(holding));
        multiples.push(multiple);
    }
    let native_multiples = DVector::from_vec(multiples.clone());
    let after: Vec<_> = p
        .exposure
        .before
        .iter()
        .enumerate()
        .map(|(i, b)| {
            finite(
                b + p.risk.row(i).transpose().dot(&native_multiples),
                "proposal factor",
            )
        })
        .collect::<Result<_, _>>()?;
    let mut residual = Vec::new();
    let mut tracking = 0.0;
    for (i, (actual, f)) in after.iter().zip(&problem.factors).enumerate() {
        let value = finite(actual - f.target, "proposal residual")?;
        residual.push(value);
        tracking = finite(
            tracking + 0.5 * f.weight * (value / f.scale).powi(2),
            "tracking objective",
        )?;
        constraints.push(check(
            format!("factor.{i}"),
            (f.lower - actual).max(actual - f.upper).max(0.0),
            problem.settings.factor_tolerance,
        ));
    }
    let total_cost = finite(candidate_costs.iter().sum(), "total cost")?;
    constraints.push(check(
        "cost".into(),
        (total_cost - problem.max_cost).max(0.0),
        problem.settings.cost_tolerance,
    ));
    let objective = finite(
        tracking + problem.settings.cost_weight * total_cost / problem.settings.cost_scale,
        "objective",
    )?;
    Ok(HedgeSolution {
        quantities,
        reference_multiples: multiples,
        final_holdings,
        after,
        residual,
        candidate_costs,
        total_cost,
        tracking_objective: tracking,
        objective,
        constraints,
    })
}
fn lot_bound(value: Decimal, lot: Decimal, upper: bool) -> Search<Decimal> {
    // Compute the exact integer quotient via the original remainder. Dividing
    // first and then rounding could erase a tiny original boundary remainder.
    let remainder = value.checked_rem(lot).ok_or(SearchFailure::Numerical)?;
    let base = value
        .checked_sub(remainder)
        .and_then(|v| v.checked_div(lot))
        .ok_or(SearchFailure::Numerical)?;
    if upper && remainder < Decimal::ZERO {
        base.checked_sub(Decimal::ONE)
            .ok_or(SearchFailure::Numerical)
    } else if !upper && remainder > Decimal::ZERO {
        base.checked_add(Decimal::ONE)
            .ok_or(SearchFailure::Numerical)
    } else {
        Ok(base)
    }
}
fn lattice<S: AdmittedSensitivity>(
    p: &Prepared<S>,
    index: usize,
    exact: &mut Vec<Option<Decimal>>,
    work: &mut HedgeWork,
    best: &mut Option<HedgeSolution>,
) -> Search<()> {
    if index == 0 {
        // Empty later axes must be checked before any potentially huge outer
        // axis. Otherwise no complete points would spend the original budget.
        for (i, c) in p.exposure.problem.candidates.iter().enumerate() {
            if let Some(lot) = c.lot {
                let lo = lot_bound(p.lower[i], lot.decimal(), false)?;
                let hi = lot_bound(p.upper[i], lot.decimal(), true)?;
                if lo > hi {
                    return Ok(());
                }
            }
        }
    }
    if index == p.reference.len() {
        spend(work, &p.exposure.problem.settings, true)?;
        let fixed = exact
            .iter()
            .enumerate()
            .map(|(i, q)| {
                q.map(|v| Ok::<_, HedgeError>(number(v)? / number(p.reference[i])?))
                    .transpose()
                    .map_err(|_| SearchFailure::Numerical)
            })
            .collect::<Search<Vec<_>>>()?;
        if let Some(x) = quadratic(p, &fixed, work)? {
            let proposal = solution(p, &x, exact, true).map_err(|_| SearchFailure::Numerical)?;
            if proposal.constraints.iter().any(|c| !c.satisfied) {
                return Err(SearchFailure::Numerical);
            }
            if best
                .as_ref()
                .is_none_or(|previous| proposal.objective < previous.objective)
            {
                *best = Some(proposal);
            }
        }
        return Ok(());
    }
    if let Some(lot) = p.exposure.problem.candidates[index].lot {
        let lot = lot.decimal();
        let lo = lot_bound(p.lower[index], lot, false)?;
        let hi = lot_bound(p.upper[index], lot, true)?;
        let mut point = lo;
        while point <= hi {
            exact[index] = Some(point.checked_mul(lot).ok_or(SearchFailure::Numerical)?);
            lattice(p, index + 1, exact, work, best)?;
            point = point
                .checked_add(Decimal::ONE)
                .ok_or(SearchFailure::Numerical)?;
        }
        exact[index] = None;
    } else {
        lattice(p, index + 1, exact, work, best)?;
    }
    Ok(())
}
/// Minimize complete scaled factor residuals plus supplied side costs, subject to
/// original native holding/trade/liquidity, full factor and cost limits. All lot
/// combinations are searched before a final proposal is returned; incomplete work
/// retains diagnostics only. Provider/clock/storage/order authority is absent.
pub fn solve_hedge_proposal(problem: &HedgeProblem) -> Result<HedgeSolveReport, HedgeError> {
    solve(problem)
}
/// Use actual replayed current/pending state risk with unchanged fresh candidate
/// columns, native quantity constraints, numerical policy and bounded optimizer.
pub fn solve_lifecycle_hedge_proposal(
    problem: &HedgeProblem,
    states: &[LifecycleHoldingRisk],
) -> Result<HedgeSolveReport, HedgeError> {
    solve_prepared(
        problem,
        super::admission::prepare_with_lifecycle(problem, states)?,
    )
}
/// Solve explicit central/directional supplied evidence with the original optimizer.
pub fn solve_directional_hedge_proposal(
    problem: &Problem<HedgeSensitivityEvidence>,
) -> Result<SolveReport<HedgeSensitivityEvidence>, HedgeError> {
    solve(problem)
}
/// Aggregate complete original quantities with explicit directional evidence.
pub fn aggregate_directional_hedge_risk(
    problem: &Problem<HedgeSensitivityEvidence>,
) -> Result<Exposure<HedgeSensitivityEvidence>, HedgeError> {
    Ok(prepare(problem)?.exposure)
}
fn solve<S: AdmittedSensitivity>(problem: &Problem<S>) -> Result<SolveReport<S>, HedgeError> {
    let p = prepare(problem)?;
    solve_prepared(problem, p)
}
fn solve_prepared<S: AdmittedSensitivity>(
    problem: &Problem<S>,
    p: Prepared<S>,
) -> Result<SolveReport<S>, HedgeError> {
    if problem.candidates.is_empty() {
        return Err(invalid(
            "candidates",
            "a proposal needs at least one complete candidate",
        ));
    }
    let mut work = HedgeWork::default();
    let mut report = SolveReport {
        exposure: p.exposure.clone(),
        status: HedgeSolveStatus::NumericalFailure,
        message: "bounded design decomposition unresolved".into(),
        authority: HedgeAuthority::ProposalOnly,
        rank: HedgeRank {
            columns: problem.candidates.len(),
            rank: 0,
            singular_values: Vec::new(),
            condition: None,
        },
        continuous: None,
        proposal: None,
        work: HedgeWork::default(),
    };
    let singular = match svd(p.design.clone(), false, &problem.settings, &mut work) {
        Ok(value) => value.singular_values,
        Err(_) => {
            report.work = work;
            return Ok(report);
        }
    };
    let largest = singular.iter().copied().fold(0.0, f64::max);
    let smallest = singular.iter().copied().fold(f64::INFINITY, f64::min);
    report.rank.rank = singular
        .iter()
        .filter(|v| **v > 0.0 && **v / largest >= problem.settings.rank_relative_tolerance)
        .count();
    report.rank.singular_values = singular.as_slice().to_vec();
    if report.rank.rank == problem.candidates.len() && smallest > 0.0 {
        let condition = largest / smallest;
        if condition.is_finite() {
            report.rank.condition = Some(condition);
        }
    }
    if report.rank.rank < problem.candidates.len() {
        report.status = HedgeSolveStatus::RankDeficient;
        report.message = "full original candidate factor matrix is unidentified".into();
    } else if report
        .rank
        .condition
        .is_none_or(|c| c > problem.settings.max_condition)
    {
        report.status = HedgeSolveStatus::IllConditioned;
        report.message = "scaled design exceeds original conditioning policy".into();
    } else if p.lower.iter().zip(&p.upper).any(|(lo, hi)| lo > hi) {
        report.status = HedgeSolveStatus::Infeasible;
        report.message =
            "original native trade/liquidity/holding intervals have empty intersection".into();
    } else {
        let searched = (|| -> Search<()> {
            let continuous = quadratic(&p, &vec![None; problem.candidates.len()], &mut work)?;
            let Some(x) = continuous else {
                report.status = HedgeSolveStatus::Infeasible;
                report.message =
                    "complete continuous constraint search found no feasible point".into();
                return Ok(());
            };
            let value = solution(&p, &x, &vec![None; problem.candidates.len()], false)
                .map_err(|_| SearchFailure::Numerical)?;
            if value.constraints.iter().any(|c| !c.satisfied) {
                return Err(SearchFailure::Numerical);
            }
            report.continuous = Some(value);
            if problem.candidates.iter().all(|c| c.lot.is_none()) {
                report.proposal = report.continuous.clone();
            } else {
                lattice(
                    &p,
                    0,
                    &mut vec![None; problem.candidates.len()],
                    &mut work,
                    &mut report.proposal,
                )?;
            }
            if report.proposal.is_some() {
                report.status = HedgeSolveStatus::Feasible;
                report.message =
                    "completed supplied-model search and every original final constraint".into();
            } else {
                report.status = HedgeSolveStatus::DiscreteInfeasible;
                report.message =
                    "complete admitted native lot lattice has no feasible proposal".into();
            }
            Ok(())
        })();
        if let Err(reason) = searched {
            report.proposal = None;
            (report.status, report.message) = match reason {
                SearchFailure::Exhausted => (
                    HedgeSolveStatus::WorkExhausted,
                    "original work budget exhausted; no incumbent is a proposal".into(),
                ),
                SearchFailure::Numerical => (
                    HedgeSolveStatus::NumericalFailure,
                    "bounded solve or original final native/factor/cost postcondition unresolved"
                        .into(),
                ),
            };
        }
    }
    report.work = work;
    Ok(report)
}
