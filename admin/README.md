# Convex admin

The operator screens for a running `convex-server`. It is a separate build from the
public `demo/frontend`. Only the server publishes it, from its `admin_ui_dir`
setting; no Pages workflow does.

| Screen | Reads | Writes |
|---|---|---|
| Runtime | `/api/v1/runtime`, `/api/v1/dependencies`, `/api/v1/runtime/results` | none |
| Reference | `/api/v1/bonds`, `/api/v1/reference/masters/:kind` | `POST /api/v1/reference/transactions` |
| Import | none | `POST /api/v1/reference/import` |
| Curves | `/api/v1/curves` | none |
| Books | `/api/v1/books`, its versions and history | none |
| Configuration | history, versions, version diff | validate, stage, activate, rollback |

- **Pending restart:** the runtime and configuration screens compare the
  configuration's active revision with the revision the runtime accepted. A
  restart-required activation commits at once but applies only at the next start,
  and the server never restarts itself.
- **Edits:** an edit opens the current record as a complete reference transaction,
  with its expected version and a fresh idempotency key filled in. The server's
  strict decoder is the only validator, and the page adds no defaults.
- **Access:** the page calls only its own origin. In required access mode, paste a
  bearer token into the header bar. It stays in page memory: never in storage, the
  URL or a cookie. Reloading the tab forgets it.
- **Showcase:** the public showcase does not ship this app. Its demo profile runs
  without authentication.

## Build and serve

```sh
cd admin
npm ci
npm run lint && npm test && npm run build   # writes admin/dist
```

Then configure the server and open `http://<host>:<port>/admin/`:

```toml
admin_ui_dir = "<repo>/admin/dist"         # or CONVEX_ADMIN_UI_DIR

[access]
cors_origins = ["http://127.0.0.1:8080"]   # the origin the page is opened from
```

Browsers send the page's origin with its module script and with every write, even
to its own server, and the server admits listed origins only. Startup refuses
`admin_ui_dir` when the directory has no `index.html` or no origin is listed.

## Develop

`npm run dev` serves on port 3100 and proxies `/api` to `CONVEX_API_URL`, which
defaults to `http://127.0.0.1:8080`. The proxy forwards the page's origin, so list
`http://localhost:3100` in the server's `cors_origins` while developing.
