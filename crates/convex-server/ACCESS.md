# Convex access control and readiness

Convex checks authentication, current role and resource grants, and quotas before dispatching protected requests. Every router constructor defaults to denied access. The executable resolves its identity integration and access policy before opening application storage. This is the R0 access boundary. The production profile requires it: `access.mode = "required"` with a policy file and introspection, behind `tls_boundary = "proxy"`. Strategy readiness stays false.

## Start an authenticated replay server

Prepare the exact database, plan and quotes using the [configuration walkthrough](../convex-engine/CONFIGURATION.md). Register an introspection client with your identity provider and select an API access-token audience. User login belongs to the OIDC provider/client; Convex accepts API bearer access tokens through OAuth introspection. An ID token for a login client does not satisfy the selected API audience/type. Introspection works with opaque or structured tokens and queries their current active state. [RFC 7662](https://www.rfc-editor.org/rfc/rfc7662.txt), [OIDC Core](https://openid.net/specs/openid-connect-core-1_0.html).

Create process TOML with your actual paths, issuer, audience, client registration and exact browser origins:

```toml
profile = "replay"
host = "127.0.0.1"
port = 8080
storage_path = "/srv/convex/inputs.redb"
runtime_plan = "/srv/convex/plan.json"
quotes_file = "/srv/convex/quotes.jsonl"

[access]
mode = "required"
policy_file = "/srv/convex/access.json"
tls_boundary = "proxy"
cors_origins = ["https://console.example.com"]

[access.introspection]
endpoint = "https://identity.example.com/oauth/introspect"
issuer = "https://identity.example.com"
audience = "convex-api"
client_id = "convex-server"
client_secret_ref = "file:///run/secrets/convex-introspection"
allow_http_loopback = false
```

Secret values never belong in TOML, policy JSON, URLs or command-line arguments. `file:///absolute/path` reads at most 4096 bytes and removes a trailing line ending; replace the file atomically for rotation. `env://IDP_CLIENT_SECRET` reads a process environment value; replacing that environment externally requires restarting the process. Environment secret names must not start with `CONVEX_`, whose namespace contains only declared process settings. Local secret resolution currently covers these two explicit references. Vault/KMS integration can implement the same identity boundary; it is not certified here.

The schema-1 policy below illustrates independent human/service grants. Replace subjects with the issuer's exact case-sensitive `sub` values, resources with canonical IDs, and validity dates with your approved expiry. `1893456000` denotes 2030-01-01 UTC, solely an example. The operator controls the policy file and its containing directory. Policy provenance is operator metadata, not an authenticated network policy-administration workflow.

```json
{
  "schema_version": 1,
  "version": 1,
  "issuer": "https://identity.example.com",
  "audience": "convex-api",
  "valid_until": 1893456000,
  "actor": "operations",
  "reason": "Approved initial replay access",
  "grants": [
    {
      "subject": "user-subject-42",
      "kind": "human",
      "roles": ["reader", "analyst", "book_writer"],
      "data": ["reference", "market", "results", "books"],
      "resources": {
        "instrument": ["BOND-001"],
        "curve": ["*"],
        "source": ["*"],
        "book": ["BOOK-A"]
      },
      "token_scopes": ["convex.api"],
      "valid_until": 1893456000,
      "quotas": {
        "requests_per_minute": 600,
        "concurrent_requests": 8,
        "websocket_sessions": 4,
        "subscriptions": 256
      }
    },
    {
      "subject": "pricing-worker",
      "kind": "service",
      "roles": ["reader"],
      "data": ["reference"],
      "resources": { "instrument": ["BOND-001"], "source": ["*"] },
      "token_scopes": ["convex.api"],
      "valid_until": 1893456000
    }
  ]
}
```

The policy grants at most 256 distinct subjects. Subjects are opaque nonempty ASCII strings of at most 255 bytes, without controls; provider delimiters such as `auth0|123` are retained exactly. A grant has at most six roles/data classes, 32 required OAuth scopes and 1024 resource identifiers across namespaces. Identifiers are bounded ASCII strings; each namespace may contain at most 256 entries. A literal `"*"` must be the sole entry in its namespace and means whole-dataset access. Empty or omitted resource sets grant nothing. Required token scopes are nonempty and all must remain present. Unknown fields, duplicate JSON members/subjects/grants, invalid expiry/bounds and mixed wildcard/exact sets are rejected. Policy files are bounded to 2 MiB; process TOML remains bounded to 64 KiB.

Inspect syntax and the redacted process settings, then start:

```sh
cargo build --locked -p convex-server --bins
cargo run --locked -p convex-server --bin convex-config -- process explain /srv/convex/server.toml
cargo run --locked -p convex-server -- /srv/convex/server.toml
```

Process inspection validates settings without contacting the issuer; successful executable startup also requires a bounded authenticated introspection availability check. Use a native client SDK to put `Authorization: Bearer <access token>` on requests. Duplicate authorization headers, query tokens, cookies and forwarded identity/role headers cannot establish identity. Responses disable caching. The two WS aliases require the same header; browser clients that cannot supply it need an explicitly reviewed same-origin BFF/proxy integration. Query-token workarounds are unsupported.

## Role and data boundaries

No token claim grants a local role, and no role grants data/resources implicitly. `kind` classifies the explicitly approved subject binding; service and human identities use the same verified issuer/audience/subject boundary. There is no administrator bypass.

| Role | Operations within explicit data and resource grants |
|---|---|
| reader | Reference/market/result/book reads, job reads and WS connection |
| analyst | Reader operations, calculations and scoped job submit/cancel |
| reference_writer | Reference mutations, curve construction/deletion and market input writes (fixings, live quotes) |
| book_writer | Portfolio/position mutations |
| operator | Runtime/dependency and configuration inspection; configuration staging, activation and rollback |

Combine roles deliberately. `reference_writer` does not automatically include `reader`; `operator` does not authorize a calculation or grant market data. The separate data classes are `reference`, `market`, `results`, `books`, `configuration`, `operations`. Resource namespaces are instrument, curve, book, ETF, source and configuration. v1 `portfolio_id` maps exactly to the book namespace; the present portfolio store remains volatile and does not establish authoritative financial books.

| Transport operation | Additional boundary |
|---|---|
| Exact bond reads/writes | Instrument ID and referenced source IDs; reference data; corresponding role |
| Reference lists, identifier aliases/resolution, retained commit inspection | Whole instrument dataset; no unsafe post-pagination filtering |
| Reference master mutation | Whole instrument dataset when `master_changes` is nonempty |
| Curves | Exact curve or whole curve dataset for lists; market data and whole source access |
| v1 quotes/pricing and quote streams | Instrument plus reference/market/results; whole sources and curves because this API does not expose complete scoped dependency selection |
| Supplied runtime valuation | All selected instrument, curve, source and calibration `InputVersion` dependencies; reference/market/results and any book in the payload |
| Runtime result resync | Whole instruments/curves/sources and reference/market/results; the current endpoint returns the entire selected runtime |
| Live quote ingress | Instrument and source of every event; market data; `reference_writer` |
| Portfolio/position operations | Book ID and every instrument in input/returned content; nested batches are checked before dispatch |
| Portfolio analytics stream | Book plus whole underlying instrument/curve/source grants and market/results/books |
| Configuration inspection | Operator, configuration ID and configuration/reference/market grants; returned dependency IDs are checked |
| Configuration stage/activate/rollback | Operator (`ConfigurationWrite`), configuration ID and configuration/reference/market grants; every ID in the body is checked; bodies up to 4 MiB |
| Configuration validate/diff, history, versions and version diff | Operator; history, versions and version diff also need the configuration ID; validate also needs reference/market grants |
| Reference import upload | Reference writer; checked like a reference transaction, whole instrument dataset when the file has master changes; up to 8 MiB |
| Reference master listing | Whole instrument dataset and reference data |
| Runtime/dependencies/WS status | Operator and operations data |
| New or unclassified endpoints | Denied until their access contract is added to the closed route table |

Body checks reject cross-scope nested batches before any mutation. JSON responses are checked again against current grants before release, including dependencies revealed by stored contents. Original input/output bytes are preserved; authorization never rewrites financial numbers. Request JSON is bounded to 2 MiB, response inspection to 4 MiB, traversal to 65536 nodes and selected resource occurrences to 1024. Oversized/ambiguous requests fail explicitly; no partial data becomes an empty success. Existing quantitative/client economic contracts remain unchanged, including the separately enforced G04 synthetic-curve gap.

The `authorize_job` boundary applies the same identity/role/data checks to book and selected resources before job submit/read/cancel. Durable jobs and their ownership/result transport remain CVX-035; no successful placeholder jobs endpoint is introduced. An anonymous jobs URL is denied before route lookup, and an authenticated unclassified URL is denied too. CVX-035 must adopt this boundary with actual persisted book/job ownership and fresh authentication rather than accepting an arbitrary client book header.

## Expiry and revocation

HTTP admissions verify active token state against the selected issuer every time. This deployment requires exact issuer, API audience, subject, exclusive `exp`, a Bearer token type, and any supplied `nbf`/`iat` not in the future. There is no clock-skew allowance. Keep host clocks synchronized. Invalid identity returns 401; denied grants return 403; unavailable verification/policy returns 503; quota exhaustion returns 429 with `Retry-After: 60`.

WS subscriptions require current topic permissions. Delivery rechecks current local grants and token expiry; unfiltered curve notifications are protected. All topics combined are bounded to the current session quota, including the all-bonds wildcard. A rejected batch cannot leave partially installed subscriptions. Text and UTF-8 binary frames have the same denial semantics. Frames/messages are at most 64 KiB and writes/close have a two-second budget.

Each WS remote-status lease lasts at most one second. The session starts refreshing at 750 ms on a 250 ms timer. No data is admitted under an expired lease; a refresh failure closes the session. Remote revocation can therefore leave at most the remaining one-second lease of admission, with close delayed by bounded identity I/O. Local grants are reread for every subscription/delivery, and policy notifications reevaluate idle sessions. A refresh cannot switch issuer/subject/audience and reuse another subject's session quota. Decisions linearize at their authorization check; already accepted operations/network bytes may finish or arrive later. This is not instantaneous distributed revocation or a latency certificate.

Replace `access.json` atomically with a validated document at a higher version, recording actor/reason. The owned watcher loads it once per second. Once loaded, revoked grants stop subsequent admission/delivery immediately. Deleted, unreadable, malformed or regressing policy makes access unavailable; old grants do not remain active indefinitely. Restoring identical accepted content recovers availability; different content requires a higher version. Issuer/audience changes require restart. The embedded `replace_policy(expected_revision, document)` has atomic CAS, one-winner semantics and harmless exact retries. To restore older grants, publish them as a new revision. Expiry is exclusive at the supplied Unix-second boundary and independently applies to policy, grant and credential.

Audit logs record stable failure codes, policy revision, action/status and a domain-separated subject fingerprint. Debug-level decisions support controlled investigation; credential values, subjects, URLs, paths, raw remote failures and submitted WS JSON are not logged by this boundary. These are controlled operational traces, not a durable financial audit transaction. Retain original policy versions and operator approvals in your controlled configuration history; shared authenticated entitlement administration and audit retention remain later managed operations work.

## TLS and browser origins

The executable uses a plain HTTP loopback upstream and rejects a supplied non-loopback listener in both `start` and `serve_until`. For remote clients, terminate verified TLS at your selected proxy and prevent access to that upstream. `tls_boundary="proxy"` declares this deployment requirement; it does not inspect/certify the proxy's TLS configuration. Forwarded protocol/identity headers never waive bearer verification. TLS termination, certificates, upstream isolation, service token issuance, IdP availability and proxy request/connection limits need actual deployment certification. [Bearer transport requirements](https://www.rfc-editor.org/rfc/rfc6750.txt), [OAuth security guidance](https://www.rfc-editor.org/rfc/rfc9700.txt).

Introspection uses standard certificate/hostname validation, no redirects, no ambient proxies or credential-forwarding discovery, an exact configured endpoint, bounded JSON to 16 KiB and a two-second total HTTP budget. Client secrets are reread for rotation. Redirects/non-200/malformed/oversized replies fail closed. `allow_http_loopback=true` is an explicit local fixture exception: only an IP-literal loopback HTTP endpoint and `tls_boundary="local_only"` are allowed. It does not permit remote plain HTTP.

CORS defaults to no allowed origins. Supply at most 32 exact normalized origins; wildcard/null/userinfo/query/fragment forms are rejected. Proxy deployments require HTTPS origins. Local-only fixtures may use an HTTP IP-literal loopback origin. The origin guard runs before CORS preflight short-circuiting and also applies to WS upgrades. Allowed preflights expose only GET/HEAD/POST/PUT/DELETE and authorization/content-type/idempotency-key/if-match. They do not authenticate or dispatch a write. Cookies and credentialed CORS are disabled. [tower-http CORS](https://docs.rs/tower-http/0.6.6/tower_http/cors/struct.CorsLayer.html).

## Quotas and health

Global admission is capped at 64 operations in flight and 128 WS sessions. Every request, CORS preflight, inbound WS message, remote WS refresh and identity health probe holds one of the 64 while it runs. Per monotonic minute, the process admits at most 6000 credential verifications: each request that presents a bearer token, and each remote WS refresh, counts once. Requests without a credential, public probes, preflights, inbound WS messages and local development count nothing against it. Identity health probes have their own 600 a minute, so authenticated traffic cannot make `/ready` fail; past that, readiness reports the identity provider unavailable. A quote feeder should batch: one `POST /api/v1/market/quotes` carries up to 256 events. Session maxima do not promise sustained capacity. Subject quotas share an HTTP/WS-message rate window; maxima are 6000 admissions/minute, 64 concurrent requests, 32 WS sessions and 256 topics per session. Counters survive policy replacement; reducing quotas does not reset usage or silently revoke already accepted work. RAII receipts release concurrency on cancellation, failed upgrade and disconnect. The usage map is capped at 512 subjects across rotations and only removes inactive expired windows. These are per-process controls; shared quotas and perimeter denial-of-service protection require a managed deployment boundary.

| Endpoint | Meaning |
|---|---|
| /live, /health, /api/v1/health | Public process liveness; independent of dependencies |
| /ready | Public sanitized analytics readiness and required dependency classes |
| /api/v1/dependencies | Protected assessment from the same health calculation |
| /api/v1/runtime | Protected actual lifecycle, exact configuration/generation and failure classification |
| /metrics | Protected Prometheus text: stage latency histograms (operator role, operations grant); 503 when the process has no recorder |
| /strategy-ready | Public 503: risk reservations, ownership fencing, execution reconciliation and kill controls are not composed |

Readiness combines the real runtime's verified dependencies, snapshot quality/market context and acknowledged output with available unexpired policy and an actual identity endpoint probe. Reference/curve/checkpoint validity is owned by runtime startup/recovery; this endpoint does not invent separate successful database/provider tests. Missing context, stale data, failed delivery, identity outage and unavailable policy close analytics readiness while liveness can remain true. Captured/replay readiness certifies only the selected historical context; it cannot imply live freshness or strategy authorization.

## Development and embedding migration

Unauthenticated local fixtures require deliberate opt-in:

```sh
mkdir -p data
CONVEX_PROFILE=demo CONVEX_HOST=127.0.0.1 CONVEX_ACCESS_MODE=local_development \
  cargo run --locked -p convex-server
```

For local replay, add `CONVEX_ACCESS_MODE=local_development` to the configuration walkthrough's loopback startup command. This bypasses authentication for the local process, is visibly logged, and cannot be combined with proxy ingress. Keep it for isolated development.

`create_router` and `create_router_with_websocket` now deny protected requests by default. Embedders supply `create_router_with_access`; compose `AccessControl::new(verifier, policy)` and a narrow `IdentityVerifier` adapter, or resolve `from_settings`. `Server::with_access_control` validates composition for both listener paths. `Server::new` only creates usable unauthenticated access when explicit local development is selected; required mode needs the supplied verifier. Existing economic tests explicitly select local fixtures and continue exercising original production probes.

The implemented adapter was exercised against a real loopback HTTP issuer and the actual server binary, including secret-file rotation, revocation, missing policy, recovery, WebSocket closure and joined shutdown. No live identity vendor, TLS proxy deployment, multi-server quotas, security audit, penetration test, MSRV/platform matrix, soak or production latency envelope is certified by these fixtures.
