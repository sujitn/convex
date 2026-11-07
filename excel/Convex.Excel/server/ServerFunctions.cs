using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Convex.Excel.Live;
using ExcelDna.Integration;

namespace Convex.Excel.Server
{
    /// Worksheet functions over the server link (DX13). CX.LIVE and CX.STATUS
    /// read RTD topics, so they must not be IsThreadSafe; CX.LIVE.GRID reads
    /// the cache; CX.SNAP reads once.
    public static class ServerFunctions
    {
        private const string Category = "Convex Server";

        [ExcelFunction(Name = "CX.LIVE",
            Description = "A live server result: one number (or a detail field) that updates when the server " +
                          "publishes a new result for the instrument (RTD).",
            Category = Category)]
        public static object CxLive(
            [ExcelArgument("CUSIP, ISIN or the server's instrument id")] object identifier,
            [ExcelArgument("Server measure id, e.g. clean_price_mid, ytm_mid, dv01_mid")] object measure,
            [ExcelArgument("value (default) | quality | status | revision | calculated_at | snapshot_id | configuration_version")]
            object field) =>
            Functions.Safe(() => LiveCells.ServerCell(Functions.AsString(identifier, ""),
                Functions.AsString(measure, ""), Functions.AsString(field, "value")));

        [ExcelFunction(Name = "CX.LIVE.GRID",
            Description = "Measures of an instrument the add-in already follows (through a CX.LIVE cell), read " +
                          "from the cache: measure, value, quality. Not live by itself: pass a CX.LIVE cell as " +
                          "trigger to recalculate it with that cell.",
            Category = Category)]
        public static object CxLiveGrid(
            [ExcelArgument("CUSIP, ISIN or the server's instrument id")] object identifier,
            [ExcelArgument("Measure ids (a range or one id); all measures when omitted")] object measures,
            [ExcelArgument("Optional live cell whose changes recalculate this grid")] object trigger) =>
            Functions.Safe(() => Grid(Functions.AsString(identifier, ""), measures));

        [ExcelFunction(Name = "CX.SNAP",
            Description = "The server's current value of a measure, read once (it does not update). Shows #N/A " +
                          "until the server answers.",
            Category = Category)]
        public static object CxSnap(
            [ExcelArgument("CUSIP, ISIN or the server's instrument id")] object identifier,
            [ExcelArgument("Server measure id, e.g. clean_price_mid")] object measure)
        {
            string id = Functions.AsString(identifier, "");
            string m;
            try
            {
                Identifiers.Classify(id);
                m = ServerFields.ValidateMeasure(Functions.AsString(measure, ""));
            }
            catch (Exception ex)
            {
                CxErrorStore.Record(ex);
                return ErrorMapper.ToExcelError(ex);
            }
            var result = ExcelAsyncUtil.RunTask("CX.SNAP", new object[] { id, m }, () => Snap.ReadAsync(id, m));
            if (result is ExcelError && Snap.TryGetError(id, m, out var error)) CxErrorStore.Record(error);
            return result;
        }

        [ExcelFunction(Name = "CX.STATUS",
            Description = "The server link: connection (default), user, server, runtime, last_message (UTC), " +
                          "instruments, topics, session, problem, or calculation (Excel's calculation mode).",
            Category = Category, IsVolatile = true)]
        public static object CxStatus(
            [ExcelArgument("connection (default) | user | server | runtime | last_message | instruments | topics | session | problem | calculation")]
            object field) =>
            Functions.Safe(() =>
            {
                var f = Functions.AsString(field, "connection").Trim().ToLowerInvariant();
                return f == "calculation" ? CalculationMode() : LiveCells.StatusCell(f);
            });

        /// Live cells update by themselves only under Automatic calculation.
        private static object CalculationMode()
        {
            int mode = (int)((dynamic)ExcelDnaUtil.Application).Calculation;
            return mode switch
            {
                -4105 => "Automatic",
                -4135 => "Manual",
                2 => "Automatic except tables",
                _ => "Unknown",
            };
        }

        private static object Grid(string identifier, object measures)
        {
            var (kind, value) = Identifiers.Classify(identifier);
            string instrumentId;
            if (kind == IdentifierKind.Instrument)
            {
                instrumentId = value;
            }
            else if (ServerLink.Resolver.TryGetCached(kind, value, out var id, out var error))
            {
                instrumentId = id ?? throw error!;
            }
            else
            {
                _ = ServerLink.Resolver.ResolveAsync(kind, value);
                throw ConvexException.Unavailable("RESOLVING",
                    $"looking up {value} on the server; recalculate once it is found");
            }

            var view = ServerLink.Source.View(instrumentId);
            if (view.Sub == Subscription.None)
                throw ConvexException.Unavailable("NOT_FOLLOWED",
                    $"CX.LIVE.GRID reads results the add-in follows: add a CX.LIVE cell for {identifier}, or use CX.SNAP");
            var latest = ServerFields.RequireResult(view);

            var names = new List<string>();
            if (Functions.IsBlank(measures))
            {
                names.AddRange(latest.Result.Results.Keys
                    .Concat(latest.LastGood?.Results.Keys ?? Enumerable.Empty<string>())
                    .Distinct(StringComparer.Ordinal).OrderBy(k => k, StringComparer.Ordinal));
            }
            else if (measures is object[,] range)
            {
                foreach (var cell in range)
                    if (!Functions.IsBlank(cell)) names.Add(ServerFields.ValidateMeasure(Functions.AsString(cell, "")));
            }
            else
            {
                names.Add(ServerFields.ValidateMeasure(Functions.AsString(measures, "")));
            }
            if (names.Count == 0) throw ConvexException.Unavailable("NO_MEASURE", $"the result of {identifier} has no measures");

            var grid = new object[names.Count, 3];
            for (int i = 0; i < names.Count; i++)
            {
                grid[i, 0] = names[i];
                grid[i, 1] = Field(view, names[i], "value");
                grid[i, 2] = Field(view, names[i], "quality");
            }
            return grid;
        }

        private static object Field(InstrumentView view, string measure, string field)
        {
            try
            {
                return ServerFields.Select(view, measure, field);
            }
            catch (Exception ex)
            {
                return ErrorMapper.ToExcelError(ex);
            }
        }

        /// CX.SNAP's reads: follow the instrument until its current result
        /// arrives (or the server says there is none), then let it go.
        private static class Snap
        {
            public static readonly TimeSpan Timeout = TimeSpan.FromSeconds(15);
            private const int MaxErrors = 1_000;
            private static readonly ConcurrentDictionary<string, Exception> Errors = new(StringComparer.Ordinal);

            public static bool TryGetError(string id, string measure, out Exception error) =>
                Errors.TryGetValue(Key(id, measure), out error!);

            public static async Task<object> ReadAsync(string identifier, string measure)
            {
                var key = Key(identifier, measure);
                try
                {
                    var (kind, value) = Identifiers.Classify(identifier);
                    var instrumentId = kind == IdentifierKind.Instrument
                        ? value
                        : await ServerLink.Resolver.ResolveAsync(kind, value).ConfigureAwait(false);
                    var view = await ServerLink.Source.SnapshotAsync(instrumentId, Timeout, CancellationToken.None)
                        .ConfigureAwait(false);
                    // A snapshot is the current result: an older one kept from
                    // an earlier session or runtime does not count.
                    if (!view.Fresh) view.Latest = null;
                    var result = ServerFields.Select(view, measure, "value");
                    Errors.TryRemove(key, out _);
                    return result;
                }
                catch (Exception ex)
                {
                    if (Errors.Count >= MaxErrors) Errors.Clear();
                    Errors[key] = ex;
                    return ErrorMapper.ToExcelError(ex);
                }
            }

            private static string Key(string id, string measure) => id.Trim() + "\u001f" + measure;
        }
    }
}
