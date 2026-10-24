//! Bounded, independent authorization policy. No token claims confer local roles.

use super::{AccessError, VerifiedIdentity};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Identity classification assigned by the operator, never inferred from a username.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityKind {
    /// Interactive identity.
    Human,
    /// Workload identity.
    Service,
}
/// Closed initial role set. A role never grants a resource wildcard implicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Read data/results within grants.
    Reader,
    /// Calculate and submit/cancel scoped jobs.
    Analyst,
    /// Edit reference/curve data.
    ReferenceWriter,
    /// Edit scoped portfolios.
    BookWriter,
    /// Inspect configuration and operations, and stage, activate and roll back configuration.
    Operator,
}
/// Capabilities checked identically at HTTP, WS and future job admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Read reference.
    ReferenceRead,
    /// Edit reference.
    ReferenceWrite,
    /// Read market inputs.
    MarketRead,
    /// Edit curves.
    CurveWrite,
    /// Read valuation results.
    ResultRead,
    /// Calculate.
    Calculate,
    /// Read books.
    BookRead,
    /// Edit books.
    BookWrite,
    /// Inspect operational/configuration state.
    Operate,
    /// Stage, activate or roll back configuration.
    ConfigurationWrite,
    /// Read a job.
    JobRead,
    /// Submit a job.
    JobSubmit,
    /// Cancel a job.
    JobCancel,
    /// Establish a stream (individual topics need further permission).
    Stream,
}
impl Action {
    pub(crate) fn allowed(self, roles: &[Role]) -> bool {
        let reader = roles
            .iter()
            .any(|r| matches!(r, Role::Reader | Role::Analyst));
        match self {
            Self::ReferenceRead
            | Self::MarketRead
            | Self::ResultRead
            | Self::BookRead
            | Self::JobRead
            | Self::Stream => reader,
            Self::Calculate | Self::JobSubmit | Self::JobCancel => roles.contains(&Role::Analyst),
            Self::ReferenceWrite | Self::CurveWrite => roles.contains(&Role::ReferenceWriter),
            Self::BookWrite => roles.contains(&Role::BookWriter),
            Self::Operate | Self::ConfigurationWrite => roles.contains(&Role::Operator),
        }
    }
}
/// Resource namespaces independent of vendor and storage identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    /// Instrument.
    Instrument,
    /// Versioned curve.
    Curve,
    /// Book; v1 PortfolioId uses this namespace.
    Book,
    /// ETF.
    Etf,
    /// Canonical source, never a vendor SDK handle.
    Source,
    /// Application configuration.
    Configuration,
}
/// Independent entitlement classes; roles alone never grant data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataScope {
    /// Reference definitions.
    Reference,
    /// Market quotes and curves.
    Market,
    /// Valuation results.
    Results,
    /// Portfolio/book contents.
    Books,
    /// Effective configuration.
    Configuration,
    /// Runtime/dependency operations.
    Operations,
}
/// Exact resource or explicit whole-dataset access. Lists require a literal wildcard grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resource {
    /// Namespace.
    pub kind: ResourceKind,
    /// Exact canonical identifier; '*' denotes whole-dataset access.
    pub id: String,
}
impl Resource {
    /// Construct a resource requirement. Validation is repeated before authorization.
    pub fn new(kind: ResourceKind, id: impl Into<String>) -> Self {
        Self {
            kind,
            id: id.into(),
        }
    }
}
/// Bounded quotas, per process and subject. Distributed quotas require a later shared adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Quotas {
    /// Admissions/messages in a monotonic 60 second window.
    pub requests_per_minute: u32,
    /// Concurrent HTTP operations.
    pub concurrent_requests: usize,
    /// Concurrent WS sessions.
    pub websocket_sessions: usize,
    /// Total distinct topics per WS session.
    pub subscriptions: usize,
}
impl Default for Quotas {
    fn default() -> Self {
        Self {
            requests_per_minute: 600,
            concurrent_requests: 8,
            websocket_sessions: 4,
            subscriptions: 256,
        }
    }
}
impl Quotas {
    pub(crate) fn validate(self) -> Result<(), AccessError> {
        if !(1..=6000).contains(&self.requests_per_minute)
            || !(1..=64).contains(&self.concurrent_requests)
            || !(1..=32).contains(&self.websocket_sessions)
            || !(1..=256).contains(&self.subscriptions)
        {
            return Err(AccessError::InvalidPolicy);
        }
        Ok(())
    }
}
/// One operator-approved binding, including independent token scope requirements.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    /// Exact issuer subject.
    pub subject: String,
    /// Explicit human/service classification.
    pub kind: IdentityKind,
    /// Closed local roles.
    pub roles: Vec<Role>,
    /// Entitlement categories.
    pub data: Vec<DataScope>,
    /// Exact resources or a sole '*' entry in a namespace.
    pub resources: BTreeMap<ResourceKind, Vec<String>>,
    /// Required OAuth scopes, all must remain present.
    pub token_scopes: Vec<String>,
    /// Exclusive grant expiry in Unix seconds.
    pub valid_until: i64,
    /// Subject quotas.
    #[serde(default)]
    pub quotas: Quotas,
}
impl Grant {
    pub(crate) fn check(
        &self,
        identity: &VerifiedIdentity,
        now: i64,
        action: Action,
        data: &[DataScope],
        resources: &[Resource],
    ) -> Result<(), AccessError> {
        if now >= self.valid_until
            || self.subject != identity.subject
            || !self
                .token_scopes
                .iter()
                .all(|s| identity.scopes.contains(s))
        {
            return Err(AccessError::Forbidden);
        }
        if !action.allowed(&self.roles) || !data.iter().all(|d| self.data.contains(d)) {
            return Err(AccessError::Forbidden);
        }
        for resource in resources {
            if !valid_id(&resource.id) {
                return Err(AccessError::Forbidden);
            }
            let allowed = self
                .resources
                .get(&resource.kind)
                .is_some_and(|ids| ids.iter().any(|id| id == "*" || id == &resource.id));
            if !allowed {
                return Err(AccessError::Forbidden);
            }
        }
        Ok(())
    }
}
/// Strict versioned access policy. Replacing it does not alter valuation configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyDocument {
    /// Supported schema is 1.
    pub schema_version: u32,
    /// Monotonic policy revision.
    pub version: u64,
    /// Exact trusted issuer.
    pub issuer: String,
    /// Exact API resource audience (not an OIDC login client).
    pub audience: String,
    /// Exclusive policy expiry in Unix seconds.
    pub valid_until: i64,
    /// Operator provenance, retained in the document.
    pub actor: String,
    /// Why this revision was authorized.
    pub reason: String,
    /// At most 256 distinct subjects.
    pub grants: Vec<Grant>,
}
impl PolicyDocument {
    /// Validate bounds and ambiguity without consulting the clock or mutating state.
    pub fn validate(&self) -> Result<(), AccessError> {
        if self.schema_version != 1
            || self.version == 0
            || self.valid_until <= 0
            || self.issuer.len() > 512
            || !self.issuer.starts_with("https://")
            || !valid_id(&self.audience)
            || self.audience == "*"
            || !valid_id(&self.actor)
            || self.actor == "*"
            || self.reason.trim().is_empty()
            || self.reason.len() > 1024
            || self.reason.chars().any(char::is_control)
            || self.grants.len() > 256
        {
            return Err(AccessError::InvalidPolicy);
        }
        let mut subjects = BTreeSet::new();
        for grant in &self.grants {
            if !valid_subject(&grant.subject)
                || grant.subject == "*"
                || !subjects.insert(&grant.subject)
                || grant.valid_until <= 0
                || grant.valid_until > self.valid_until
                || grant.roles.len() > 6
                || grant.data.len() > 6
                || grant.token_scopes.is_empty()
                || grant.token_scopes.len() > 32
                || grant.resources.len() > 6
            {
                return Err(AccessError::InvalidPolicy);
            }
            if grant.roles.iter().collect::<BTreeSet<_>>().len() != grant.roles.len()
                || grant.data.iter().collect::<BTreeSet<_>>().len() != grant.data.len()
                || grant.token_scopes.iter().collect::<BTreeSet<_>>().len()
                    != grant.token_scopes.len()
            {
                return Err(AccessError::InvalidPolicy);
            }
            if grant
                .token_scopes
                .iter()
                .any(|scope| !valid_scope(scope) || scope == "*")
            {
                return Err(AccessError::InvalidPolicy);
            }
            let mut count = 0usize;
            for ids in grant.resources.values() {
                count = count.saturating_add(ids.len());
                if ids.len() > 256
                    || ids.iter().any(|id| !valid_id(id))
                    || (ids.iter().any(|id| id == "*") && ids.len() != 1)
                    || ids.iter().collect::<BTreeSet<_>>().len() != ids.len()
                {
                    return Err(AccessError::InvalidPolicy);
                }
            }
            if count > 1024 {
                return Err(AccessError::InvalidPolicy);
            }
            grant.quotas.validate()?;
        }
        Ok(())
    }
}
pub(crate) fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._:@/*".contains(&b))
}
pub(crate) fn valid_subject(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value.is_ascii()
        && !value.chars().any(char::is_control)
}
pub(crate) fn valid_scope(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|b| b == 0x21 || (0x23..=0x5b).contains(&b) || (0x5d..=0x7e).contains(&b))
}
