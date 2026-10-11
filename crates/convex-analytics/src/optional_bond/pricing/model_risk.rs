//! Parameter scenarios reuse the exact admitted cash, dates and original HW1F tree.
use super::*;

/// Native constant parameter; neither member is a market volatility quotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ModelParameter {
    /// OU mean reversion per year.
    MeanReversion,
    /// Short-rate diffusion per square-root year.
    Volatility,
}
/// Parameter derivative refinement bounds, in price per100 original face.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ModelRiskSettings {
    /// Absolute first derivative difference per one native parameter unit.
    pub absolute_first_tolerance: f64,
    /// Relative first derivative difference.
    pub relative_first_tolerance: f64,
    /// Absolute second derivative difference per native parameter unit squared.
    pub absolute_second_tolerance: f64,
    /// Relative second derivative difference.
    pub relative_second_tolerance: f64,
}
impl ModelRiskSettings {
    /// Finite explicit bounds; there are no default parameter-risk tolerances.
    pub fn validate(self) -> Result<(), OptionalBondError> {
        if [
            self.absolute_first_tolerance,
            self.absolute_second_tolerance,
        ]
        .iter()
        .any(|v| !v.is_finite() || *v <= 0.0)
            || [
                self.relative_first_tolerance,
                self.relative_second_tolerance,
            ]
            .iter()
            .any(|v| !v.is_finite() || *v < 0.0)
        {
            return Err(invalid(
                "model risk settings",
                "finite positive absolute and nonnegative relative derivative bounds required",
            ));
        }
        Ok(())
    }
}
/// Actual parameter derivatives on one of the original admitted dated grids.
#[derive(Debug, Clone, Serialize)]
pub struct GridModelRisk {
    /// Original desired resolution, before inserting exact events.
    pub desired_steps: usize,
    /// Central first price derivative per native parameter unit.
    pub first: f64,
    /// Central second price derivative per native parameter unit squared.
    pub second: f64,
}
/// Complete native parameter risk, separate from rate duration or PV01.
#[derive(Debug, Clone, Serialize)]
pub struct OptionalModelRisk {
    /// Explicit constant HW1F parameter.
    pub parameter: ModelParameter,
    /// Actual native-unit positive central move.
    pub bump: f64,
    /// Independently checked derivative refinement policy.
    pub settings: ModelRiskSettings,
    /// All actually measured first and second derivatives.
    pub grids: Vec<GridModelRisk>,
    /// Original fully admitted model price.
    pub base: OptionalBondPrice,
    /// Actual positive parameter scenario, including original model provenance.
    pub up: OptionalBondPrice,
    /// Actual negative parameter scenario.
    pub down: OptionalBondPrice,
}
impl PreparedOptionalBond {
    /// SAME rate bump, state-resolution and settings guard used by native `risk`.
    pub fn risk_resolution(
        &self,
        bump: f64,
        settings: RiskSettings,
    ) -> Result<f64, OptionalBondError> {
        if !bump.is_finite() || bump <= 0.0 || bump > 0.10 {
            return Err(invalid("risk bump", "requires decimal bump in (0,0.10]"));
        }
        settings.validate()?;
        let finest = self
            .grids
            .last()
            .ok_or_else(|| invalid("risk grid", "empty"))?;
        let mut minimum_bump: f64 = 0.0;
        for (decision, _) in &finest.exercise {
            if *decision > 0 {
                minimum_bump = minimum_bump.max(2.0 * finest.tree.state_spacing(*decision)?);
            }
        }
        if bump < minimum_bump {
            return Err(OptionalBondError::RiskResolution { bump, minimum_bump });
        }
        Ok(minimum_bump)
    }
    /// Rebuild the SAME native tree at every original exact time and DF. Cash,
    /// exercised branches, future reset basis and known revisions stay intact.
    pub fn price_model_scenario(
        &self,
        oas: f64,
        parameter: ModelParameter,
        delta: f64,
    ) -> Result<OptionalBondPrice, OptionalBondError> {
        if delta == 0.0 {
            return self.price(oas);
        }
        let mut scenario = self.clone();
        scenario.model = self.model.parameter_scenario(parameter, delta)?;
        for grid in &mut scenario.grids {
            grid.tree = Hw1fTree::build(
                scenario.model.parameters(),
                grid.tree.times(),
                &grid.base_discounts,
            )?;
            let factor = 1 + scenario.exercises.len() + 3 * scenario.floating.len();
            if grid
                .tree
                .diagnostics()
                .nodes
                .checked_mul(factor)
                .is_none_or(|work| work > MAX_PRICE_WORK)
            {
                return Err(invalid(
                    "optional rollback work",
                    "32 million node-visit conservative budget per grid exceeded",
                ));
            }
        }
        scenario.price(oas)
    }
    /// Parameter risk checks price convergence and BOTH final successive first/
    /// second derivative refinements. The rate-cell floor has different units
    /// and is not mislabelled as a model-parameter resolution certificate.
    pub fn model_risk(
        &self,
        oas: f64,
        parameter: ModelParameter,
        bump: f64,
        settings: ModelRiskSettings,
    ) -> Result<OptionalModelRisk, OptionalBondError> {
        if !bump.is_finite() || bump <= 0.0 || bump > 0.1 {
            return Err(invalid(
                "model bump",
                "positive finite native-unit bump <=0.1 required",
            ));
        }
        settings.validate()?;
        let base = self.price(oas)?;
        let up = self.price_model_scenario(oas, parameter, bump)?;
        let down = self.price_model_scenario(oas, parameter, -bump)?;
        let grids = base
            .grids
            .iter()
            .zip(&up.grids)
            .zip(&down.grids)
            .map(|((b, u), d)| {
                Ok(GridModelRisk {
                    desired_steps: b.desired_steps,
                    first: finite(
                        (u.dirty_price - d.dirty_price) / (2.0 * bump),
                        "model first derivative",
                    )?,
                    second: finite(
                        (u.dirty_price + d.dirty_price - 2.0 * b.dirty_price) / (bump * bump),
                        "model second derivative",
                    )?,
                })
            })
            .collect::<Result<Vec<_>, OptionalBondError>>()?;
        for pair in grids[grids.len() - 3..].windows(2) {
            if (pair[1].first - pair[0].first).abs()
                > settings.absolute_first_tolerance
                    + settings.relative_first_tolerance * pair[1].first.abs()
                || (pair[1].second - pair[0].second).abs()
                    > settings.absolute_second_tolerance
                        + settings.relative_second_tolerance * pair[1].second.abs()
            {
                return Err(OptionalBondError::ModelRiskConvergence { diagnostics: grids });
            }
        }
        Ok(OptionalModelRisk {
            parameter,
            bump,
            settings,
            grids,
            base,
            up,
            down,
        })
    }
}
