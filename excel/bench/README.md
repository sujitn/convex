# Excel add-in measurements (X0)

Local measurement tools for the Excel real-time work. They need a desktop Excel
(Microsoft 365, 64-bit), so CI does not run them. Results are recorded in the
plan, not in this repository.

## What is here

| Path | Purpose |
|---|---|
| `../Convex.Excel.Bench/` | Bench add-in (`CXB.*` functions). It provides a synthetic RTD server, a recalculation recorder and a main-thread probe, and loads beside the real add-in. |
| `Run-ExcelBench.ps1` | Driver. It starts a private, hidden Excel over COM, builds the sheets, runs each scenario and writes JSON results. |
| `WsDrain/` | Console test. It measures how many valuation frames per second a `net472` `ClientWebSocket` can receive, parse and apply. |
| `Run-ServerCells.ps1` | Live server cells (X3) in real Excel against a running convex-server: `CX.LIVE`, `CX.SNAP` and `CX.STATUS` while a feed moves the price, and, given `-StopServer` and `-StartServer`, values kept as `Stale` while the server is down and followed again after it restarts. It writes `%APPDATA%\Convex\server.json` for the run and restores it. |
| `Run-LimitCells.ps1` | `CX.LIMIT` (X4) in real Excel against a running convex-server: a limit's status, value, record id and quality while a feed moves the book across its bounds, the `#N/A` reasons, and, given `-StopServer` and `-StartServer`, the status kept as `Stale` while the server is down and current again after it restarts. It writes `%APPDATA%\Convex\server.json` for the run and restores it. |

## Build

```powershell
cargo build --profile excel -p convex-ffi
dotnet build excel/Convex.Excel/Convex.Excel.csproj -c Release
dotnet build excel/Convex.Excel.Bench/Convex.Excel.Bench.csproj -c Release
dotnet build excel/bench/WsDrain/WsDrain.csproj -c Release
```

Add `-p:NativeLibPath=<path to convex_ffi.dll>` when cargo builds into a
different target directory. If the bench build fails with `Reference with Path:
convex_ffi.dll ... NOT FOUND`, the native DLL has not been built (or
`NativeLibPath` points elsewhere): the bench `.dna` packs it for the native
check.

## Run

```powershell
excel/bench/Run-ExcelBench.ps1 -OutDir <folder> -Runs 2
excel/bench/WsDrain/bin/Release/net472/WsDrain.exe 50000 2000
```

The driver changes `Application.RTD.ThrottleInterval`. That setting applies to
all of Excel and persists across restarts, so the driver restores the original
value in a `finally` block and prints it. If a run is killed, check the value
afterwards with `(New-Object -ComObject Excel.Application).RTD.ThrottleInterval`.

Each Excel instance the driver starts is stopped when its scenario ends, even
if `Quit` leaves the process running (PowerShell holds COM references).

### Limits against a local server

`Run-LimitCells.ps1` needs a server that monitors limits and accepts a limit
set, which the Docker showcase's edge refuses. Run the showcase natively
instead (Git Bash; `$BIN` is the folder holding `convex-server`,
`convex-config` and `convex-core-inputs`, `$DIR` a scratch folder):

```sh
$BIN/convex-core-inputs $DIR/convex.redb deploy/showcase/seed.json
sh deploy/showcase/live.sh activate $DIR/convex.redb $BIN deploy/showcase $DIR/boot INSTRUMENT-001
# deploy/showcase/server.toml with storage_path = "$DIR/convex.redb"
$BIN/convex-server $DIR/server.toml &
sh deploy/showcase/live.sh feed http://127.0.0.1:8081/api/v1/market/quotes INSTRUMENT-001 &
```

Then give `SHOWCASE-BOOK` a limit set the feed crosses. Under local
development a set commits at once: the first without `If-Match`, each
replacement with `If-Match` naming the version it replaces.

```powershell
$url = 'http://127.0.0.1:8081/api/v1/books/SHOWCASE-BOOK/limits'
$set = { param($upper) '{"limits":[{"limit_id":"mv","measure":{"kind":"market_value"},"upper":"' + $upper +
    '","warning_fraction":"0.9998"},{"limit_id":"face","measure":{"kind":"face","instrument":{"one":"INSTRUMENT-001"}},"upper":"2000000","warning_fraction":"0.9"}]}' }
Invoke-WebRequest -Method Put $url -Headers @{ 'Idempotency-Key' = 'set-1' } -ContentType application/json -Body (& $set 5000000)
$mv = [decimal](((Invoke-WebRequest "$url/status").Content | ConvertFrom-Json).limits | Where-Object limit_id -eq mv).value
Invoke-WebRequest -Method Put $url -Headers @{ 'Idempotency-Key' = 'set-2'; 'If-Match' = '1' } -SkipHeaderValidation `
    -ContentType application/json -Body (& $set ([math]::Round($mv + 100)))
excel/bench/Run-LimitCells.ps1 -Book SHOWCASE-BOOK -Limit mv
```

The feed moves the price by up to two cents a second, about 200 of the
book's market value, so the `mv` limit changes status every few seconds.

## Scenarios and fields

| Scenario | Sheet |
|---|---|
| `native` | The packed bench `.xll` alone, in a folder with no `convex_ffi.dll`. Records `CXB.NATIVE()`. |
| `rtd` | `A1:An` hold `=CXB.TOPIC(ROW())` and `B1:Bn` hold `=CXB.SEEN(A1,ROW(),TRUE)`. Every topic is pushed every 250 ms for `-Seconds`. Each throttle setting is run. |
| `cxlive` | `A1:An` hold `=CX.SPREAD.LIVE(...)` (Z-spread) with a distinct mark per row, so `CxCache` cannot merge them. The curve is referenced by name. `B` holds `CXB.SEEN`. The driver edits one curve input. |
| `dependents` | As `cxlive`, but rows `1..n/2` read `USD.SOFR` and the rest `USD.ALT`. The driver edits `USD.SOFR`, then records how many cells changed and how many topics the hub computed (`CX.DIAG`); both should be `n/2`. |

| Field | Meaning |
|---|---|
| `latency_ms` | From the push to the `CXB.SEEN` evaluation (`rtd` only). It includes the wait for the throttle. |
| `batch_span_ms` | The recalculation time of one refresh: the first to the last recorded evaluation in a burst. A burst ends at a gap over `batch_gap_ms`. It covers the `CXB.TOPIC` and `CXB.SEEN` cells, so the sheet has twice `n` cells. |
| `main_thread_delay_ms` | How long a macro queued every 50 ms waited to run. This approximates how long Excel's UI thread was busy. |
| `marks.curve_edit` | For `cxlive` and `dependents`: the time from the edit to the first and the last live cell that changed. |
