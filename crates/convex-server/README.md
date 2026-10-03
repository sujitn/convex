# Convex server configuration

The server currently runs only with an explicit demo profile. Production is the default and fails before opening storage because production provider composition is not implemented. Demo responses contain demonstration inputs; they do not establish production readiness.

For a local demonstration, run from the repository root:

```sh
mkdir -p data
CONVEX_PROFILE=demo CONVEX_HOST=127.0.0.1 cargo run --locked -p convex-server
```

To select a TOML file, supply its path after the binary name:

```sh
cargo run --locked -p convex-server -- /path/to/settings.toml
```

For example:

```toml
profile = "demo"
host = "127.0.0.1"
port = 8080
websocket_enabled = true
storage_path = "./data/convex.redb"
```

## Loading and validation

Configuration precedence is compiled defaults, selected TOML file, then environment overrides. With no explicit path, the server checks `config/convex.toml`; an absent default file permits environment-only startup. A missing explicit path, unreadable file, invalid setting, unknown TOML key or unknown `CONVEX_` environment variable fails startup. Each supplied file must itself be valid before overrides apply. TOML input is limited to 64 KiB.

All settings require a restart. Quantitative model choices, data overrides, managed configuration history and activation are separate future capabilities. No secret-valued setting is supported. Startup inspection redacts paths, and configuration errors omit submitted values and file contents.

| TOML key | Environment variable | Behavior |
|---|---|---|
| profile | CONVEX_PROFILE | `production` by default; only explicit `demo` currently starts |
| host | CONVEX_HOST | IPv4/IPv6 literal; default `0.0.0.0`; hostnames fail |
| port | CONVEX_PORT | Integer 0–65535; default 8080; zero selects an ephemeral port |
| websocket_enabled | CONVEX_WEBSOCKET_ENABLED | Boolean; environment accepts true/false or 1/0, case insensitive; default true |
| websocket_port | CONVEX_WEBSOCKET_PORT | Optional; accepted only when WebSockets are enabled and it equals the HTTP port |
| storage_path | CONVEX_STORAGE_PATH | Nonempty local redb path; default `./data/convex.redb`; parent directory must exist |
| quotes_file | CONVEX_QUOTES_FILE | Rejected: file provider composition is not implemented |
| curves_file | CONVEX_CURVES_FILE | Rejected: file provider composition is not implemented |
| fixings_file | CONVEX_FIXINGS_FILE | Rejected: file provider composition is not implemented |
| bonds_file | CONVEX_BONDS_FILE | Rejected: file provider composition is not implemented |

Startup logs the effective redacted configuration and, after a successful bind, the actual listener address. A disabled WebSocket setting removes `/ws`, `/api/v1/ws` and `/api/v1/ws/status`.

## Demo output labels

Demo HTTP responses, including WebSocket upgrades, carry `x-convex-profile: demo` and `x-convex-data-quality: demo`. These headers are exposed to browser clients through CORS. Every demo WebSocket JSON message additionally carries `profile: "demo"` and `data_quality: "demo"`; existing economic fields retain their semantics. The legacy numeric `quality` field is not a certification label.

The standalone router construction helpers retain their legacy embedding behavior. They do not validate provider capabilities or confer production certification. Use the validated server startup path; authoritative runtime and per-measure data quality remain separate implementation work.

## Migrating existing startup code

ServerConfig::from_env and apply_env_overrides now return Result. ServerConfig::from_file returns ConfigError, and Server::start returns StartupError. Callers must handle errors. Add an explicit demo profile for current local demonstrations. Previously ignored invalid values, unsupported settings and hostnames must be corrected; they no longer fall back silently.

No database schema or pricing convention changes are introduced by these startup changes.
