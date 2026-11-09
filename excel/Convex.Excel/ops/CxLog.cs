using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Text;
using System.Text.RegularExpressions;
using System.Threading;

namespace Convex.Excel.Ops
{
    /// Prepares text for the log and the connection history: one line, of
    /// bounded length, with anything that looks like a credential removed.
    /// Everything kept goes through here, so a server's or identity provider's
    /// error text cannot carry a token to disk.
    internal static class LogText
    {
        private const int MaxLength = 2000;
        private const string Hidden = "***";
        private const RegexOptions Options = RegexOptions.Compiled | RegexOptions.CultureInvariant;

        private static readonly Regex Bearer = new(@"(?i)\bbearer\s+[A-Za-z0-9\-._~+/]{8,}=*", Options);
        private static readonly Regex Jwt = new(@"\beyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+(\.[A-Za-z0-9_-]*)?", Options);
        private static readonly Regex Named = new(
            @"(?i)(""?\b(?:access_token|refresh_token|id_token|client_secret|code_verifier|authorization)\b""?\s*[:=]\s*""?)[^\s""&,;}]+",
            Options);
        private static readonly Regex Query = new(@"(?i)([?&](?:code|token)=)[^&\s""]+", Options);

        public static string Clean(string? text)
        {
            var line = (text ?? "").Replace('\r', ' ').Replace('\n', ' ');
            if (line.Length > MaxLength) line = line.Substring(0, MaxLength) + "...";
            line = Bearer.Replace(line, "Bearer " + Hidden);
            line = Jwt.Replace(line, Hidden);
            line = Named.Replace(line, "$1" + Hidden);
            return Query.Replace(line, "$1" + Hidden);
        }
    }

    internal sealed class ConnectionEvent
    {
        public ConnectionEvent(DateTime utc, string text)
        {
            Utc = utc;
            Text = text;
        }

        public DateTime Utc { get; }
        public string Text { get; }
    }

    /// The last connection events, for the Error Log form. A line repeating
    /// the one before within a minute is skipped, so a reconnect loop cannot
    /// push out the history that explains it.
    internal sealed class EventRing
    {
        private static readonly TimeSpan RepeatWindow = TimeSpan.FromMinutes(1);

        private readonly object _lock = new();
        private readonly Queue<ConnectionEvent> _events = new();
        private readonly int _capacity;
        private readonly Func<DateTime> _now;
        private string? _lastText;
        private DateTime _lastKept;

        public EventRing(int capacity, Func<DateTime>? now = null)
        {
            _capacity = capacity;
            _now = now ?? (() => DateTime.UtcNow);
        }

        /// False when the line repeats the previous one and was skipped.
        public bool Add(string text)
        {
            var now = _now();
            lock (_lock)
            {
                if (text == _lastText && now - _lastKept < RepeatWindow) return false;
                _lastText = text;
                _lastKept = now;
                _events.Enqueue(new ConnectionEvent(now, text));
                while (_events.Count > _capacity) _events.Dequeue();
                return true;
            }
        }

        /// Newest first.
        public List<ConnectionEvent> Snapshot()
        {
            lock (_lock) return _events.Reverse().ToList();
        }

        public void Clear()
        {
            lock (_lock)
            {
                _events.Clear();
                _lastText = null;
            }
        }
    }

    /// A rolling log file, written by its own thread: a caller only queues a
    /// line, so Excel's thread never waits on the disk. A line that cannot be
    /// queued or written is counted and reported when writing resumes. Nothing
    /// here throws to a caller, and its thread must not throw at all: an
    /// unhandled exception on it would end Excel.
    internal sealed class RollingLog : IDisposable
    {
        private const int MaxQueued = 10_000;
        private static readonly TimeSpan RetryAfter = TimeSpan.FromSeconds(30);
        private static readonly TimeSpan KeepOtherProcessLogs = TimeSpan.FromDays(14);

        private readonly BlockingCollection<object> _queue = new(MaxQueued);
        private readonly Thread _writer;
        private readonly string _directory;
        private readonly string _baseName;
        private readonly long _maxBytes;
        private readonly int _files;
        private FileStream? _stream;
        private string? _chosenPath;
        private volatile string _path = "";
        private long _length;
        private long _dropped;
        private DateTime _nextOpen;
        private DateTime _nextRoll;

        /// `files` counts the current file: 5 keeps the current one and four older.
        public RollingLog(string directory, string baseName = "convex-excel",
            long maxBytes = 10 * 1024 * 1024, int files = 5)
        {
            _directory = directory;
            _baseName = baseName;
            _maxBytes = maxBytes;
            _files = files;
            _writer = new Thread(Run) { IsBackground = true, Name = "Convex log" };
            _writer.Start();
        }

        /// The file being written; empty until it is open.
        public string Path => _path;

        public void Write(string line)
        {
            try
            {
                if (!_queue.TryAdd(line)) Interlocked.Increment(ref _dropped);
            }
            catch (InvalidOperationException)
            {
                // Closing: the line is lost, as any after the add-in closes.
            }
        }

        /// Waits until what is queued now has reached the file.
        public void Flush(TimeSpan timeout)
        {
            var done = new ManualResetEventSlim();
            try
            {
                if (_queue.TryAdd(done, timeout)) done.Wait(timeout);
            }
            catch (InvalidOperationException)
            {
                // Closed: Dispose has drained the queue.
            }
        }

        public void Dispose()
        {
            _queue.CompleteAdding();
            _writer.Join(TimeSpan.FromSeconds(2));
        }

        private void Run()
        {
            try
            {
                Open();
                Prune();
                foreach (var item in _queue.GetConsumingEnumerable())
                {
                    if (item is ManualResetEventSlim done)
                    {
                        FlushStream();
                        done.Set();
                        continue;
                    }
                    Append((string)item);
                    if (_queue.Count == 0) FlushStream();
                }
            }
            catch (Exception)
            {
                // The log is not worth losing Excel for.
            }
            finally
            {
                Close();
            }
        }

        private void Append(string line)
        {
            try
            {
                if (_stream == null && !Open())
                {
                    Interlocked.Increment(ref _dropped);
                    return;
                }
                if (_length >= _maxBytes) Roll();
                // A file that cannot be rolled stops growing at twice the limit.
                if (_stream == null || _length >= 2 * _maxBytes)
                {
                    Interlocked.Increment(ref _dropped);
                    return;
                }
                var dropped = Interlocked.Exchange(ref _dropped, 0);
                if (dropped > 0)
                    Put(DateTime.UtcNow.ToString("yyyy-MM-ddTHH:mm:ss.fffZ", CultureInfo.InvariantCulture) +
                        " INFO " + dropped.ToString(CultureInfo.InvariantCulture) + " lines were not written");
                Put(line);
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ObjectDisposedException)
            {
                Interlocked.Increment(ref _dropped);
                Close();
                _nextOpen = DateTime.UtcNow + RetryAfter;
            }
        }

        private void Put(string line)
        {
            var bytes = Encoding.UTF8.GetBytes(line + "\r\n");
            _stream!.Write(bytes, 0, bytes.Length);
            _length += bytes.Length;
        }

        /// Opens the log. Another Excel process holding the file means this
        /// one writes its own, for as long as it runs.
        private bool Open()
        {
            if (DateTime.UtcNow < _nextOpen) return false;
            try
            {
                Directory.CreateDirectory(_directory);
                if (_chosenPath != null) return Open(_chosenPath);
                var shared = System.IO.Path.Combine(_directory, _baseName + ".log");
                if (Open(shared)) return true;
                var own = System.IO.Path.Combine(_directory,
                    _baseName + "-" + Process.GetCurrentProcess().Id.ToString(CultureInfo.InvariantCulture) + ".log");
                if (Open(own)) return true;
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
            {
                // Retried below.
            }
            _nextOpen = DateTime.UtcNow + RetryAfter;
            return false;
        }

        private bool Open(string path)
        {
            try
            {
                _stream = new FileStream(path, FileMode.Append, FileAccess.Write, FileShare.Read);
            }
            catch (IOException)
            {
                return false;
            }
            _length = _stream.Length;
            _chosenPath = path;
            _path = path;
            return true;
        }

        private void Roll()
        {
            if (DateTime.UtcNow < _nextRoll) return;
            Close();
            var path = _chosenPath!;
            try
            {
                string Older(int n) => path + "." + n.ToString(CultureInfo.InvariantCulture);
                File.Delete(Older(_files - 1));
                for (int i = _files - 2; i >= 1; i--)
                    if (File.Exists(Older(i))) File.Move(Older(i), Older(i + 1));
                File.Move(path, Older(1));
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
            {
                // Something holds a file open (a reader without delete sharing).
                _nextRoll = DateTime.UtcNow + RetryAfter;
            }
            if (!Open(path)) _nextOpen = DateTime.UtcNow + RetryAfter;
        }

        /// Files other Excel processes wrote and left behind.
        private void Prune()
        {
            try
            {
                var cutoff = DateTime.UtcNow - KeepOtherProcessLogs;
                foreach (var file in Directory.GetFiles(_directory, _baseName + "-*"))
                {
                    try
                    {
                        if (File.GetLastWriteTimeUtc(file) < cutoff) File.Delete(file);
                    }
                    catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
                    {
                        // In use: another process's log.
                    }
                }
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
            {
                // No directory, or none readable: nothing to prune.
            }
        }

        private void FlushStream()
        {
            try
            {
                _stream?.Flush();
            }
            catch (Exception ex) when (ex is IOException or ObjectDisposedException)
            {
                Close();
                _nextOpen = DateTime.UtcNow + RetryAfter;
            }
        }

        private void Close()
        {
            try
            {
                _stream?.Dispose();
            }
            catch (IOException)
            {
                // Closing anyway.
            }
            _stream = null;
        }
    }

    /// The add-in's log file and connection history.
    internal static class CxLog
    {
        private static RollingLog? _file;

        /// What the Error Log form's Connection tab shows.
        public static EventRing Events { get; } = new(500);

        /// The log file being written; empty when there is none.
        public static string FilePath => Volatile.Read(ref _file)?.Path ?? "";

        private static string DefaultDirectory => Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Convex", "Excel", "logs");

        public static void Start(string? directory = null)
        {
            var file = new RollingLog(directory ?? DefaultDirectory);
            if (Interlocked.CompareExchange(ref _file, file, null) != null) file.Dispose();
        }

        /// Writes what is queued and closes the file.
        public static void Stop() => Interlocked.Exchange(ref _file, null)?.Dispose();

        /// To the file only.
        public static void Info(string text) => Volatile.Read(ref _file)?.Write(Line("INFO", LogText.Clean(text)));

        /// To the connection history and the file.
        public static void Connection(string text)
        {
            var clean = LogText.Clean(text);
            if (Events.Add(clean)) Volatile.Read(ref _file)?.Write(Line("CONN", clean));
        }

        private static string Line(string kind, string text) =>
            DateTime.UtcNow.ToString("yyyy-MM-ddTHH:mm:ss.fffZ", CultureInfo.InvariantCulture) + " " + kind + " " + text;
    }

    /// The last 1,024 samples of a latency, in milliseconds.
    internal sealed class LatencyWindow
    {
        public const int Capacity = 1024;

        private readonly object _lock = new();
        private readonly double[] _samples = new double[Capacity];
        private int _next;
        private int _count;

        public int Count
        {
            get { lock (_lock) return _count; }
        }

        public void Add(double milliseconds)
        {
            lock (_lock)
            {
                _samples[_next] = milliseconds;
                _next = (_next + 1) % Capacity;
                if (_count < Capacity) _count++;
            }
        }

        /// The nearest-rank percentile (0.5, 0.99); null with no samples.
        public double? Percentile(double fraction)
        {
            double[] sorted;
            lock (_lock)
            {
                if (_count == 0) return null;
                sorted = new double[_count];
                Array.Copy(_samples, sorted, _count);
            }
            Array.Sort(sorted);
            int rank = (int)Math.Ceiling(fraction * sorted.Length);
            return sorted[Math.Max(rank, 1) - 1];
        }
    }

    /// Times each server frame, from its receipt to the last topic it changed.
    /// A frame is handled start to finish on one receive thread, so the marks
    /// are per thread. A frame that changes no topic is counted, not timed.
    /// Excel's RTD throttle and recalculation come after this.
    internal sealed class FrameTimer
    {
        [ThreadStatic] private static long _received;
        [ThreadStatic] private static long _pushed;

        private long _frames;

        public long Count => Interlocked.Read(ref _frames);
        public LatencyWindow Latency { get; } = new();

        public void Begin()
        {
            Interlocked.Increment(ref _frames);
            _received = Stopwatch.GetTimestamp();
            _pushed = 0;
        }

        /// A topic took a new value. Outside a frame (a cell connecting, a timer) this does nothing.
        public static void Pushed()
        {
            if (_received != 0) _pushed = Stopwatch.GetTimestamp();
        }

        public void End()
        {
            if (_pushed != 0) Latency.Add((_pushed - _received) * 1000.0 / Stopwatch.Frequency);
            _received = 0;
            _pushed = 0;
        }
    }
}
