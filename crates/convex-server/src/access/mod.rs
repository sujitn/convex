//! Authentication, current entitlement checks and bounded transport admission.
//! Kept outside pure analytics. The default composition denies protected access.

pub(crate) mod http;
pub mod identity;
pub(crate) mod policy;
use async_trait::async_trait;
pub use policy::*;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::{Arc, Mutex, RwLock},
    time::Instant,
};
use thiserror::Error;
use tokio::sync::{watch, OwnedSemaphorePermit, Semaphore};

/// Stable failures without credential, URL, remote text or path content.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum AccessError {
    /// Missing/inactive/expired identity.
    #[error("UNAUTHENTICATED")]
    Unauthenticated,
    /// Current grant does not permit this operation.
    #[error("FORBIDDEN")]
    Forbidden,
    /// Required identity/policy service cannot be verified.
    #[error("ACCESS_UNAVAILABLE")]
    Unavailable,
    /// Fixed resource or subject quota exhausted.
    #[error("ACCESS_QUOTA_EXCEEDED")]
    Quota,
    /// Invalid bounded access policy/configuration.
    #[error("INVALID_ACCESS_POLICY")]
    InvalidPolicy,
    /// Policy revision conflicts or regresses.
    #[error("ACCESS_POLICY_CONFLICT")]
    Conflict,
    /// Invalid bounded request.
    #[error("INVALID_ACCESS_REQUEST")]
    InvalidRequest,
}
/// Opaque bearer credential. Debug/serialization can never reveal its value.
#[derive(Clone)]
pub struct BearerToken(Arc<str>);
impl BearerToken {
    /// Parse the RFC 6750 alphabet and a hard 8192-byte bound.
    pub fn parse(value: &str) -> Result<Self, AccessError> {
        let text = value.trim_end_matches('=');
        if value.is_empty()
            || value.len() > 8192
            || text.is_empty()
            || !text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._~+/".contains(&b))
            || value[text.len()..].len() > 8
        {
            return Err(AccessError::Unauthenticated);
        }
        Ok(Self(Arc::from(value)))
    }
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}
impl fmt::Debug for BearerToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BearerToken([REDACTED])")
    }
}
/// Result of a trusted access-token verifier, never constructed from a proxy identity header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedIdentity {
    /// Exact issuer.
    pub issuer: String,
    /// Exact issuer subject.
    pub subject: String,
    /// Exact selected API audience.
    pub audience: String,
    /// Exclusive credential expiry, Unix seconds.
    pub expires_at: i64,
    /// Current remote OAuth scopes.
    pub scopes: BTreeSet<String>,
}
/// Real external identity boundary. Implementations must reject unverifiable credentials.
#[async_trait]
pub trait IdentityVerifier: Send + Sync {
    /// Verify an API access token, including issuer, audience and current revocation state.
    async fn verify(&self, token: &BearerToken) -> Result<VerifiedIdentity, AccessError>;
    /// Bounded real availability check, without using a customer's credential.
    async fn probe(&self) -> Result<(), AccessError>;
}
/// Explicit accepted context. Retained only for a bounded operation/session; never sent to clients.
#[derive(Clone)]
pub struct AccessContext {
    identity: Option<VerifiedIdentity>,
    token: Option<BearerToken>,
    local: bool,
    verified_at: Instant,
}
impl fmt::Debug for AccessContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccessContext([REDACTED])")
    }
}
impl AccessContext {
    /// Pseudonymous controlled-audit identity; credentials and issuer subjects stay out of logs.
    pub fn subject_fingerprint(&self) -> Option<String> {
        self.identity.as_ref().map(|identity| {
            let mut bytes = identity.issuer.as_bytes().to_vec();
            bytes.push(0);
            bytes.extend_from_slice(identity.subject.as_bytes());
            convex_engine::ports::snapshot::content_digest(b"convex.access-subject.v1\0", &bytes)
        })
    }
    /// Accepted verified identity, absent only in explicit local development.
    pub fn identity(&self) -> Option<&VerifiedIdentity> {
        self.identity.as_ref()
    }
}
struct SubjectUsage {
    window: Instant,
    count: u32,
    requests: usize,
    sessions: usize,
}
struct Usage {
    window: Instant,
    /// Credential verifications in this window.
    verifications: u32,
    /// Identity-provider readiness probes in this window.
    probes: u32,
    subjects: BTreeMap<String, SubjectUsage>,
}

/// Credential verifications the process admits per monotonic minute: each request that
/// presents a bearer token, and each remote WS refresh (D78).
pub const VERIFICATIONS_PER_MINUTE: u32 = 6_000;
/// Identity-provider readiness probes per monotonic minute. They have their own window,
/// so authenticated traffic cannot make readiness fail (D78).
pub const READINESS_PROBES_PER_MINUTE: u32 = 600;
struct PolicyState {
    document: Option<PolicyDocument>,
    available: bool,
}
/// One transport-owned access controller; no background task or mutable domain clock.
pub struct AccessControl {
    verifier: Option<Arc<dyn IdentityVerifier>>,
    local: bool,
    policy: RwLock<PolicyState>,
    changes: watch::Sender<u64>,
    usage: Mutex<Usage>,
    requests: Arc<Semaphore>,
    sessions: Arc<Semaphore>,
}
/// RAII quota receipt: cancellation, failed upgrade and disconnect release capacity.
pub struct Admission {
    access: Arc<AccessControl>,
    subject: Option<String>,
    session: bool,
    _permit: OwnedSemaphorePermit,
}
impl Drop for Admission {
    fn drop(&mut self) {
        if let Some(subject) = &self.subject {
            if let Ok(mut usage) = self.access.usage.lock() {
                if let Some(row) = usage.subjects.get_mut(subject) {
                    if self.session {
                        row.sessions = row.sessions.saturating_sub(1);
                    } else {
                        row.requests = row.requests.saturating_sub(1);
                    }
                }
            }
        }
    }
}
impl AccessControl {
    /// Resolve configured identity credentials and policy before any application storage opens.
    pub async fn from_settings(
        settings: &identity::AccessSettings,
    ) -> Result<Arc<Self>, AccessError> {
        settings.validate()?;
        if settings.mode == identity::AccessMode::LocalDevelopment {
            return Ok(Self::local_development());
        }
        let configured = settings
            .introspection
            .clone()
            .ok_or(AccessError::InvalidPolicy)?;
        let document = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            Self::read_policy(
                settings
                    .policy_file
                    .as_deref()
                    .ok_or(AccessError::InvalidPolicy)?,
            ),
        )
        .await
        .map_err(|_| AccessError::Unavailable)??;
        if document.issuer != configured.issuer
            || document.audience != configured.audience
            || now_seconds()? >= document.valid_until
        {
            return Err(AccessError::InvalidPolicy);
        }
        let verifier = identity::IntrospectionVerifier::new(configured)?;
        tokio::time::timeout(std::time::Duration::from_secs(3), verifier.probe())
            .await
            .map_err(|_| AccessError::Unavailable)??;
        Self::new(verifier, document)
    }
    /// Read strict policy with a 2 MiB file bound and duplicate-member rejection.
    pub async fn read_policy(path: &str) -> Result<PolicyDocument, AccessError> {
        use tokio::io::AsyncReadExt;
        let file = tokio::fs::File::open(path)
            .await
            .map_err(|_| AccessError::Unavailable)?;
        let mut bytes = Vec::new();
        file.take(2 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| AccessError::Unavailable)?;
        if bytes.len() > 2 * 1024 * 1024 {
            return Err(AccessError::InvalidPolicy);
        }
        http::strict_json(&bytes).map_err(|_| AccessError::InvalidPolicy)?;
        let document: PolicyDocument =
            serde_json::from_slice(&bytes).map_err(|_| AccessError::InvalidPolicy)?;
        document.validate()?;
        Ok(document)
    }
    /// Owned policy watcher: one-second local reload, fail closed on read/parse/regression.
    pub async fn watch_policy(self: Arc<Self>, path: String, mut stop: watch::Receiver<bool>) {
        let mut timer = tokio::time::interval(std::time::Duration::from_secs(1));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                changed = stop.changed() => { if changed.is_err() || *stop.borrow() { break; } }
                _ = timer.tick() => {
                    let loaded = tokio::time::timeout(std::time::Duration::from_secs(2), Self::read_policy(&path)).await;
                    match loaded { Ok(Ok(document)) => { if let Some(version) = self.policy_version() { if self.replace_policy(version, document).is_err() { self.mark_policy_unavailable(); } } }, _ => self.mark_policy_unavailable() }
                }
            }
        }
    }
    fn build(
        verifier: Option<Arc<dyn IdentityVerifier>>,
        local: bool,
        document: Option<PolicyDocument>,
    ) -> Arc<Self> {
        let (changes, _) = watch::channel(0);
        Arc::new(Self {
            verifier,
            local,
            policy: RwLock::new(PolicyState {
                available: document.is_some(),
                document,
            }),
            changes,
            usage: Mutex::new(Usage {
                window: Instant::now(),
                verifications: 0,
                probes: 0,
                subjects: BTreeMap::new(),
            }),
            requests: Arc::new(Semaphore::new(64)),
            sessions: Arc::new(Semaphore::new(128)),
        })
    }
    /// Safe default for every API constructor.
    pub fn deny_all() -> Arc<Self> {
        Self::build(None, false, None)
    }
    /// Explicit insecure local fixture mode. Server listeners restrict it to opted-in loopback.
    pub fn local_development() -> Arc<Self> {
        Self::build(None, true, None)
    }
    /// Compose a verifier and an operator-approved bounded policy.
    pub fn new(
        verifier: Arc<dyn IdentityVerifier>,
        policy: PolicyDocument,
    ) -> Result<Arc<Self>, AccessError> {
        policy.validate()?;
        Ok(Self::build(Some(verifier), false, Some(policy)))
    }
    /// Whether the composition deliberately bypasses authentication for local development.
    pub fn is_local_development(&self) -> bool {
        self.local
    }
    /// Whether a real verification boundary was supplied (not a readiness certificate).
    pub fn is_configured(&self) -> bool {
        self.verifier.is_some()
    }
    /// Subscribe to local policy changes for prompt idle-session reevaluation.
    pub fn subscribe_policy(&self) -> watch::Receiver<u64> {
        self.changes.subscribe()
    }
    /// Monotonic current revision; missing/poisoned policy yields no version.
    pub fn policy_version(&self) -> Option<u64> {
        self.policy
            .read()
            .ok()?
            .document
            .as_ref()
            .map(|d| d.version)
    }
    /// Replace grants atomically by expected revision. Same-content retries are harmless.
    pub fn replace_policy(
        &self,
        expected: u64,
        document: PolicyDocument,
    ) -> Result<(), AccessError> {
        document.validate()?;
        let mut state = self.policy.write().map_err(|_| AccessError::Unavailable)?;
        let previous = state.document.as_ref().ok_or(AccessError::Unavailable)?;
        if previous == &document {
            if !state.available {
                state.available = true;
                self.changes.send_modify(|v| *v = v.saturating_add(1));
            }
            return Ok(());
        }
        if previous.version != expected
            || document.version <= previous.version
            || document.issuer != previous.issuer
            || document.audience != previous.audience
        {
            return Err(AccessError::Conflict);
        }
        let version = document.version;
        state.document = Some(document);
        state.available = true;
        self.changes.send_modify(|v| *v = v.saturating_add(1));
        tracing::info!(target: "convex_server::access_audit", policy_version = version, code = "POLICY_ACTIVATED", "Access policy audit");
        Ok(())
    }
    /// Mark a required policy source unavailable; never keep granting indefinitely on a broken reload.
    pub fn mark_policy_unavailable(&self) {
        if let Ok(mut state) = self.policy.write() {
            if state.available {
                state.available = false;
                self.changes.send_modify(|v| *v = v.saturating_add(1));
                tracing::warn!(target: "convex_server::access_audit", code = "POLICY_UNAVAILABLE", "Access policy audit");
            }
        }
    }
    /// Verify a request credential; unknown headers never create an identity.
    pub async fn authenticate(
        &self,
        token: Option<BearerToken>,
    ) -> Result<AccessContext, AccessError> {
        if self.local {
            return Ok(AccessContext {
                identity: None,
                token: None,
                local: true,
                verified_at: Instant::now(),
            });
        }
        let token = token.ok_or(AccessError::Unauthenticated)?;
        let verifier = self.verifier.as_ref().ok_or(AccessError::Unauthenticated)?;
        self.charge_identity_call(false)?;
        let identity =
            tokio::time::timeout(std::time::Duration::from_secs(3), verifier.verify(&token))
                .await
                .map_err(|_| AccessError::Unavailable)??;
        let ctx = AccessContext {
            identity: Some(identity),
            token: Some(token),
            local: false,
            verified_at: Instant::now(),
        };
        self.check_identity(&ctx, now_seconds()?)?;
        Ok(ctx)
    }
    /// Refresh remote revocation/scopes. No cache grants access after refresh failure.
    pub async fn refresh(&self, ctx: &AccessContext) -> Result<AccessContext, AccessError> {
        let _permit = self.admit_attempt()?;
        let refreshed = self.authenticate(ctx.token.clone()).await?;
        if let (Some(original), Some(current)) = (&ctx.identity, &refreshed.identity) {
            if original.issuer != current.issuer
                || original.subject != current.subject
                || original.audience != current.audience
            {
                return Err(AccessError::Unauthenticated);
            }
        }
        Ok(refreshed)
    }
    /// WS credentials have at most one second of remote-status freshness.
    pub fn session_fresh(&self, ctx: &AccessContext) -> bool {
        ctx.local || ctx.verified_at.elapsed() < std::time::Duration::from_secs(1)
    }
    /// Renew before the remote-status lease ends, keeping normal busy streams connected.
    pub fn session_needs_refresh(&self, ctx: &AccessContext) -> bool {
        !ctx.local && ctx.verified_at.elapsed() >= std::time::Duration::from_millis(750)
    }
    fn check_identity(&self, ctx: &AccessContext, now: i64) -> Result<(), AccessError> {
        if self.local && ctx.local {
            return Ok(());
        }
        let identity = ctx.identity.as_ref().ok_or(AccessError::Unauthenticated)?;
        if now >= identity.expires_at {
            return Err(AccessError::Unauthenticated);
        }
        let state = self.policy.read().map_err(|_| AccessError::Unavailable)?;
        let policy = state.document.as_ref().ok_or(AccessError::Unavailable)?;
        if !state.available || now >= policy.valid_until {
            return Err(AccessError::Unavailable);
        }
        if identity.issuer != policy.issuer
            || identity.audience != policy.audience
            || !policy::valid_subject(&identity.subject)
            || identity.scopes.len() > 64
            || identity
                .scopes
                .iter()
                .any(|s| !policy::valid_scope(s) || s == "*")
        {
            return Err(AccessError::Unauthenticated);
        }
        Ok(())
    }
    /// Check latest grants at supplied time; pure and deterministic apart from the current policy read.
    pub fn authorize_at(
        &self,
        ctx: &AccessContext,
        now: i64,
        action: Action,
        data: &[DataScope],
        resources: &[Resource],
    ) -> Result<(), AccessError> {
        self.check_identity(ctx, now)?;
        if self.local && ctx.local {
            return Ok(());
        }
        let identity = ctx.identity.as_ref().ok_or(AccessError::Unauthenticated)?;
        let state = self.policy.read().map_err(|_| AccessError::Unavailable)?;
        let policy = state.document.as_ref().ok_or(AccessError::Unavailable)?;
        if !state.available || now >= policy.valid_until {
            return Err(AccessError::Unavailable);
        }
        let result = policy
            .grants
            .iter()
            .find(|g| g.subject == identity.subject)
            .ok_or(AccessError::Forbidden)?
            .check(identity, now, action, data, resources);
        tracing::debug!(target: "convex_server::access_audit", policy_version = policy.version, subject = ?ctx.subject_fingerprint(), action = ?action, allowed = result.is_ok(), "Authorization decision");
        result
    }
    /// Check current identity, roles and all resources at admission/delivery time.
    pub fn authorize(
        &self,
        ctx: &AccessContext,
        action: Action,
        data: &[DataScope],
        resources: &[Resource],
    ) -> Result<(), AccessError> {
        self.authorize_at(ctx, now_seconds()?, action, data, resources)
    }
    /// The future durable job transport must pass this check before enqueue/read/cancel.
    pub fn authorize_job(
        &self,
        ctx: &AccessContext,
        action: Action,
        book: &str,
        resources: &[Resource],
    ) -> Result<(), AccessError> {
        if !matches!(
            action,
            Action::JobRead | Action::JobSubmit | Action::JobCancel
        ) || resources.len() > 1024
        {
            return Err(AccessError::Forbidden);
        }
        let mut required = Vec::with_capacity(resources.len() + 1);
        required.push(Resource::new(ResourceKind::Book, book));
        required.extend_from_slice(resources);
        self.authorize(
            ctx,
            action,
            &[DataScope::Books, DataScope::Results],
            &required,
        )
    }
    /// Global bound of 64 operations in flight: every request, preflight, inbound WS
    /// message, WS refresh and readiness probe holds one while it runs. It counts nothing;
    /// the per-minute windows count only network work toward the identity provider (D78).
    pub fn admit_attempt(&self) -> Result<OwnedSemaphorePermit, AccessError> {
        self.requests
            .clone()
            .try_acquire_owned()
            .map_err(|_| AccessError::Quota)
    }
    /// Charge one identity-provider call to its per-minute window, before the call.
    fn charge_identity_call(&self, probe: bool) -> Result<(), AccessError> {
        let mut usage = self.usage.lock().map_err(|_| AccessError::Unavailable)?;
        if usage.window.elapsed() >= std::time::Duration::from_secs(60) {
            usage.window = Instant::now();
            usage.verifications = 0;
            usage.probes = 0;
        }
        let (used, limit) = if probe {
            (&mut usage.probes, READINESS_PROBES_PER_MINUTE)
        } else {
            (&mut usage.verifications, VERIFICATIONS_PER_MINUTE)
        };
        if *used >= limit {
            return Err(AccessError::Quota);
        }
        *used += 1;
        Ok(())
    }
    fn quotas(&self, ctx: &AccessContext) -> Result<Quotas, AccessError> {
        self.check_identity(ctx, now_seconds()?)?;
        if self.local && ctx.local {
            return Ok(Quotas::default());
        }
        let identity = ctx.identity.as_ref().ok_or(AccessError::Unauthenticated)?;
        let state = self.policy.read().map_err(|_| AccessError::Unavailable)?;
        state
            .document
            .as_ref()
            .and_then(|p| {
                p.grants.iter().find(|g| {
                    g.subject == identity.subject
                        && g.valid_until > now_seconds().unwrap_or(i64::MAX)
                })
            })
            .map(|g| g.quotas)
            .ok_or(AccessError::Forbidden)
    }
    /// Current total topic quota, also checked after policy changes.
    pub fn subscription_limit(&self, ctx: &AccessContext) -> Result<usize, AccessError> {
        Ok(self.quotas(ctx)?.subscriptions)
    }
    /// Per-subject admission; rate use is shared between HTTP and WS messages.
    pub fn admit(
        self: &Arc<Self>,
        ctx: &AccessContext,
        session: bool,
        permit: OwnedSemaphorePermit,
    ) -> Result<Admission, AccessError> {
        let quotas = self.quotas(ctx)?;
        let subject = ctx.identity.as_ref().map(|id| id.subject.clone());
        if let Some(subject) = &subject {
            let mut usage = self.usage.lock().map_err(|_| AccessError::Unavailable)?;
            if !usage.subjects.contains_key(subject) && usage.subjects.len() >= 512 {
                usage.subjects.retain(|_, r| {
                    r.requests != 0
                        || r.sessions != 0
                        || r.window.elapsed() < std::time::Duration::from_secs(60)
                });
            }
            if !usage.subjects.contains_key(subject) && usage.subjects.len() >= 512 {
                return Err(AccessError::Quota);
            }
            let row = usage
                .subjects
                .entry(subject.clone())
                .or_insert(SubjectUsage {
                    window: Instant::now(),
                    count: 0,
                    requests: 0,
                    sessions: 0,
                });
            if row.window.elapsed() >= std::time::Duration::from_secs(60) {
                row.window = Instant::now();
                row.count = 0;
            }
            if row.count >= quotas.requests_per_minute
                || (session && row.sessions >= quotas.websocket_sessions)
                || (!session && row.requests >= quotas.concurrent_requests)
            {
                return Err(AccessError::Quota);
            }
            row.count += 1;
            if session {
                row.sessions += 1;
            } else {
                row.requests += 1;
            }
        }
        let permit = if session {
            self.sessions.clone().try_acquire_owned().map_err(|_| {
                if let Some(subject) = &subject {
                    if let Ok(mut usage) = self.usage.lock() {
                        if let Some(row) = usage.subjects.get_mut(subject) {
                            row.sessions = row.sessions.saturating_sub(1);
                        }
                    }
                }
                AccessError::Quota
            })?
        } else {
            permit
        };
        Ok(Admission {
            access: self.clone(),
            subject,
            session,
            _permit: permit,
        })
    }
    /// Charge and release one WS message admission without retaining a second session lease.
    pub fn admit_message(self: &Arc<Self>, ctx: &AccessContext) -> Result<Admission, AccessError> {
        let permit = self.admit_attempt()?;
        self.admit(ctx, false, permit)
    }
    /// Separate configured policy and real identity availability for readiness.
    pub async fn health(&self) -> AccessHealth {
        if self.local {
            return AccessHealth {
                required: false,
                policy_ready: true,
                identity_ready: true,
            };
        }
        let policy_ready = self.policy.read().ok().is_some_and(|s| {
            s.available
                && s.document
                    .as_ref()
                    .is_some_and(|d| now_seconds().is_ok_and(|now| now < d.valid_until))
        });
        let identity_ready = if let Some(verifier) = &self.verifier {
            match self
                .admit_attempt()
                .and_then(|permit| self.charge_identity_call(true).map(|()| permit))
            {
                Ok(_permit) => {
                    tokio::time::timeout(std::time::Duration::from_secs(3), verifier.probe())
                        .await
                        .is_ok_and(|r| r.is_ok())
                }
                Err(_) => false,
            }
        } else {
            false
        };
        AccessHealth {
            required: true,
            policy_ready,
            identity_ready,
        }
    }
}
/// Low-cardinality availability, never credentials or entitlement contents.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct AccessHealth {
    /// Required composition, false only for explicit local development.
    pub required: bool,
    /// Valid available policy.
    pub policy_ready: bool,
    /// Real verifier availability.
    pub identity_ready: bool,
}
pub(crate) fn now_seconds() -> Result<i64, AccessError> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .ok_or(AccessError::Unavailable)
}
