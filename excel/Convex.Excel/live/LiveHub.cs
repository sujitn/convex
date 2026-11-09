using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Convex.Excel.Server;
using ExcelDna.Integration;
using Newtonsoft.Json.Linq;

namespace Convex.Excel.Live
{
    /// Where a topic's new value goes (an Excel-DNA RTD topic in the add-in).
    internal interface ITopicSink
    {
        void Push(object value);
    }

    /// One read of the FFI change log (convex_changes_since).
    internal sealed class ChangeSet
    {
        public ulong Cursor { get; }
        public bool Complete { get; }
        public IReadOnlyList<string> Names { get; }
        public IReadOnlyList<ulong> Handles { get; }

        public ChangeSet(ulong cursor, bool complete, IReadOnlyList<string> names, IReadOnlyList<ulong> handles)
        {
            Cursor = cursor;
            Complete = complete;
            Names = names;
            Handles = handles;
        }

        public static ChangeSet Parse(JToken result) => new(
            (ulong)result["cursor"]!,
            (bool)result["complete"]!,
            (result["names"] as JArray ?? new JArray()).Select(n => (string)n!).ToArray(),
            (result["handles"] as JArray ?? new JArray()).Select(h => (ulong)h).ToArray());
    }

    internal readonly struct LiveStats
    {
        public int Topics { get; }
        public long Computes { get; }
        public long Pushes { get; }
        public long Drains { get; }
        public double LastDrainMs { get; }

        public LiveStats(int topics, long computes, long pushes, long drains, double lastDrainMs)
        {
            Topics = topics;
            Computes = computes;
            Pushes = pushes;
            Drains = drains;
            LastDrainMs = lastDrainMs;
        }
    }

    /// The topic hub behind every live cell. It owns the topics, keeps each
    /// one's last value, and pushes a value to Excel only when it changed (the
    /// value cutoff). Local topics are recomputed only when the FFI change log
    /// names a bond or curve reference they read. Server topics (ServerCells)
    /// change when the server stream brings their instrument's result. Excel's
    /// own calc chain then recalculates the formulas downstream of the cells
    /// that changed. Local and server topics share the topic ceiling.
    ///
    /// Pure C#: the Excel-DNA RTD server (ConvexRtdServer) is a thin adapter,
    /// so this class is tested headlessly.
    internal sealed class LiveHub
    {
        public const string ProgId = "Convex.Rtd";
        public const int MaxTopics = 20_000;
        /// Safety poll of the change log while topics exist. Registry mutations
        /// in the add-in also request a drain directly (Cx), so this only
        /// bounds the delay for a mutation no one announced.
        public const int PollMs = 2_000;

        public static LiveHub Instance { get; } = new(
            LocalCompute.Compute,
            () => Cx.Generation(),
            Cx.ChangesSince,
            work => ThreadPool.QueueUserWorkItem(_ => work()),
            Math.Max(1, Math.Min(4, Environment.ProcessorCount / 2)),
            startPoll: true,
            server: ServerLink.Cells);

        private sealed class Topic
        {
            public readonly int Id;
            public readonly string Key;
            public readonly LocalSpec Spec;
            public readonly ITopicSink Sink;
            public readonly object Gate = new();
            public object? Value;
            public Exception? Error;
            public long Seq;

            public Topic(int id, string key, LocalSpec spec, ITopicSink sink)
            {
                Id = id;
                Key = key;
                Spec = spec;
                Sink = sink;
            }
        }

        private readonly Func<LocalSpec, object> _compute;
        private readonly Func<ulong> _generation;
        private readonly Func<ulong, ChangeSet> _changesSince;
        private readonly Action<Action> _schedule;
        private readonly int _parallelism;
        private readonly bool _startPoll;
        private readonly ServerCells? _server;

        private readonly object _lock = new();
        private readonly Dictionary<int, Topic> _byId = new();
        private readonly Dictionary<string, Topic> _byKey = new(StringComparer.Ordinal);
        private readonly Dictionary<string, HashSet<Topic>> _byName = new(StringComparer.Ordinal);
        private readonly Dictionary<ulong, HashSet<Topic>> _byHandle = new();
        private readonly HashSet<Topic> _retry = new();
        // Topic ids Excel connected to a server topic.
        private readonly HashSet<int> _serverIds = new();
        // Errors for topic strings that never became a topic (bad spec, limit).
        private readonly Dictionary<string, Exception> _refused = new(StringComparer.Ordinal);
        private ulong _cursor;
        private bool _haveCursor;
        private Timer? _poll;

        // 0 idle, 1 draining, 2 draining and another drain requested.
        private int _drainState;

        private long _computes;
        private long _pushes;
        private long _drains;
        private long _lastDrainTicks;

        internal LiveHub(
            Func<LocalSpec, object> compute,
            Func<ulong> generation,
            Func<ulong, ChangeSet> changesSince,
            Action<Action> schedule,
            int parallelism,
            bool startPoll,
            ServerCells? server = null)
        {
            _compute = compute;
            _generation = generation;
            _changesSince = changesSince;
            _schedule = schedule;
            _parallelism = parallelism;
            _startPoll = startPoll;
            _server = server;
        }

        public int TopicCount
        {
            get { lock (_lock) return _byId.Count + _serverIds.Count; }
        }

        public LiveStats Stats()
        {
            lock (_lock)
                return new LiveStats(_byId.Count + _serverIds.Count, Interlocked.Read(ref _computes),
                    Interlocked.Read(ref _pushes), Interlocked.Read(ref _drains),
                    Interlocked.Read(ref _lastDrainTicks) * 1000.0 / Stopwatch.Frequency);
        }

        /// Excel connected a topic. Indexes it before computing, so a change
        /// that lands during the compute is caught by the next drain, then
        /// computes on the calling thread so the first value is never a
        /// placeholder (Manual calculation would otherwise keep it until F9).
        public object Connect(int topicId, IList<string> topicInfo, ITopicSink sink)
        {
            string key = LocalSpec.KeyOf(topicInfo);
            if (_server != null && ServerCells.Owns(topicInfo)) return ConnectServer(topicId, topicInfo, sink, key);
            LocalSpec spec;
            try
            {
                spec = LocalSpec.FromTopicStrings(topicInfo);
            }
            catch (Exception ex)
            {
                return Refuse(key, ex);
            }

            var topic = new Topic(topicId, key, spec, sink);
            lock (_lock)
            {
                if (_byId.Count + _serverIds.Count >= MaxTopics) return Refuse(key, TopicLimit());
                if (!_haveCursor)
                {
                    _cursor = _generation();
                    _haveCursor = true;
                }
                _byId[topicId] = topic;
                _byKey[key] = topic;
                _refused.Remove(key);
                foreach (var name in spec.Names) Index(_byName, name, topic);
                foreach (var handle in spec.Handles) Index(_byHandle, handle, topic);
                if (_startPoll && _poll == null)
                    _poll = new Timer(_ => RequestDrain(), null, PollMs, PollMs);
            }

            Evaluate(topic, push: false);
            lock (topic.Gate) return topic.Value ?? ExcelError.ExcelErrorNA;
        }

        private object ConnectServer(int topicId, IList<string> topicInfo, ITopicSink sink, string key)
        {
            lock (_lock)
            {
                if (_byId.Count + _serverIds.Count >= MaxTopics) return Refuse(key, TopicLimit());
                _serverIds.Add(topicId);
            }
            try
            {
                var value = _server!.Connect(topicId, topicInfo, sink);
                lock (_lock) _refused.Remove(key);
                return value;
            }
            catch (Exception ex)
            {
                lock (_lock) _serverIds.Remove(topicId);
                return Refuse(key, ex);
            }
        }

        private static ConvexException TopicLimit() => new(
            "live topic limit reached (" + MaxTopics.ToString("N0", CultureInfo.InvariantCulture) +
            " distinct live cells); use the static CX functions for the rest");

        public void Disconnect(int topicId)
        {
            Timer? stop = null;
            bool server;
            lock (_lock) server = _serverIds.Remove(topicId);
            if (server)
            {
                _server!.Disconnect(topicId);
                return;
            }
            lock (_lock)
            {
                if (!_byId.TryGetValue(topicId, out var topic)) return;
                _byId.Remove(topicId);
                if (_byKey.TryGetValue(topic.Key, out var current) && current == topic)
                    _byKey.Remove(topic.Key);
                foreach (var name in topic.Spec.Names) Unindex(_byName, name, topic);
                foreach (var handle in topic.Spec.Handles) Unindex(_byHandle, handle, topic);
                _retry.Remove(topic);
                if (_byId.Count == 0)
                {
                    stop = _poll;
                    _poll = null;
                    _haveCursor = false;
                }
            }
            stop?.Dispose();
        }

        /// Detail behind a topic's error value, for CX.LASTERROR.
        public bool TryGetError(string key, out Exception error)
        {
            lock (_lock)
            {
                if (_byKey.TryGetValue(key, out var topic))
                {
                    lock (topic.Gate)
                    {
                        error = topic.Error!;
                        return error != null;
                    }
                }
                if (_refused.TryGetValue(key, out error!)) return true;
            }
            return _server != null && _server.TryGetError(key, out error);
        }

        /// Ask for a drain of the change log. Single-flight: a request during a
        /// drain runs exactly one more drain after it.
        public void RequestDrain()
        {
            if (TopicCount == 0) return;
            while (true)
            {
                int state = Volatile.Read(ref _drainState);
                if (state == 2) return;
                if (Interlocked.CompareExchange(ref _drainState, state + 1, state) == state)
                {
                    if (state == 0) _schedule(RunDrains);
                    return;
                }
            }
        }

        private void RunDrains()
        {
            while (true)
            {
                try
                {
                    Drain();
                }
                catch (Exception ex)
                {
                    // A drain must never kill the process from a pool thread.
                    CxErrorStore.Record(ex, "(live)", "live drain");
                }
                if (Interlocked.CompareExchange(ref _drainState, 1, 2) == 2) continue;
                if (Interlocked.CompareExchange(ref _drainState, 0, 1) == 1) return;
            }
        }

        /// One drain: read the change log, recompute the topics that read a
        /// changed reference (every topic when the log is incomplete), and push
        /// the values that changed. Synchronous, for tests.
        internal void Drain()
        {
            long start = Stopwatch.GetTimestamp();
            Topic[] affected;
            lock (_lock)
            {
                if (_byId.Count == 0) return;
                var changes = _changesSince(_cursor);
                _cursor = changes.Cursor;
                var set = new HashSet<Topic>(_retry);
                _retry.Clear();
                if (!changes.Complete)
                {
                    set.UnionWith(_byId.Values);
                }
                else
                {
                    foreach (var name in changes.Names)
                        if (_byName.TryGetValue(name, out var readers)) set.UnionWith(readers);
                    foreach (var handle in changes.Handles)
                        if (_byHandle.TryGetValue(handle, out var readers)) set.UnionWith(readers);
                }
                affected = set.ToArray();
            }
            Interlocked.Increment(ref _drains);
            if (affected.Length == 0) return;

            if (_parallelism <= 1 || affected.Length == 1)
                foreach (var topic in affected) Evaluate(topic, push: true);
            else
                Parallel.ForEach(affected,
                    new ParallelOptions { MaxDegreeOfParallelism = _parallelism },
                    topic => Evaluate(topic, push: true));
            Interlocked.Exchange(ref _lastDrainTicks, Stopwatch.GetTimestamp() - start);
        }

        private void Evaluate(Topic topic, bool push)
        {
            long seq = Interlocked.Increment(ref topic.Seq);
            object value;
            Exception? error = null;
            try
            {
                Interlocked.Increment(ref _computes);
                value = _compute(topic.Spec);
            }
            catch (ConvexException ex) when (ex.Code == ErrorCodes.ResourceLimit)
            {
                // The FFI's concurrent-call cap: keep the last value and retry on
                // the next drain (the safety poll guarantees one).
                lock (_lock)
                    if (_byId.ContainsKey(topic.Id)) _retry.Add(topic);
                return;
            }
            catch (Exception ex)
            {
                value = ErrorMapper.ToExcelError(ex);
                error = ex;
            }

            lock (topic.Gate)
            {
                if (seq != topic.Seq) return; // a newer compute owns the topic
                // Value cutoff. A changed error message also counts, so the
                // cell re-runs and CX.LASTERROR shows the current cause.
                bool unchanged = topic.Value != null && Same(topic.Value, value) &&
                                 topic.Error?.Message == error?.Message;
                topic.Error = error;
                if (unchanged) return;
                topic.Value = value;
            }
            if (!push) return;
            try
            {
                topic.Sink.Push(value);
                Interlocked.Increment(ref _pushes);
            }
            catch (Exception ex)
            {
                // A push can race Excel disconnecting the topic.
                CxErrorStore.Record(ex, "(live)", "live push");
            }
        }

        private object Refuse(string key, Exception ex)
        {
            lock (_lock)
            {
                if (_refused.Count >= MaxRefused) _refused.Clear(); // detail only; bounded
                _refused[key] = ex;
            }
            return ErrorMapper.ToExcelError(ex);
        }

        private const int MaxRefused = 1_000;

        internal static bool Same(object a, object b) => (a, b) switch
        {
            (double x, double y) => x.Equals(y), // NaN equals NaN; -0.0 equals 0.0
            _ => a.Equals(b),
        };

        private static void Index<TKey>(Dictionary<TKey, HashSet<Topic>> index, TKey key, Topic topic)
            where TKey : notnull
        {
            if (!index.TryGetValue(key, out var set)) index[key] = set = new HashSet<Topic>();
            set.Add(topic);
        }

        private static void Unindex<TKey>(Dictionary<TKey, HashSet<Topic>> index, TKey key, Topic topic)
            where TKey : notnull
        {
            if (index.TryGetValue(key, out var set) && set.Remove(topic) && set.Count == 0)
                index.Remove(key);
        }
    }

    /// Computes a local topic through the same cached FFI path as the static
    /// functions, so a live cell and a static cell with the same request share
    /// one engine call.
    internal static class LocalCompute
    {
        public static object Compute(LocalSpec spec) =>
            LiveFields.Select(spec.Verb, spec.Field,
                Cx.ParseEnvelope(Cx.RawRpc(spec.Verb, spec.RequestJson)));
    }
}
