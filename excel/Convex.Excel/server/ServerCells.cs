using System;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Convex.Excel.Live;

namespace Convex.Excel.Server
{
    /// The scalars a server cell can show (DX3, DX13), and why it shows #N/A.
    internal static class ServerFields
    {
        public static readonly string[] Cell =
        {
            "value", "quality", "status", "revision", "calculated_at", "snapshot_id", "configuration_version",
        };

        public static readonly string[] Status =
        {
            "connection", "user", "server", "runtime", "last_message", "instruments", "topics", "session",
            "problem",
        };

        /// Canonical CX.LIVE field, or #NAME? listing the fields.
        public static string ValidateCell(string field) => Validate(field, Cell, "CX.LIVE");

        /// Canonical CX.STATUS field, or #NAME? listing the fields.
        public static string ValidateStatus(string field) => Validate(field, Status, "CX.STATUS");

        /// A server measure id: lowercase letters, digits and underscores.
        public static string ValidateMeasure(string measure)
        {
            var m = measure.Trim().ToLowerInvariant();
            if (m.Length is < 1 or > 64 || !m.All(c => c is >= 'a' and <= 'z' or >= '0' and <= '9' or '_'))
                throw new ConvexException(
                    $"'{measure}' is not a server measure id (for example clean_price_mid, ytm_mid or dv01_mid)");
            return m;
        }

        private static string Validate(string field, string[] allowed, string function)
        {
            var f = field.Trim().ToLowerInvariant();
            if (allowed.Contains(f)) return f;
            throw new ConvexException($"unknown {function} field {f}: use {string.Join(", ", allowed)}",
                ErrorCodes.UnknownToken);
        }

        /// The value of a server cell for `view`. Throws a ConvexException for
        /// #N/A (with the reason) when there is nothing to show.
        public static object Select(InstrumentView view, string measure, string field)
        {
            var latest = RequireResult(view);
            var envelope = latest.Result;
            switch (field)
            {
                case "status":
                    return envelope.Status;
                case "revision":
                    return (double)latest.Revision;
                case "snapshot_id":
                    return envelope.SnapshotId ?? throw Missing("NO_SNAPSHOT", $"the result of {view.InstrumentId} names no snapshot");
                case "configuration_version":
                    return envelope.ConfigurationVersion ??
                           throw Missing("NO_CONFIGURATION", $"the result of {view.InstrumentId} names no configuration version");
            }

            // value, quality and calculated_at read the measure. A measure the
            // current result lacks comes from the last good result, if any.
            var source = envelope;
            bool lastGood = false;
            if (!envelope.Results.TryGetValue(measure, out var m) &&
                latest.LastGood != null && latest.LastGood.Results.TryGetValue(measure, out m))
            {
                source = latest.LastGood;
                lastGood = true;
            }
            if (m == null) throw NoMeasure(view.InstrumentId, measure, envelope);
            switch (field)
            {
                case "value":
                    return m.Value ?? throw Missing("NOT_A_NUMBER",
                        $"{measure} of {view.InstrumentId} is not a single number" + (m.Unit != null ? $" (unit {m.Unit})" : ""));
                case "quality":
                    if (!view.Fresh) return "Stale";
                    if (lastGood) return latest.RetainedQuality ?? "Stale";
                    return m.Quality.Length > 0 ? m.Quality : envelope.Quality;
                case "calculated_at":
                    return source.CalculatedAtUtc?.ToOADate() ??
                           throw Missing("NO_TIME", $"the result of {view.InstrumentId} has no calculation time");
                default:
                    throw new ConvexException($"unknown CX.LIVE field {field}", ErrorCodes.UnknownToken);
            }
        }

        /// The instrument's newest result, or #N/A saying why there is none.
        public static Valuation RequireResult(InstrumentView view)
        {
            if (view.Sub is Subscription.Refused or Subscription.OverCap || view.ProblemCode == "RESULT_TOO_LARGE")
                throw ConvexException.Unavailable(view.ProblemCode ?? "REFUSED",
                    view.ProblemMessage ?? "the server refused " + view.InstrumentId);
            if (view.Latest != null) return view.Latest;
            var server = view.Server.Length > 0 ? view.Server : "the server";
            throw view.Link switch
            {
                LinkState.NotConfigured => Missing("NOT_CONFIGURED",
                    "no Convex server is set: use Server on the Convex ribbon" +
                    (view.LinkProblem != null ? " (" + view.LinkProblem + ")" : "")),
                LinkState.SignedOut => Missing("SIGNED_OUT",
                    $"not signed in to {server}" + (view.LinkProblem != null ? ": " + view.LinkProblem : "")),
                LinkState.Connecting => Missing("CONNECTING", $"connecting to {server}"),
                LinkState.Reconnecting => Missing("DISCONNECTED",
                    $"not connected to {server}" + (view.LinkProblem != null ? ": " + view.LinkProblem : "")),
                _ => view.Acked
                    ? Missing("NO_RESULT", $"the server's runtime has no result for {view.InstrumentId} yet")
                    : Missing("WAITING", $"waiting for the server's result for {view.InstrumentId}"),
            };
        }

        public static object SelectStatus(SourceStatus status, string field, int topics) => field switch
        {
            "connection" => status.Connection,
            "user" => status.User,
            "server" => status.Server.Length > 0 ? status.Server : throw Missing("NOT_CONFIGURED", "no Convex server is set"),
            "runtime" => status.Runtime ?? throw Missing("NO_RUNTIME", "no result has arrived yet, so the runtime is not known"),
            "last_message" => status.LastMessageUtc?.ToOADate() ?? throw Missing("NO_MESSAGE", "no message from the server yet"),
            "instruments" => (double)status.Instruments,
            "topics" => (double)topics,
            "session" => status.SessionId ?? throw Missing("NO_SESSION", "not connected"),
            "problem" => status.Problem ?? "",
            _ => throw new ConvexException($"unknown CX.STATUS field {field}", ErrorCodes.UnknownToken),
        };

        private static ConvexException NoMeasure(string id, string measure, Envelope envelope)
        {
            var error = envelope.Errors.FirstOrDefault(e => e.Field == measure) ??
                        (envelope.Status == "Failed" ? envelope.Errors.FirstOrDefault() : null);
            if (error != null)
                return ConvexException.Unavailable(error.Code, $"{measure} of {id}: {error.Message}");
            var have = envelope.Results.Keys.OrderBy(k => k, StringComparer.Ordinal).Take(12).ToList();
            return Missing("NO_MEASURE",
                $"the result of {id} has no measure {measure}" +
                (have.Count > 0 ? "; it has " + string.Join(", ", have) + (envelope.Results.Count > have.Count ? ", …" : "") : ""));
        }

        private static ConvexException Missing(string code, string message) => ConvexException.Unavailable(code, message);
    }

    /// Server topics of the hub. A cell's topic strings are its request:
    /// ["S1", identifier, measure, field], or ["S1S", field] for CX.STATUS,
    /// so equal cells share a topic and a saved workbook reconnects with no
    /// earlier state. A server cell has no value until the stream brings one,
    /// so a new cell shows #N/A until its result arrives (under Manual
    /// calculation, until the next F9).
    internal sealed class ServerCells
    {
        public const string Kind = "S1";
        public const string StatusKind = "S1S";

        private sealed class Cell
        {
            public readonly int Id;
            public readonly string Key;
            public readonly bool IsStatus;
            public readonly string Identifier;
            public readonly string Measure;
            public readonly string Field;
            public readonly ITopicSink Sink;
            public readonly object Gate = new();
            public string? InstrumentId;
            public Exception? ResolveError;
            public object? Value;
            public Exception? Error;
            public long Seq;

            public Cell(int id, string key, bool isStatus, string identifier, string measure, string field, ITopicSink sink)
            {
                Id = id;
                Key = key;
                IsStatus = isStatus;
                Identifier = identifier;
                Measure = measure;
                Field = field;
                Sink = sink;
            }
        }

        private readonly ServerSource _source;
        private readonly IdentifierResolver _resolver;
        private readonly object _lock = new();
        private readonly Dictionary<int, Cell> _byId = new();
        private readonly Dictionary<string, Cell> _byKey = new(StringComparer.Ordinal);
        private readonly Dictionary<string, HashSet<Cell>> _byInstrument = new(StringComparer.Ordinal);
        private readonly Dictionary<string, HashSet<Cell>> _resolving = new(StringComparer.Ordinal);
        private readonly HashSet<Cell> _status = new();
        private readonly Timer _retry; // held so the timer is not collected

        /// `retryEvery` is how often a failed lookup is checked for a retry
        /// (default a minute); see RetryFailedLookups.
        public ServerCells(ServerSource source, IdentifierResolver resolver, TimeSpan? retryEvery = null)
        {
            _source = source;
            _resolver = resolver;
            _source.InstrumentsChanged += OnInstrumentsChanged;
            _source.StatusChanged += OnStatusChanged;
            var every = retryEvery ?? TimeSpan.FromMinutes(1);
            _retry = new Timer(_ => RetryFailedLookups(), null, every, every);
        }

        public static bool Owns(IList<string> topicInfo) =>
            topicInfo.Count > 0 && (topicInfo[0] == Kind || topicInfo[0] == StatusKind);

        /// Topic strings of a CX.LIVE cell.
        public static string[] TopicStrings(string identifier, string measure, string field) =>
            new[] { Kind, identifier.Trim(), ServerFields.ValidateMeasure(measure), ServerFields.ValidateCell(field) };

        /// Topic strings of a CX.STATUS cell.
        public static string[] StatusTopicStrings(string field) => new[] { StatusKind, ServerFields.ValidateStatus(field) };

        public int Count
        {
            get { lock (_lock) return _byId.Count; }
        }

        /// Excel connected a server topic: index it, start following its
        /// instrument and return what is known now.
        public object Connect(int topicId, IList<string> topicInfo, ITopicSink sink)
        {
            string key = LocalSpec.KeyOf(topicInfo);
            Cell cell;
            if (topicInfo.Count == 2 && topicInfo[0] == StatusKind)
            {
                cell = new Cell(topicId, key, true, "", "", ServerFields.ValidateStatus(topicInfo[1]), sink);
            }
            else if (topicInfo.Count == 4 && topicInfo[0] == Kind)
            {
                cell = new Cell(topicId, key, false, topicInfo[1], ServerFields.ValidateMeasure(topicInfo[2]),
                    ServerFields.ValidateCell(topicInfo[3]), sink);
            }
            else
            {
                throw new ConvexException("not a Convex server topic (rebuild the cell)");
            }

            // Classify before indexing: a bad identifier is refused with no state left behind.
            var (kind, value) = cell.IsStatus ? (IdentifierKind.Instrument, "") : Identifiers.Classify(cell.Identifier);
            bool resolve = false;
            lock (_lock)
            {
                _byId[topicId] = cell;
                _byKey[key] = cell;
                if (cell.IsStatus)
                {
                    _status.Add(cell);
                }
                else
                {
                    if (kind == IdentifierKind.Instrument)
                        Bind(cell, value);
                    else if (_resolver.TryGetCached(kind, value, out var id, out var error))
                    {
                        if (id != null) Bind(cell, id);
                        else cell.ResolveError = error;
                    }
                    else
                    {
                        if (!_resolving.TryGetValue(value, out var waiting))
                        {
                            _resolving[value] = waiting = new HashSet<Cell>();
                            resolve = true;
                        }
                        waiting.Add(cell);
                    }
                }
            }
            if (resolve)
            {
                var k = kind;
                var v = value;
                _resolver.ResolveAsync(k, v).ContinueWith(t => Resolved(v, t), TaskScheduler.Default);
            }
            var first = Evaluate(cell, push: false);
            // CX.STATUS("topics") counts this cell; refresh it off this thread.
            ThreadPool.QueueUserWorkItem(_ => RefreshStatusCells());
            // A concurrent evaluation may own the topic already; it pushes its value.
            lock (cell.Gate) return cell.Value ?? first;
        }

        public void Disconnect(int topicId)
        {
            lock (_lock)
            {
                if (!_byId.TryGetValue(topicId, out var cell)) return;
                _byId.Remove(topicId);
                if (_byKey.TryGetValue(cell.Key, out var current) && current == cell) _byKey.Remove(cell.Key);
                _status.Remove(cell);
                foreach (var waiting in _resolving.Values) waiting.Remove(cell);
                if (cell.InstrumentId is { } id)
                {
                    if (_byInstrument.TryGetValue(id, out var cells) && cells.Remove(cell) && cells.Count == 0)
                        _byInstrument.Remove(id);
                    _source.Release(id);
                }
            }
            ThreadPool.QueueUserWorkItem(_ => RefreshStatusCells());
        }

        /// Detail behind a server topic's #N/A, for CX.LASTERROR.
        public bool TryGetError(string key, out Exception error)
        {
            lock (_lock)
            {
                if (_byKey.TryGetValue(key, out var cell))
                {
                    lock (cell.Gate)
                    {
                        error = cell.Error!;
                        return error != null;
                    }
                }
            }
            error = null!;
            return false;
        }

        /// Under _lock. Acquire never calls back, so holding the lock is safe.
        private void Bind(Cell cell, string instrumentId)
        {
            cell.InstrumentId = instrumentId;
            if (!_byInstrument.TryGetValue(instrumentId, out var cells))
                _byInstrument[instrumentId] = cells = new HashSet<Cell>();
            cells.Add(cell);
            _source.Acquire(instrumentId);
        }

        private void Resolved(string value, Task<string> lookup)
        {
            Cell[] cells;
            lock (_lock)
            {
                if (!_resolving.TryGetValue(value, out var waiting)) return;
                _resolving.Remove(value);
                cells = waiting.Where(c => _byId.ContainsKey(c.Id)).ToArray();
                foreach (var cell in cells)
                {
                    if (lookup.Status == TaskStatus.RanToCompletion) Bind(cell, lookup.Result);
                    else cell.ResolveError = lookup.Exception?.InnerException ?? new ConvexException("lookup failed");
                }
            }
            foreach (var cell in cells) Evaluate(cell, push: true);
        }

        /// Look up again each CUSIP or ISIN cell whose lookup failed, once the
        /// resolver no longer holds the failure: every one after a new server or
        /// sign-in (which clears the resolver), and otherwise when its 5 minutes
        /// pass, so reference data added on the server later is found. A held
        /// failure costs no call. Runs from a timer, and from ServerLink.
        public void RetryFailedLookups()
        {
            var start = new List<(IdentifierKind Kind, string Value)>();
            var changed = new List<Cell>();
            lock (_lock)
            {
                foreach (var cell in _byId.Values)
                {
                    if (cell.IsStatus || cell.ResolveError == null) continue;
                    var (kind, value) = Identifiers.Classify(cell.Identifier);
                    if (_resolver.TryGetCached(kind, value, out var id, out var error))
                    {
                        if (id != null)
                        {
                            cell.ResolveError = null;
                            Bind(cell, id);
                            changed.Add(cell);
                        }
                        else if (error != cell.ResolveError)
                        {
                            cell.ResolveError = error;
                            changed.Add(cell);
                        }
                        continue;
                    }
                    cell.ResolveError = null;
                    changed.Add(cell);
                    if (!_resolving.TryGetValue(value, out var waiting))
                    {
                        _resolving[value] = waiting = new HashSet<Cell>();
                        start.Add((kind, value));
                    }
                    waiting.Add(cell);
                }
            }
            foreach (var (kind, value) in start)
                _resolver.ResolveAsync(kind, value).ContinueWith(t => Resolved(value, t), TaskScheduler.Default);
            foreach (var cell in changed) Evaluate(cell, push: true);
        }

        private void OnInstrumentsChanged(IReadOnlyCollection<string> ids)
        {
            var cells = new List<Cell>();
            lock (_lock)
                foreach (var id in ids)
                    if (_byInstrument.TryGetValue(id, out var set)) cells.AddRange(set);
            foreach (var cell in cells) Evaluate(cell, push: true);
        }

        private void OnStatusChanged(bool link)
        {
            Cell[] cells;
            lock (_lock) cells = link ? _byId.Values.ToArray() : _status.ToArray();
            foreach (var cell in cells) Evaluate(cell, push: true);
        }

        private void RefreshStatusCells() => OnStatusChanged(false);

        /// Compute the cell; store and (when `push`) send the value if it
        /// changed. Returns the computed value.
        private object Evaluate(Cell cell, bool push)
        {
            long seq = Interlocked.Increment(ref cell.Seq);
            object value;
            Exception? error = null;
            try
            {
                value = Compute(cell);
            }
            catch (Exception ex)
            {
                value = ErrorMapper.ToExcelError(ex);
                error = ex;
            }
            lock (cell.Gate)
            {
                if (seq != cell.Seq) return value; // a newer evaluation owns the topic
                bool unchanged = cell.Value != null && LiveHub.Same(cell.Value, value) &&
                                 cell.Error?.Message == error?.Message;
                cell.Error = error;
                if (unchanged) return value;
                cell.Value = value;
            }
            if (!push) return value;
            try
            {
                cell.Sink.Push(value);
            }
            catch (Exception ex)
            {
                CxErrorStore.Record(ex, "(server)", "server cell push");
            }
            return value;
        }

        private object Compute(Cell cell)
        {
            if (cell.IsStatus) return ServerFields.SelectStatus(_source.Status(), cell.Field, Count);
            if (cell.ResolveError != null) throw cell.ResolveError;
            var id = cell.InstrumentId ?? throw ConvexException.Unavailable("RESOLVING",
                $"looking up {cell.Identifier} on the server");
            return ServerFields.Select(_source.View(id), cell.Measure, cell.Field);
        }
    }
}
