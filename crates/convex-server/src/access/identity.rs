//! OAuth 2.0 resource-server introspection. Login/discovery is owned by the identity provider.
use super::{
    policy::{valid_id, valid_scope, valid_subject},
    AccessError, BearerToken, IdentityVerifier, VerifiedIdentity,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fmt, net::IpAddr, sync::Arc, time::Duration};
use tokio::io::AsyncReadExt;

/// Explicit access mode. Insecure mode requires opted-in loopback startup.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessMode {
    /// No protected request succeeds without configured verification.
    #[default]
    Required,
    /// Explicit local tests/development only.
    LocalDevelopment,
}
/// TLS deployment boundary; the executable itself always binds plain HTTP on loopback.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TlsBoundary {
    /// Local isolated client only.
    #[default]
    LocalOnly,
    /// External TLS proxy with inaccessible loopback upstream.
    Proxy,
}
/// Explicit RFC 7662 endpoint and client secret reference, without persisted secret values.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntrospectionSettings {
    /// Operator-selected endpoint; redirects/discovery are disabled.
    pub endpoint: String,
    /// Exact trusted token issuer, HTTPS identifier.
    pub issuer: String,
    /// API audience, distinct from a login client ID.
    pub audience: String,
    /// Registered introspection client identifier.
    pub client_id: String,
    /// env://NAME or file:///absolute/path, reread for rotation.
    pub client_secret_ref: String,
    /// Explicit test/replay exception permitting HTTP to an IP-literal loopback fixture.
    #[serde(default)]
    pub allow_http_loopback: bool,
}
impl fmt::Debug for IntrospectionSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("IntrospectionSettings([REDACTED])")
    }
}
/// Strict restart-only process access settings; grants live in a separate versioned policy.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct AccessSettings {
    /// Safe default is required verification.
    pub mode: AccessMode,
    /// Required bounded schema-1 policy file.
    pub policy_file: Option<String>,
    /// External identity integration.
    pub introspection: Option<IntrospectionSettings>,
    /// Exact origins only; empty disables browser cross-origin access.
    pub cors_origins: Vec<String>,
    /// Declared TLS ingress boundary.
    pub tls_boundary: TlsBoundary,
}
impl fmt::Debug for AccessSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.redacted().fmt(f)
    }
}
/// Secret/path/URL-free process inspection.
#[derive(Debug, Serialize)]
pub struct RedactedAccessSettings {
    /// Mode.
    pub mode: AccessMode,
    /// Whether a policy location was supplied.
    pub policy_configured: bool,
    /// Whether identity integration was supplied.
    pub identity_configured: bool,
    /// Exact number of allowed origins.
    pub allowed_origin_count: usize,
    /// Ingress boundary.
    pub tls_boundary: TlsBoundary,
}
impl AccessSettings {
    /// Redacted inspection never prints external URLs or credential references.
    pub fn redacted(&self) -> RedactedAccessSettings {
        RedactedAccessSettings {
            mode: self.mode,
            policy_configured: self.policy_file.is_some(),
            identity_configured: self.introspection.is_some(),
            allowed_origin_count: self.cors_origins.len(),
            tls_boundary: self.tls_boundary,
        }
    }
    /// Validate syntax. Actual missing composition is rejected by startup, before storage.
    pub fn validate(&self) -> Result<(), AccessError> {
        if self.cors_origins.len() > 32
            || self.cors_origins.iter().collect::<BTreeSet<_>>().len() != self.cors_origins.len()
        {
            return Err(AccessError::InvalidPolicy);
        }
        for origin in &self.cors_origins {
            let parsed = reqwest::Url::parse(origin).map_err(|_| AccessError::InvalidPolicy)?;
            if origin.len() > 512
                || parsed.origin().ascii_serialization() != *origin
                || !parsed.username().is_empty()
                || parsed.password().is_some()
                || parsed.host_str().is_none()
                || (parsed.scheme() != "https"
                    && !(self.tls_boundary == TlsBoundary::LocalOnly
                        && parsed.scheme() == "http"
                        && url_is_loopback(&parsed)))
            {
                return Err(AccessError::InvalidPolicy);
            }
        }
        if self
            .policy_file
            .as_ref()
            .is_some_and(|p| p.is_empty() || p.len() > 4096 || p.contains('\0'))
        {
            return Err(AccessError::InvalidPolicy);
        }
        if let Some(settings) = &self.introspection {
            settings.validate()?;
        }
        if self.mode == AccessMode::LocalDevelopment
            && (self.policy_file.is_some()
                || self.introspection.is_some()
                || self.tls_boundary != TlsBoundary::LocalOnly)
        {
            return Err(AccessError::InvalidPolicy);
        }
        Ok(())
    }
}
fn url_is_loopback(url: &reqwest::Url) -> bool {
    url.host_str().is_some_and(|host| {
        host.trim_matches(['[', ']'])
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
    })
}
impl IntrospectionSettings {
    /// Require an exact HTTPS endpoint, unless the explicit loopback fixture exception is selected.
    pub fn validate(&self) -> Result<(), AccessError> {
        let endpoint =
            reqwest::Url::parse(&self.endpoint).map_err(|_| AccessError::InvalidPolicy)?;
        let issuer = reqwest::Url::parse(&self.issuer).map_err(|_| AccessError::InvalidPolicy)?;
        if self.endpoint.len() > 2048
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.fragment().is_some()
            || endpoint.query().is_some()
            || (endpoint.scheme() != "https"
                && !(self.allow_http_loopback
                    && endpoint.scheme() == "http"
                    && url_is_loopback(&endpoint)))
            || self.issuer.len() > 512
            || issuer.scheme() != "https"
            || issuer.host_str().is_none()
            || !issuer.username().is_empty()
            || issuer.password().is_some()
            || issuer.query().is_some()
            || issuer.fragment().is_some()
            || !valid_id(&self.audience)
            || self.audience == "*"
            || self.client_id.is_empty()
            || self.client_id.len() > 256
            || !self
                .client_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b))
        {
            return Err(AccessError::InvalidPolicy);
        }
        validate_secret_reference(&self.client_secret_ref)
    }
}
fn validate_secret_reference(reference: &str) -> Result<(), AccessError> {
    if let Some(name) = reference.strip_prefix("env://") {
        if !name.is_empty()
            && name.len() <= 128
            && !name.starts_with("CONVEX_")
            && name
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        {
            return Ok(());
        }
    }
    if let Some(path) = reference.strip_prefix("file://") {
        if path.len() <= 4096
            && std::path::Path::new(path).is_absolute()
            && !path.contains('\0')
            && !path.chars().any(char::is_control)
        {
            return Ok(());
        }
    }
    Err(AccessError::InvalidPolicy)
}
async fn resolve_secret(reference: &str) -> Result<String, AccessError> {
    let secret = if let Some(name) = reference.strip_prefix("env://") {
        std::env::var(name).map_err(|_| AccessError::Unavailable)?
    } else if let Some(path) = reference.strip_prefix("file://") {
        let file = tokio::fs::File::open(path)
            .await
            .map_err(|_| AccessError::Unavailable)?;
        let mut bytes = Vec::new();
        file.take(4097)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| AccessError::Unavailable)?;
        if bytes.len() > 4096 {
            return Err(AccessError::Unavailable);
        }
        String::from_utf8(bytes)
            .map_err(|_| AccessError::Unavailable)?
            .trim_end_matches(['\r', '\n'])
            .to_owned()
    } else {
        return Err(AccessError::InvalidPolicy);
    };
    if secret.is_empty() || secret.len() > 4096 || secret.chars().any(char::is_control) {
        return Err(AccessError::Unavailable);
    }
    Ok(secret)
}
/// Real external adapter. Each verification asks the issuer; positive token results are not cached.
pub struct IntrospectionVerifier {
    settings: IntrospectionSettings,
    client: reqwest::Client,
}
impl IntrospectionVerifier {
    /// Build using standard TLS validation and a two-second total request budget.
    pub fn new(settings: IntrospectionSettings) -> Result<Arc<Self>, AccessError> {
        settings.validate()?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .connect_timeout(Duration::from_secs(1))
            .timeout(Duration::from_secs(2))
            .pool_max_idle_per_host(8)
            .https_only(!settings.allow_http_loopback)
            .build()
            .map_err(|_| AccessError::Unavailable)?;
        Ok(Arc::new(Self { settings, client }))
    }
    async fn introspect(&self, token: &str) -> Result<Claims, AccessError> {
        let secret = resolve_secret(&self.settings.client_secret_ref).await?;
        // RFC 6749 client_secret_basic encodes each component before base64.
        let mut encoded =
            reqwest::Url::parse("https://localhost/").map_err(|_| AccessError::Unavailable)?;
        encoded.query_pairs_mut().append_pair("", &secret);
        let password = encoded
            .query()
            .and_then(|q| q.strip_prefix('='))
            .ok_or(AccessError::Unavailable)?;
        let mut response = self
            .client
            .post(&self.settings.endpoint)
            .basic_auth(&self.settings.client_id, Some(password))
            .header("accept", "application/json")
            .form(&[("token", token), ("token_type_hint", "access_token")])
            .send()
            .await
            .map_err(|_| AccessError::Unavailable)?;
        if response.status() != reqwest::StatusCode::OK
            || response.content_length().is_some_and(|n| n > 16384)
            || !response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| {
                    v.split(';')
                        .next()
                        .is_some_and(|v| v.trim().eq_ignore_ascii_case("application/json"))
                })
        {
            return Err(AccessError::Unavailable);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| AccessError::Unavailable)?
        {
            if bytes.len().saturating_add(chunk.len()) > 16384 {
                return Err(AccessError::Unavailable);
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| AccessError::Unavailable)
    }
}
#[derive(Deserialize)]
struct Claims {
    active: bool,
    sub: Option<String>,
    iss: Option<String>,
    aud: Option<Audience>,
    exp: Option<i64>,
    nbf: Option<i64>,
    iat: Option<i64>,
    scope: Option<String>,
    token_type: Option<String>,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}
#[async_trait]
impl IdentityVerifier for IntrospectionVerifier {
    async fn verify(&self, token: &BearerToken) -> Result<VerifiedIdentity, AccessError> {
        let claims = self.introspect(token.expose()).await?;
        if !claims.active {
            return Err(AccessError::Unauthenticated);
        }
        let now = super::now_seconds()?;
        let subject = claims
            .sub
            .filter(|s| valid_subject(s) && s != "*")
            .ok_or(AccessError::Unauthenticated)?;
        let expiry = claims
            .exp
            .filter(|expiry| now < *expiry)
            .ok_or(AccessError::Unauthenticated)?;
        let audience = match claims.aud {
            Some(Audience::One(ref value)) => value == &self.settings.audience,
            Some(Audience::Many(ref values)) => {
                values.len() <= 32 && values.iter().any(|v| v == &self.settings.audience)
            }
            None => false,
        };
        if claims.iss.as_deref() != Some(&self.settings.issuer)
            || !audience
            || claims.nbf.is_some_and(|time| time > now)
            || claims.iat.is_some_and(|time| time > now)
            || !claims
                .token_type
                .as_deref()
                .is_some_and(|t| t.eq_ignore_ascii_case("bearer"))
        {
            return Err(AccessError::Unauthenticated);
        }
        let scope = claims.scope.unwrap_or_default();
        if scope.len() > 2048 {
            return Err(AccessError::Unauthenticated);
        }
        let tokens: Vec<_> = scope.split(' ').collect();
        if !scope.is_empty() && tokens.iter().any(|s| !valid_scope(s)) {
            return Err(AccessError::Unauthenticated);
        }
        let scopes: BTreeSet<_> = tokens
            .into_iter()
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        if scopes.len() > 64 || scopes.iter().any(|s| !valid_scope(s) || s == "*") {
            return Err(AccessError::Unauthenticated);
        }
        Ok(VerifiedIdentity {
            issuer: self.settings.issuer.clone(),
            subject,
            audience: self.settings.audience.clone(),
            expires_at: expiry,
            scopes,
        })
    }
    async fn probe(&self) -> Result<(), AccessError> {
        let claims = self.introspect("convex-invalid-readiness-probe").await?;
        if claims.active {
            return Err(AccessError::Unavailable);
        }
        Ok(())
    }
}
