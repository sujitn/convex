using System;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;
using System.Net.Http;
using System.Threading;
using System.Threading.Tasks;

namespace Convex.Excel.Server
{
    /// Timings of the server link; tests shorten them.
    internal sealed class ServerTimings
    {
        /// How often subscription changes are batched and timers checked.
        public TimeSpan Tick { get; set; } = TimeSpan.FromMilliseconds(100);
        /// How long an instrument stays subscribed after its last cell goes,
        /// so a recalculation that disconnects and reconnects a topic costs nothing.
        public TimeSpan Linger { get; set; } = TimeSpan.FromSeconds(30);
        /// The server sends a heartbeat every 30 s; silence this long ends the session.
        public TimeSpan Watchdog { get; set; } = TimeSpan.FromSeconds(45);
        public TimeSpan ConnectTimeout { get; set; } = TimeSpan.FromSeconds(10);
        public TimeSpan BackoffInitial { get; set; } = TimeSpan.FromMilliseconds(500);
        public TimeSpan BackoffMax { get; set; } = TimeSpan.FromSeconds(30);
        /// A session that lived this long resets the reconnect backoff.
        public TimeSpan HealthyAfter { get; set; } = TimeSpan.FromSeconds(30);
        /// When a quota or availability refusal is retried.
        public TimeSpan RetryRefused { get; set; } = TimeSpan.FromSeconds(60);
        /// The server's Retry-After for a refused connection (fixed at 60 s).
        public TimeSpan TooManyRequests { get; set; } = TimeSpan.FromSeconds(60);
        /// At most one resubscription for a runtime switch in this interval.
        public TimeSpan ReseedInterval { get; set; } = TimeSpan.FromSeconds(5);
        /// A status read that failed (other than a 429) is retried after this,
        /// doubling to BackoffMax.
        public TimeSpan StatusRetry { get; set; } = TimeSpan.FromSeconds(1);
    }

    internal enum LinkState { NotConfigured, SignedOut, Connecting, Connected, Reconnecting }

    /// Where an instrument stands with the server.
    internal enum Subscription
    {
        /// Not followed by any cell.
        None,
        /// To be subscribed with the next batch.
        Wanted,
        /// Subscribed; the acknowledgement has not arrived.
        Sent,
        /// Subscribed and acknowledged.
        Live,
        /// The server refused it (see the problem).
        Refused,
        /// Beyond the 256 instruments one session follows.
        OverCap,
    }

    /// One instrument as a cell sees it.
    internal sealed class InstrumentView
    {
        public string InstrumentId { get; set; } = "";
        public Subscription Sub { get; set; }
        /// The newest frame received for it, possibly from an earlier session.
        public Valuation? Latest { get; set; }
        /// Latest arrived on the open session, from the current runtime.
        public bool Fresh { get; set; }
        /// The open session has finished sending its current result.
        public bool Acked { get; set; }
        public string? ProblemCode { get; set; }
        public string? ProblemMessage { get; set; }
        public LinkState Link { get; set; }
        public string? LinkProblem { get; set; }
        public string? Runtime { get; set; }
        public string Server { get; set; } = "";
    }

    /// The link as CX.STATUS and the ribbon show it.
    internal sealed class SourceStatus
    {
        public LinkState Link { get; set; }
        public string Connection { get; set; } = "";
        public string User { get; set; } = "";
        public string Server { get; set; } = "";
        public string? Runtime { get; set; }
        public DateTime? LastMessageUtc { get; set; }
        public int Instruments { get; set; }
        public int Books { get; set; }
        public string? SessionId { get; set; }
        public string? Problem { get; set; }
    }

    /// The server link: one WebSocket session per Excel process (DX7), the
    /// instruments its cells follow, and the newest result of each.
    ///
    /// - Subscriptions change in batches, once per tick (100 ms); an instrument
    ///   stays subscribed for the linger time after its last cell goes.
    /// - A subscription receives each instrument's current result before its
    ///   acknowledgement (server D99), so the stream alone seeds the cells.
    /// - Frames are kept per instrument when newer by (runtime_id, revision);
    ///   a new runtime resubscribes everything to reseed.
    /// - RESYNC_REQUIRED resubscribes what the session follows. A refused batch
    ///   is split in halves until the refused instruments are found (DX14);
    ///   they stay refused until Reconnect or a new configuration.
    /// - A lost session keeps every value (quality Stale, DX6) and reconnects
    ///   with jittered backoff. The server's heartbeat drives a watchdog.
    /// - A renewed token opens a second session, which takes over once it has
    ///   resubscribed everything (make-before-break, DX15): the server reads
    ///   a token only at the upgrade and closes a session when it expires.
    /// - Books (CX.LIMIT) follow on the same session; see ServerSource.Limits.cs.
    internal sealed partial class ServerSource : ISessionEvents, IDisposable
    {
        /// One session follows at most this many instruments (server D100).
        public const int MaxInstruments = 256;

        private sealed class Inst
        {
            public readonly string Id;
            public readonly long Seq;
            public int Refs;
            public DateTime? LingerUntil;
            public Subscription Sub = Subscription.Wanted;
            public Valuation? Latest;
            public bool Fresh;
            public bool Acked;
            public string? ProblemCode;
            public string? ProblemMessage;
            public DateTime? RetryAt;
            /// The refusal already in the connection history. ClearProblem
            /// leaves it, so a refusal retried each minute is said once.
            public string? LoggedRefusal;
            public string? TooLargeRuntime;
            public ulong TooLargeRevision;
            public readonly List<TaskCompletionSource<bool>> Waiters = new();

            public Inst(string id, long seq)
            {
                Id = id;
                Seq = seq;
            }
        }

        private readonly ILiveTransport _transport;
        private readonly ServerTimings _timings;
        private readonly Func<DateTime> _now;
        private readonly object _gate = new();
        private readonly Dictionary<string, Inst> _instruments = new(StringComparer.Ordinal);
        private readonly Random _random = new();
        private readonly HashSet<string> _changed = new(StringComparer.Ordinal);
        private Timer? _timer;
        private int _ticking;
        private bool _linkChanged;
        private bool _infoChanged;
        private long _seq;

        private Uri? _uri;
        private Uri? _httpBase;
        private string _server = "";
        private string? _configProblem;
        private ITokenProvider _tokens = new NoTokenProvider();

        private ServerSession? _pendingPrimary;
        private ServerSession? _primary;
        private ServerSession? _pendingStandby;
        private ServerSession? _standby;
        private bool _hadSession;
        private TimeSpan _backoff;
        private DateTime _nextAttempt;
        private TimeSpan _standbyBackoff;
        private DateTime _nextStandbyAttempt;

        private LinkState _link = LinkState.NotConfigured;
        private string? _linkProblem;
        private string? _runtime;
        private DateTime _lastReseed;
        private DateTime? _lastMessage;
        private string? _lastStreamError;

        public ServerSource(ILiveTransport transport, ServerTimings? timings = null, Func<DateTime>? utcNow = null,
            HttpClient? http = null)
        {
            _transport = transport;
            _timings = timings ?? new ServerTimings();
            _now = utcNow ?? (() => DateTime.UtcNow);
            _http = http ?? new HttpClient { Timeout = TimeSpan.FromSeconds(30) };
        }

        /// Instruments whose view changed.
        public event Action<IReadOnlyCollection<string>>? InstrumentsChanged;

        /// The link changed (true: connection, runtime or session, which
        /// changes every cell's quality) or only its details (false).
        public event Action<bool>? StatusChanged;

        /// Frames received, and how long each took to reach its topics.
        public Ops.FrameTimer Frames { get; } = new();

        /// Point the link at a server (or at none) with a token source. Every
        /// cached result is dropped; the cells stay and resubscribe.
        public void Configure(ServerConfig? config, ITokenProvider tokens)
        {
            lock (_gate)
            {
                RetireAll();
                _tokens = tokens;
                _uri = null;
                _httpBase = null;
                _configProblem = null;
                _server = config?.Url ?? "";
                if (config is { Configured: true })
                {
                    try
                    {
                        _uri = config.WebSocketUri();
                        _httpBase = config.HttpBase();
                    }
                    catch (ConvexException ex)
                    {
                        _configProblem = ex.Message;
                    }
                }
                _hadSession = false;
                _backoff = TimeSpan.Zero;
                _standbyBackoff = TimeSpan.Zero;
                _nextAttempt = _nextStandbyAttempt = _now();
                _runtime = null;
                _lastMessage = null;
                _lastStreamError = null;
                foreach (var inst in _instruments.Values)
                {
                    inst.Latest = null;
                    inst.Fresh = inst.Acked = false;
                    inst.TooLargeRuntime = null;
                    inst.LoggedRefusal = null;
                    ClearProblem(inst);
                    if (inst.Sub is Subscription.Sent or Subscription.Live or Subscription.Refused)
                        inst.Sub = Subscription.Wanted;
                    _changed.Add(inst.Id);
                }
                ConfigureBooks();
                Ops.CxLog.Connection(_uri != null ? "server set to " + _server
                    : _configProblem != null ? "server not usable: " + _configProblem
                    : "no server configured");
                _link = (LinkState)(-1); // force a link notification
                UpdateLink(null);
                _timer ??= new Timer(_ => Tick(), null, _timings.Tick, _timings.Tick);
            }
            Notify();
        }

        /// The server's HTTP base and the current token, for HTTP calls; null
        /// when there is no server or no sign-in.
        public (Uri Base, string? Token)? HttpEndpoint()
        {
            lock (_gate)
                return _httpBase != null && _tokens.Ready ? (_httpBase, _tokens.Token) : null;
        }

        /// The token source in use (for a 401 on an HTTP call).
        public ITokenProvider Tokens
        {
            get { lock (_gate) return _tokens; }
        }

        /// A cell follows `instrumentId`. Never calls back (the next tick
        /// reports the change), so a caller may hold its own lock.
        public void Acquire(string instrumentId)
        {
            lock (_gate) AcquireLocked(instrumentId);
        }

        /// A cell stopped following `instrumentId`. Never calls back.
        public void Release(string instrumentId)
        {
            lock (_gate)
            {
                if (!_instruments.TryGetValue(instrumentId, out var inst) || inst.Refs == 0) return;
                if (--inst.Refs == 0) inst.LingerUntil = _now() + _timings.Linger;
            }
        }

        /// Drop the session, forget refusals and connect again now.
        public void Reconnect()
        {
            lock (_gate)
            {
                Ops.CxLog.Connection("reconnect requested");
                RetireAll();
                LoseSession();
                foreach (var inst in _instruments.Values)
                {
                    if (inst.Sub == Subscription.Refused)
                    {
                        inst.Sub = Subscription.Wanted;
                        inst.LoggedRefusal = null;
                        ClearProblem(inst);
                        _changed.Add(inst.Id);
                    }
                }
                RetryRefusedBooks(all: true);
                _backoff = TimeSpan.Zero;
                _nextAttempt = _now();
                UpdateLink(null);
            }
            Notify();
        }

        public InstrumentView View(string instrumentId)
        {
            lock (_gate)
            {
                var view = new InstrumentView
                {
                    InstrumentId = instrumentId,
                    Link = _link,
                    LinkProblem = _linkProblem ?? _configProblem ?? _tokens.Problem,
                    Runtime = _runtime,
                    Server = _server,
                };
                if (!_instruments.TryGetValue(instrumentId, out var inst)) return view;
                view.Sub = inst.Sub;
                view.Latest = inst.Latest;
                view.Fresh = IsFresh(inst);
                view.Acked = inst.Acked && _primary != null;
                view.ProblemCode = inst.ProblemCode;
                view.ProblemMessage = inst.ProblemMessage;
                return view;
            }
        }

        public SourceStatus Status()
        {
            lock (_gate)
            {
                return new SourceStatus
                {
                    Link = _link,
                    Connection = _link switch
                    {
                        LinkState.NotConfigured => _configProblem == null
                            ? "Not configured"
                            : "Not configured: " + _configProblem,
                        LinkState.SignedOut => "Signed out" + (_tokens.Problem is { } p ? ": " + p : ""),
                        LinkState.Connecting => "Connecting",
                        LinkState.Connected => "Connected",
                        _ => "Reconnecting" + (_linkProblem is { } r ? ": " + r : ""),
                    },
                    User = _tokens.User,
                    Server = _server,
                    Runtime = _runtime,
                    LastMessageUtc = _lastMessage,
                    Instruments = _instruments.Values.Count(i => i.Sub is Subscription.Sent or Subscription.Live),
                    Books = _books.Values.Count(b => b.Sub is Subscription.Sent or Subscription.Live),
                    SessionId = _primary?.SessionId,
                    Problem = _lastStreamError,
                };
            }
        }

        /// Follow `instrumentId` until its current result arrives, the server
        /// says it has none, or it is refused; then let it go (it lingers).
        public async Task<InstrumentView> SnapshotAsync(string instrumentId, TimeSpan timeout, CancellationToken ct)
        {
            var done = new TaskCompletionSource<bool>(TaskCreationOptions.RunContinuationsAsynchronously);
            Inst inst;
            lock (_gate)
            {
                inst = AcquireLocked(instrumentId);
                inst.Waiters.Add(done);
                if (Satisfied(inst)) done.TrySetResult(true);
            }
            try
            {
                await Task.WhenAny(done.Task, Task.Delay(timeout, ct)).ConfigureAwait(false);
            }
            finally
            {
                lock (_gate) inst.Waiters.Remove(done);
                Release(instrumentId);
            }
            ct.ThrowIfCancellationRequested();
            return View(instrumentId);
        }

        public void Dispose()
        {
            lock (_gate)
            {
                _timer?.Dispose();
                _timer = null;
                RetireAll();
            }
        }

        // ---------------------------------------------------------------
        // Tick: batching, timers, connection and renewal.
        // ---------------------------------------------------------------

        internal void Tick()
        {
            if (Interlocked.Exchange(ref _ticking, 1) == 1) return;
            try
            {
                lock (_gate)
                {
                    var now = _now();
                    if (_uri == null || !_tokens.Ready)
                    {
                        if (_primary != null || _pendingPrimary != null || _standby != null || _pendingStandby != null)
                        {
                            RetireAll();
                            LoseSession();
                        }
                        UpdateLink(null);
                        ExpireLingering(now, unsubscribe: false);
                        ExpireLingeringBooks(now, unsubscribe: false);
                        return;
                    }

                    if (_primary != null && now - _primary.LastMessageUtc > _timings.Watchdog)
                        _primary.Abort("no message from the server for " + Seconds(_timings.Watchdog));
                    if (_standby != null && now - _standby.LastMessageUtc > _timings.Watchdog)
                        _standby.Abort("no message from the server for " + Seconds(_timings.Watchdog));

                    if (_primary == null && _pendingPrimary == null && now >= _nextAttempt)
                    {
                        _pendingPrimary = Open(_tokens.Token);
                        UpdateLink(_linkProblem);
                    }

                    var token = _tokens.Token;
                    if (_primary != null && token != _primary.Token && _standby == null &&
                        _pendingStandby == null && now >= _nextStandbyAttempt)
                        _pendingStandby = Open(token);

                    ExpireLingering(now, unsubscribe: true);

                    foreach (var inst in _instruments.Values)
                    {
                        if (inst.Sub == Subscription.Refused && inst.RetryAt is { } retry && retry <= now)
                        {
                            inst.Sub = Subscription.Wanted;
                            ClearProblem(inst);
                            _changed.Add(inst.Id);
                        }
                    }
                    PromoteOverCap();
                    Flush();
                    TickBooks(now);
                }
            }
            catch (Exception ex)
            {
                CxErrorStore.Record(ex, "(server)", "server link");
            }
            finally
            {
                Volatile.Write(ref _ticking, 0);
                Notify();
            }
        }

        private ServerSession Open(string? token)
        {
            var session = new ServerSession(_transport, _uri!, token, this);
            session.Start(_timings.ConnectTimeout);
            return session;
        }

        private void ExpireLingering(DateTime now, bool unsubscribe)
        {
            List<string>? gone = null;
            foreach (var inst in _instruments.Values)
            {
                if (inst.Refs > 0 || inst.LingerUntil is not { } until || until > now) continue;
                (gone ??= new List<string>()).Add(inst.Id);
            }
            if (gone == null) return;
            var unsub = new List<string>();
            foreach (var id in gone)
            {
                var inst = _instruments[id];
                _instruments.Remove(id);
                if (inst.Sub is Subscription.Sent or Subscription.Live) unsub.Add(id);
            }
            if (unsubscribe && _primary != null)
                foreach (var chunk in Chunks(unsub))
                {
                    _primary.Unsubscribe(chunk);
                    _standby?.Unsubscribe(chunk);
                }
            PromoteOverCap();
        }

        private int Active() => _instruments.Values.Count(i =>
            i.Sub is Subscription.Sent or Subscription.Live || (i.Sub == Subscription.Wanted && i.Refs > 0));

        private void PromoteOverCap()
        {
            int active = Active();
            foreach (var inst in _instruments.Values.Where(i => i.Sub == Subscription.OverCap && i.Refs > 0)
                         .OrderBy(i => i.Seq).ToList())
            {
                if (active >= MaxInstruments) break;
                inst.Sub = Subscription.Wanted;
                ClearProblem(inst);
                _changed.Add(inst.Id);
                active++;
            }
        }

        /// Send the wanted instruments, oldest first.
        private void Flush()
        {
            if (_primary == null) return;
            var wanted = _instruments.Values.Where(i => i.Sub == Subscription.Wanted && i.Refs > 0)
                .OrderBy(i => i.Seq).Select(i => i.Id).ToList();
            foreach (var chunk in Chunks(wanted))
            {
                _primary.Subscribe(chunk);
                _standby?.Subscribe(chunk);
                foreach (var id in chunk) _instruments[id].Sub = Subscription.Sent;
            }
        }

        private Inst AcquireLocked(string instrumentId)
        {
            if (!_instruments.TryGetValue(instrumentId, out var inst))
            {
                inst = new Inst(instrumentId, ++_seq);
                if (Active() >= MaxInstruments)
                {
                    inst.Sub = Subscription.OverCap;
                    inst.ProblemCode = "SESSION_LIMIT";
                    inst.ProblemMessage = $"one server session follows at most {MaxInstruments} instruments; " +
                                          "remove live cells of other instruments to follow this one";
                }
                _instruments[instrumentId] = inst;
                _changed.Add(instrumentId);
            }
            inst.Refs++;
            inst.LingerUntil = null;
            return inst;
        }

        // ---------------------------------------------------------------
        // Session events (receive threads).
        // ---------------------------------------------------------------

        public void Connected(ServerSession session, string sessionId)
        {
            lock (_gate)
            {
                _lastMessage = _now();
                _infoChanged = true;
                if (session == _pendingPrimary)
                {
                    _pendingPrimary = null;
                    _primary = session;
                    _hadSession = true;
                    _linkProblem = null;
                    Ops.CxLog.Connection($"session {sessionId} open ({_server})");
                    UpdateLink(null);
                    Flush();
                    FlushBooks();
                }
                else if (session == _pendingStandby)
                {
                    _pendingStandby = null;
                    _standby = session;
                    Ops.CxLog.Connection($"renewal session {sessionId} open ({_server})");
                    var followed = _instruments.Values
                        .Where(i => i.Sub is Subscription.Sent or Subscription.Live)
                        .OrderBy(i => i.Seq).Select(i => i.Id).ToList();
                    foreach (var chunk in Chunks(followed)) session.Subscribe(chunk);
                    int books = SubscribeFollowedBooks(session);
                    if (followed.Count == 0 && books == 0) Promote();
                }
            }
            Notify();
        }

        public void Valuation(ServerSession session, Valuation valuation)
        {
            lock (_gate)
            {
                if (session != _primary && session != _standby) return;
                var now = _now();
                _lastMessage = now;
                _infoChanged = true;
                if (_runtime != valuation.RuntimeId)
                {
                    var previous = _runtime;
                    _runtime = valuation.RuntimeId;
                    Ops.CxLog.Connection("runtime " + (previous == null ? "" : previous + " -> ") + _runtime);
                    _linkChanged = true;
                    foreach (var id in _instruments.Keys) _changed.Add(id);
                    if (previous != null && now - _lastReseed >= _timings.ReseedInterval)
                    {
                        // A new runtime: every instrument's current result comes
                        // from it, and a quiet instrument would not publish soon.
                        _lastReseed = now;
                        var live = _instruments.Values.Where(i => i.Sub == Subscription.Live)
                            .OrderBy(i => i.Seq).Select(i => i.Id).ToList();
                        foreach (var chunk in Chunks(live)) session.Subscribe(chunk);
                    }
                }
                if (!_instruments.TryGetValue(valuation.InstrumentId, out var inst)) return;
                if (inst.TooLargeRuntime == valuation.RuntimeId && valuation.Revision <= inst.TooLargeRevision)
                    return;
                // Revisions compare within the open session (and the session
                // that replaces it on a token renewal, on the same server).
                // After a lost session the first frame is taken as it is: a
                // restarted server may number its revisions afresh.
                if (inst.Latest != null && inst.Fresh &&
                    !valuation.Supersedes(inst.Latest.RuntimeId, inst.Latest.Revision))
                    return;
                inst.Latest = valuation;
                inst.Fresh = true;
                if (inst.ProblemCode == "RESULT_TOO_LARGE")
                {
                    ClearProblem(inst);
                    inst.TooLargeRuntime = null;
                }
                _changed.Add(inst.Id);
                CompleteWaiters(inst);
            }
            Notify();
        }

        public void Subscribed(ServerSession session, string[] ids)
        {
            lock (_gate)
            {
                if (session != _primary && session != _standby) return;
                _lastMessage = _now();
                foreach (var id in ids)
                {
                    if (!_instruments.TryGetValue(id, out var inst)) continue;
                    if (inst.Sub == Subscription.Sent) inst.Sub = Subscription.Live;
                    if (inst.Sub != Subscription.Live) continue;
                    inst.LoggedRefusal = null;
                    inst.Acked = true;
                    _changed.Add(id);
                    CompleteWaiters(inst);
                }
                if (session == _standby && !session.IsClosed && session.PendingSubscribes == 0) Promote();
            }
            Notify();
        }

        public void SubscribeRefused(ServerSession session, string[] ids, string code, string message)
        {
            bool rejected = false;
            lock (_gate)
            {
                if (session != _primary && session != _standby) return;
                if (code == "UNAUTHENTICATED")
                {
                    rejected = true;
                    session.Abort("the server no longer accepts the sign-in");
                }
                else if (ids.Length > 1)
                {
                    // The server refuses a whole message without naming the
                    // instrument: split until each refused one stands alone.
                    int half = ids.Length / 2;
                    session.Subscribe(ids.Take(half).ToArray());
                    session.Subscribe(ids.Skip(half).ToArray());
                }
                else if (ids.Length == 1 && _instruments.TryGetValue(ids[0], out var inst))
                {
                    // A quota refusal is retried each minute; say so once.
                    if (inst.LoggedRefusal != code)
                    {
                        inst.LoggedRefusal = code;
                        Ops.CxLog.Connection($"instrument {ids[0]} refused: {code} {message}");
                    }
                    inst.Sub = Subscription.Refused;
                    inst.ProblemCode = code;
                    inst.ProblemMessage = Refusal(code, message, ids[0]);
                    inst.RetryAt = code is "ACCESS_QUOTA_EXCEEDED" or "ACCESS_UNAVAILABLE"
                        ? _now() + _timings.RetryRefused
                        : null;
                    _changed.Add(inst.Id);
                    CompleteWaiters(inst);
                }
                if (session == _standby && !session.IsClosed && session.PendingSubscribes == 0) Promote();
            }
            if (rejected) _tokens.Rejected();
            Notify();
        }

        public void StreamError(ServerSession session, string code, string message)
        {
            lock (_gate)
            {
                if (session != _primary && session != _standby) return;
                _lastMessage = _now();
                if (code == "RESYNC_REQUIRED")
                {
                    Ops.CxLog.Connection("resync required: " + message);
                    // Which stream lagged shows only in the text (DX19); an
                    // unknown text resyncs both.
                    bool limits = LimitErrors.IsLimitsResync(message);
                    if (limits || !LimitErrors.IsValuationsResync(message)) ResyncBooks();
                    if (!limits)
                    {
                        // The valuation stream lagged: resubscribing resends each current result.
                        var live = _instruments.Values.Where(i => i.Sub == Subscription.Live)
                            .OrderBy(i => i.Seq).Select(i => i.Id).ToList();
                        foreach (var chunk in Chunks(live)) session.Subscribe(chunk);
                    }
                }
                else if (code == "RESULT_TOO_LARGE" &&
                         TooLarge.TryParse(message, out var id, out var runtime, out var revision) &&
                         _instruments.TryGetValue(id, out var inst) &&
                         (inst.Latest == null || inst.Latest.RuntimeId != runtime || revision > inst.Latest.Revision))
                {
                    // Not resubscribed: that would only repeat the error.
                    if (inst.ProblemCode != code) Ops.CxLog.Connection($"instrument {id} is too large to stream: {message}");
                    inst.TooLargeRuntime = runtime;
                    inst.TooLargeRevision = revision;
                    inst.ProblemCode = code;
                    inst.ProblemMessage = $"the server's result for {id} (runtime {runtime}, revision " +
                                          $"{revision.ToString(CultureInfo.InvariantCulture)}) is larger than " +
                                          "one 64 KiB message, so it cannot be streamed";
                    _changed.Add(id);
                    CompleteWaiters(inst);
                }
                else
                {
                    _lastStreamError = code + ": " + message;
                    Ops.CxLog.Connection("stream error " + _lastStreamError);
                    _infoChanged = true;
                }
            }
            Notify();
        }

        public void Closed(ServerSession session, string reason, LinkRefusedException? refusal)
        {
            bool rejected = refusal?.Status == 401;
            lock (_gate)
            {
                var now = _now();
                if (session == _pendingStandby || session == _standby)
                {
                    Ops.CxLog.Connection($"renewal session closed ({_server}): {reason}");
                    if (session == _pendingStandby) _pendingStandby = null;
                    if (session == _standby) _standby = null;
                    _standbyBackoff = Next(_standbyBackoff);
                    _nextStandbyAttempt = now + Jitter(_standbyBackoff);
                }
                else if (session == _pendingPrimary || session == _primary)
                {
                    bool wasPrimary = session == _primary;
                    Ops.CxLog.Connection($"session closed ({_server}): {reason}");
                    _pendingPrimary = null;
                    _primary = null;
                    if (_standby is { IsClosed: false, SessionId: not null })
                    {
                        Promote();
                    }
                    else
                    {
                        LoseSession();
                        // A session that lived long enough starts the backoff
                        // again; repeated failures double it to the cap.
                        if (wasPrimary && now - session.OpenedUtc >= _timings.HealthyAfter) _backoff = TimeSpan.Zero;
                        _backoff = Next(_backoff);
                        _nextAttempt = now + (refusal?.RetryAfter ??
                                              (refusal?.Status == 429 ? _timings.TooManyRequests : Jitter(_backoff)));
                        _linkProblem = rejected ? "the server refused the sign-in" : reason;
                        UpdateLink(_linkProblem);
                    }
                }
            }
            if (rejected) _tokens.Rejected();
            Notify();
        }

        // ---------------------------------------------------------------
        // Helpers (under the lock).
        // ---------------------------------------------------------------

        private void Promote()
        {
            var old = _primary;
            _primary = _standby;
            _standby = null;
            _standbyBackoff = TimeSpan.Zero;
            old?.Retire();
            Ops.CxLog.Connection($"renewal session took over ({_server})");
            _linkProblem = null;
            _linkChanged = true;
            UpdateLink(null);
        }

        /// The open session is gone: values stay, quality goes Stale (DX6).
        private void LoseSession()
        {
            foreach (var inst in _instruments.Values)
            {
                inst.Fresh = false;
                inst.Acked = false;
                if (inst.Sub is Subscription.Sent or Subscription.Live) inst.Sub = Subscription.Wanted;
                // The next session says again whether a result is too large.
                inst.TooLargeRuntime = null;
                if (inst.ProblemCode == "RESULT_TOO_LARGE") ClearProblem(inst);
                _changed.Add(inst.Id);
            }
            LoseBookSession();
            _linkChanged = true;
        }

        private void RetireAll()
        {
            foreach (var s in new[] { _pendingPrimary, _primary, _pendingStandby, _standby }) s?.Retire();
            _pendingPrimary = _primary = _pendingStandby = _standby = null;
        }

        private void UpdateLink(string? problem)
        {
            var link = _uri == null ? LinkState.NotConfigured
                : !_tokens.Ready ? LinkState.SignedOut
                : _primary != null ? LinkState.Connected
                : _hadSession || problem != null ? LinkState.Reconnecting
                : LinkState.Connecting;
            if (link != _link)
            {
                _link = link;
                _linkChanged = true;
                Ops.CxLog.Connection("link " + link + (problem != null ? ": " + problem : ""));
                foreach (var id in _instruments.Keys) _changed.Add(id);
            }
            _infoChanged = true;
        }

        private bool IsFresh(Inst inst) =>
            inst.Fresh && _primary != null && inst.Latest != null && inst.Latest.RuntimeId == _runtime;

        private bool Satisfied(Inst inst) =>
            inst.ProblemCode != null || inst.Sub is Subscription.Refused or Subscription.OverCap ||
            IsFresh(inst) || (inst.Acked && _primary != null);

        private void CompleteWaiters(Inst inst)
        {
            if (inst.Waiters.Count == 0 || !Satisfied(inst)) return;
            foreach (var waiter in inst.Waiters) waiter.TrySetResult(true);
        }

        private static void ClearProblem(Inst inst)
        {
            inst.ProblemCode = null;
            inst.ProblemMessage = null;
            inst.RetryAt = null;
        }

        private static string Refusal(string code, string message, string id) => code switch
        {
            "FORBIDDEN" => $"you are not entitled to the results of {id} (FORBIDDEN)",
            "ACCESS_QUOTA_EXCEEDED" => $"the server's subscription quota for your sign-in is used up; {id} is retried in a minute",
            "ACCESS_UNAVAILABLE" => $"the server could not check access to {id}; it is retried in a minute",
            _ => $"the server refused {id}: {code}" + (message.Length > 0 && message != code ? " (" + message + ")" : ""),
        };

        private TimeSpan Next(TimeSpan backoff) =>
            backoff == TimeSpan.Zero
                ? _timings.BackoffInitial
                : TimeSpan.FromTicks(Math.Min(_timings.BackoffMax.Ticks, backoff.Ticks * 2));

        /// Between half and all of `delay`, so many clients do not reconnect together.
        private TimeSpan Jitter(TimeSpan delay) =>
            TimeSpan.FromTicks((long)(delay.Ticks * (0.5 + 0.5 * _random.NextDouble())));

        private static IEnumerable<string[]> Chunks(IReadOnlyList<string> ids)
        {
            for (int i = 0; i < ids.Count; i += MaxInstruments)
                yield return ids.Skip(i).Take(MaxInstruments).ToArray();
        }

        private static string Seconds(TimeSpan t) => t.TotalSeconds.ToString("0.#", CultureInfo.InvariantCulture) + " s";

        private void Notify()
        {
            string[] ids, books;
            bool link, info;
            lock (_gate)
            {
                ids = _changed.Count == 0 ? Array.Empty<string>() : _changed.ToArray();
                _changed.Clear();
                books = _changedBooks.Count == 0 ? Array.Empty<string>() : _changedBooks.ToArray();
                _changedBooks.Clear();
                link = _linkChanged;
                info = _infoChanged;
                _linkChanged = _infoChanged = false;
            }
            try
            {
                if (ids.Length > 0) InstrumentsChanged?.Invoke(ids);
                if (books.Length > 0) BooksChanged?.Invoke(books);
                if (link || info) StatusChanged?.Invoke(link);
            }
            catch (Exception ex)
            {
                CxErrorStore.Record(ex, "(server)", "server link");
            }
        }
    }
}
