using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using ExcelDna.Integration;
using ExcelDna.Integration.Rtd;

namespace Convex.Excel.Bench
{
    // X0 measurement surface (excel-plan). Driven by excel/bench/Run-ExcelBench.ps1
    // through Application.Run("CXB.CTL", ...). The prefix is CXB., never CX., so
    // this add-in can load beside the real one.

    /// Synthetic RTD server: topic i is the cell =CXB.TOPIC(i). The pusher writes
    /// Stopwatch timestamps, so a dependent CXB.SEEN cell can measure latency.
    [ComVisible(true)]
    [ProgId(ProgIdName)]
    public sealed class BenchRtd : ExcelRtdServer
    {
        public const string ProgIdName = "Convex.Bench.Rtd";
        internal static BenchRtd? Instance;

        private readonly object _lock = new();
        private readonly Dictionary<int, Topic> _byIndex = new();
        private readonly Dictionary<Topic, int> _byTopic = new();

        protected override bool ServerStart()
        {
            Instance = this;
            return true;
        }

        protected override void ServerTerminate()
        {
            Pusher.Stop();
            Instance = null;
        }

        protected override object ConnectData(Topic topic, IList<string> topicInfo, ref bool newValues)
        {
            int i = int.Parse(topicInfo[0], CultureInfo.InvariantCulture);
            lock (_lock)
            {
                _byIndex[i] = topic;
                _byTopic[topic] = i;
            }
            return 0.0;
        }

        protected override void DisconnectData(Topic topic)
        {
            lock (_lock)
            {
                if (_byTopic.TryGetValue(topic, out int i))
                {
                    _byTopic.Remove(topic);
                    _byIndex.Remove(i);
                }
            }
        }

        internal Topic[] Snapshot()
        {
            lock (_lock) return _byIndex.OrderBy(k => k.Key).Select(k => k.Value).ToArray();
        }

        internal int Count
        {
            get { lock (_lock) return _byIndex.Count; }
        }
    }

    /// Background writer: `rate` topic updates per second, round-robin, for
    /// `seconds`. Each value is the Stopwatch timestamp at the moment of the push.
    internal static class Pusher
    {
        private static Thread? _thread;
        private static volatile bool _stop;
        private static long _pushed;
        private static volatile bool _running;

        public static long Pushed => Interlocked.Read(ref _pushed);
        public static bool Running => _running;

        public static string Start(int rate, double seconds)
        {
            Stop();
            var topics = BenchRtd.Instance?.Snapshot() ?? Array.Empty<ExcelRtdServer.Topic>();
            if (topics.Length == 0) return "no topics";
            _stop = false;
            Interlocked.Exchange(ref _pushed, 0);
            _running = true;
            _thread = new Thread(() => Run(topics, rate, seconds)) { IsBackground = true, Name = "cxb-push" };
            _thread.Start();
            return "pushing " + topics.Length.ToString(CultureInfo.InvariantCulture);
        }

        public static void Stop()
        {
            _stop = true;
            _thread?.Join(2000);
            _thread = null;
        }

        private static void Run(ExcelRtdServer.Topic[] topics, int rate, double seconds)
        {
            try
            {
                var sw = Stopwatch.StartNew();
                long n = 0;
                int idx = 0;
                while (!_stop && sw.Elapsed.TotalSeconds < seconds)
                {
                    long due = (long)(sw.Elapsed.TotalSeconds * rate);
                    while (n < due)
                    {
                        topics[idx].UpdateValue((double)Stopwatch.GetTimestamp());
                        idx = (idx + 1) % topics.Length;
                        n++;
                    }
                    Interlocked.Exchange(ref _pushed, n);
                    Thread.Sleep(5);
                }
            }
            finally
            {
                _running = false;
            }
        }
    }

    /// Fixed-capacity, lock-free append log of CXB.SEEN evaluations.
    internal static class Recorder
    {
        private const int Cap = 4_000_000;
        private const int MaxId = 100_000;

        private static readonly long[] _t = new long[Cap];
        private static readonly int[] _id = new int[Cap];
        private static readonly double[] _latMs = new double[Cap];
        private static readonly bool[] _changed = new bool[Cap];
        private static readonly double[] _prev = new double[MaxId + 1];
        private static readonly bool[] _hasPrev = new bool[MaxId + 1];
        private static readonly object _prevLock = new();
        private static int _n;
        private static long _nonNumeric;
        private static readonly List<(string Label, long T)> _marks = new();

        public static int Count => Math.Min(Volatile.Read(ref _n), Cap);

        public static void Reset()
        {
            Interlocked.Exchange(ref _n, 0);
            Interlocked.Exchange(ref _nTouch, 0);
            Interlocked.Exchange(ref _nonNumeric, 0);
            lock (_marks) _marks.Clear();
            // Keep _prev: "changed" is relative to the value before the reset.
        }

        public static void Mark(string label)
        {
            lock (_marks) _marks.Add((label, Stopwatch.GetTimestamp()));
        }

        public static void NonNumeric() => Interlocked.Increment(ref _nonNumeric);

        // Evaluation times of the cells under test (CXB.TOPIC), so a refresh
        // batch spans the RTD cells' own recalculation, not only CXB.SEEN's.
        private static readonly long[] _touch = new long[Cap];
        private static int _nTouch;

        public static void Touch()
        {
            int k = Interlocked.Increment(ref _nTouch) - 1;
            if (k < Cap) _touch[k] = Stopwatch.GetTimestamp();
        }

        public static void Add(int id, double value, bool latency)
        {
            long now = Stopwatch.GetTimestamp();
            bool changed = true;
            if (id >= 0 && id <= MaxId)
            {
                lock (_prevLock)
                {
                    changed = !_hasPrev[id] || !_prev[id].Equals(value);
                    _prev[id] = value;
                    _hasPrev[id] = true;
                }
            }
            int k = Interlocked.Increment(ref _n) - 1;
            if (k >= Cap) return;
            _t[k] = now;
            _id[k] = id;
            _latMs[k] = latency ? (now - value) * 1000.0 / Stopwatch.Frequency : double.NaN;
            _changed[k] = changed;
        }

        /// Records after the most recent mark whose value changed.
        public static int ChangedSinceMark()
        {
            long since = LastMark();
            int count = 0, n = Count;
            for (int k = 0; k < n; k++)
                if (_changed[k] && _t[k] >= since) count++;
            return count;
        }

        private static long LastMark()
        {
            lock (_marks) return _marks.Count == 0 ? 0 : _marks[_marks.Count - 1].T;
        }

        public static void Write(JsonOut j, double batchGapMs)
        {
            int n = Count;
            var order = Enumerable.Range(0, n).OrderBy(k => _t[k]).ToArray();
            double toMs = 1000.0 / Stopwatch.Frequency;

            var lat = new List<double>();
            foreach (int k in order) if (!double.IsNaN(_latMs[k])) lat.Add(_latMs[k]);
            j.Num("evaluations", n);
            j.Num("non_numeric", Interlocked.Read(ref _nonNumeric));
            j.Num("changed", order.Count(k => _changed[k]));
            j.Stats("latency_ms", lat);

            // Refresh batches over every recorded evaluation (CXB.SEEN cells and,
            // when present, the CXB.TOPIC cells they watch), split where the
            // timeline has a gap above batchGapMs. Span = one refresh's recalc time.
            int nTouch = Math.Min(Volatile.Read(ref _nTouch), Cap);
            var times = new long[n + nTouch];
            for (int k = 0; k < n; k++) times[k] = _t[k];
            Array.Copy(_touch, 0, times, n, nTouch);
            Array.Sort(times);
            var sizes = new List<double>();
            var spans = new List<double>();
            int start = 0;
            for (int i = 1; i <= times.Length; i++)
            {
                bool split = i == times.Length || (times[i] - times[i - 1]) * toMs > batchGapMs;
                if (!split) continue;
                sizes.Add(i - start);
                spans.Add((times[i - 1] - times[start]) * toMs);
                start = i;
            }
            j.Num("topic_evaluations", nTouch);
            j.Num("batch_gap_ms", batchGapMs);
            j.Num("batches", sizes.Count);
            j.Stats("batch_evaluations", sizes);
            j.Stats("batch_span_ms", spans);

            List<(string Label, long T)> marks;
            lock (_marks) marks = _marks.ToList();
            j.Begin("marks");
            foreach (var (label, t) in marks)
            {
                // First and last changed evaluation after this mark.
                long first = long.MaxValue, last = long.MinValue;
                int changed = 0;
                foreach (int k in order)
                {
                    if (_t[k] < t || !_changed[k]) continue;
                    changed++;
                    if (_t[k] < first) first = _t[k];
                    if (_t[k] > last) last = _t[k];
                }
                j.Begin(label);
                j.Num("changed", changed);
                if (changed > 0)
                {
                    j.Num("first_ms", (first - t) * toMs);
                    j.Num("last_ms", (last - t) * toMs);
                }
                j.End();
            }
            j.End();
        }
    }

    /// Main-thread availability probe: every intervalMs a background timer queues
    /// a macro and records how long Excel took to run it.
    internal static class Probe
    {
        private static Timer? _timer;
        private static readonly List<double> _delays = new();

        public static void Start(int intervalMs)
        {
            Stop();
            lock (_delays) _delays.Clear();
            _timer = new Timer(_ =>
            {
                long t0 = Stopwatch.GetTimestamp();
                ExcelAsyncUtil.QueueAsMacro(() =>
                {
                    double ms = (Stopwatch.GetTimestamp() - t0) * 1000.0 / Stopwatch.Frequency;
                    lock (_delays) _delays.Add(ms);
                });
            }, null, intervalMs, intervalMs);
        }

        public static void Stop()
        {
            _timer?.Dispose();
            _timer = null;
        }

        public static void Write(JsonOut j)
        {
            List<double> copy;
            lock (_delays) copy = _delays.ToList();
            j.Stats("main_thread_delay_ms", copy);
        }
    }

    /// Minimal invariant-culture JSON writer (no package dependency).
    internal sealed class JsonOut
    {
        private readonly StringBuilder _sb = new();
        private readonly Stack<bool> _first = new();

        public JsonOut() { _sb.Append('{'); _first.Push(true); }

        private void Key(string k)
        {
            if (!_first.Pop()) _sb.Append(',');
            _first.Push(false);
            _sb.Append('"').Append(k.Replace("\"", "'")).Append("\":");
        }

        public void Str(string k, string v) { Key(k); _sb.Append('"').Append(v.Replace("\\", "\\\\").Replace("\"", "'")).Append('"'); }

        public void Num(string k, double v)
        {
            Key(k);
            _sb.Append(double.IsNaN(v) || double.IsInfinity(v) ? "null" : v.ToString("R", CultureInfo.InvariantCulture));
        }

        public void Begin(string k) { Key(k); _sb.Append('{'); _first.Push(true); }
        public void End() { _first.Pop(); _sb.Append('}'); }

        public void Stats(string k, List<double> xs)
        {
            Begin(k);
            Num("n", xs.Count);
            if (xs.Count > 0)
            {
                var s = xs.OrderBy(x => x).ToArray();
                double P(double q) => s[Math.Min(s.Length - 1, (int)Math.Floor(q * (s.Length - 1) + 0.5))];
                Num("p50", P(0.50));
                Num("p90", P(0.90));
                Num("p99", P(0.99));
                Num("max", s[s.Length - 1]);
                Num("mean", s.Average());
            }
            End();
        }

        public override string ToString() => _sb.ToString() + "}";
    }

    public static class BenchFunctions
    {
        [ExcelFunction(Name = "CXB.TOPIC", Description = "Synthetic RTD topic i (X0 bench).")]
        public static object Topic(int i)
        {
            Recorder.Touch();
            return XlCall.RTD(BenchRtd.ProgIdName, null, i.ToString(CultureInfo.InvariantCulture));
        }

        /// Pass-through recorder. Put it in a separate cell that references the
        /// cell under test, so it recalculates only when that cell changes.
        /// latency=TRUE treats the value as a Stopwatch timestamp (CXB.TOPIC).
        [ExcelFunction(Name = "CXB.SEEN", IsThreadSafe = true,
            Description = "Records when this cell recalculates (X0 bench).")]
        public static object Seen(object value, int id, bool latency)
        {
            if (value is double d) Recorder.Add(id, d, latency);
            else Recorder.NonNumeric();
            return value;
        }

        [DllImport("convex_ffi.dll", CallingConvention = CallingConvention.Cdecl)]
        private static extern IntPtr convex_version();

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
        private static extern IntPtr GetModuleHandle(string name);

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
        private static extern int GetModuleFileName(IntPtr module, StringBuilder path, int size);

        /// Native-packing spike: did convex_ffi.dll load, and from where?
        [ExcelFunction(Name = "CXB.NATIVE", Description = "convex_ffi load status (X0 bench).")]
        public static object Native()
        {
            try
            {
                // convex_version returns a static string: do not free it.
                string version = Marshal.PtrToStringAnsi(convex_version()) ?? "?";
                var path = new StringBuilder(1024);
                IntPtr h = GetModuleHandle("convex_ffi.dll");
                if (h != IntPtr.Zero) GetModuleFileName(h, path, path.Capacity);
                return "OK " + version + " from " + path;
            }
            catch (Exception ex)
            {
                return "FAIL " + ex.GetType().Name + ": " + ex.Message;
            }
        }

        /// Control entry point for the driver (Application.Run).
        [ExcelFunction(Name = "CXB.CTL", Description = "Bench control (X0 bench).")]
        public static object Ctl(string cmd, object a, object b)
        {
            try
            {
                switch (cmd)
                {
                    case "reset":
                        Pusher.Stop();
                        Probe.Stop();
                        Recorder.Reset();
                        return "ok";
                    case "mark":
                        Recorder.Mark(a as string ?? "mark");
                        return "ok";
                    case "probe":
                        Probe.Start(ToInt(a, 50));
                        return "ok";
                    case "push":
                        return Pusher.Start(ToInt(a, 1000), b is double s ? s : 10.0);
                    case "status":
                        return string.Format(CultureInfo.InvariantCulture,
                            "topics={0} pushing={1} pushed={2} evaluations={3} changed_since_mark={4}",
                            BenchRtd.Instance?.Count ?? 0, Pusher.Running, Pusher.Pushed,
                            Recorder.Count, Recorder.ChangedSinceMark());
                    case "changed":
                        return (double)Recorder.ChangedSinceMark();
                    case "dump":
                        Pusher.Stop();
                        Probe.Stop();
                        var j = new JsonOut();
                        j.Num("topics", BenchRtd.Instance?.Count ?? 0);
                        j.Num("pushed", Pusher.Pushed);
                        Recorder.Write(j, b is double gap ? gap : 20.0);
                        Probe.Write(j);
                        File.WriteAllText((string)a, j.ToString());
                        return "ok";
                    default:
                        return "unknown command " + cmd;
                }
            }
            catch (Exception ex)
            {
                return "FAIL " + ex.Message;
            }
        }

        private static int ToInt(object o, int fallback) =>
            o is double d ? (int)d : fallback;
    }
}
