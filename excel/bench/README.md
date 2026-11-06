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
