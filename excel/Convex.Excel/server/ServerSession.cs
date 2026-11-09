using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Net;
using System.Net.WebSockets;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;

namespace Convex.Excel.Server
{
    /// The transport seam (DX7): the add-in holds the connection in process
    /// today; an out-of-process connector would implement this instead.
    internal interface ILiveTransport
    {
        /// Open a WebSocket to `uri`, sending `bearer` as the Authorization
        /// header when it is not null. Throws LinkRefusedException when the
        /// server answers the upgrade with an HTTP error.
        Task<ILiveConnection> ConnectAsync(Uri uri, string? bearer, CancellationToken ct);
    }

    /// One open WebSocket carrying text messages.
    internal interface ILiveConnection : IDisposable
    {
        Task SendAsync(string text, CancellationToken ct);

        /// The next text message, or null once the server has closed.
        Task<string?> ReceiveAsync(CancellationToken ct);

        /// Drop the connection at once.
        void Abort();

        /// Close politely, waiting briefly for the server.
        Task CloseAsync();
    }

    /// The server refused a connection: its HTTP status and any Retry-After.
    internal sealed class LinkRefusedException : Exception
    {
        public int? Status { get; }
        public TimeSpan? RetryAfter { get; }

        public LinkRefusedException(string message, int? status, TimeSpan? retryAfter, Exception? inner = null)
            : base(message, inner)
        {
            Status = status;
            RetryAfter = retryAfter;
        }
    }

    /// ClientWebSocket on net472. Keep-alive frames are off: the server counts
    /// every inbound frame against the identity's admission window, and its
    /// own 30 s heartbeat keeps the connection busy. No Origin header is sent
    /// (the server refuses an Origin it does not list).
    internal sealed class ClientWebSocketTransport : ILiveTransport
    {
        /// The server sends at most 64 KiB; anything larger is not ours.
        public const int MaxMessageBytes = 1 << 20;

        public async Task<ILiveConnection> ConnectAsync(Uri uri, string? bearer, CancellationToken ct)
        {
            var socket = new ClientWebSocket();
            socket.Options.KeepAliveInterval = TimeSpan.Zero;
            if (bearer != null) socket.Options.SetRequestHeader("Authorization", "Bearer " + bearer);
            try
            {
                await socket.ConnectAsync(uri, ct).ConfigureAwait(false);
            }
            catch (Exception ex) when (ex is WebSocketException or WebException)
            {
                socket.Dispose();
                throw Refusal(ex);
            }
            catch
            {
                socket.Dispose();
                throw;
            }
            return new Connection(socket);
        }

        /// The HTTP status and Retry-After behind a failed upgrade, when the
        /// framework exposes them (net472 wraps a WebException).
        internal static LinkRefusedException Refusal(Exception ex)
        {
            for (var e = ex; e != null; e = e.InnerException)
            {
                if (e is WebException { Response: HttpWebResponse response })
                {
                    TimeSpan? retry = null;
                    var header = response.Headers["Retry-After"];
                    if (header != null && int.TryParse(header, NumberStyles.None, CultureInfo.InvariantCulture, out var s))
                        retry = TimeSpan.FromSeconds(s);
                    return new LinkRefusedException(
                        $"the server refused the connection (HTTP {(int)response.StatusCode})",
                        (int)response.StatusCode, retry, ex);
                }
            }
            // Some framework builds only put the status in the message text.
            var text = ex.ToString();
            foreach (var status in new[] { 401, 403, 429, 503 })
                if (text.Contains("'" + status.ToString(CultureInfo.InvariantCulture) + "'"))
                    return new LinkRefusedException($"the server refused the connection (HTTP {status})", status, null, ex);
            return new LinkRefusedException("could not reach the server: " + ex.Message, null, null, ex);
        }

        private sealed class Connection : ILiveConnection
        {
            private readonly ClientWebSocket _socket;
            private readonly byte[] _buffer = new byte[16 * 1024];

            public Connection(ClientWebSocket socket) => _socket = socket;

            public Task SendAsync(string text, CancellationToken ct) =>
                _socket.SendAsync(new ArraySegment<byte>(Encoding.UTF8.GetBytes(text)),
                    WebSocketMessageType.Text, true, ct);

            public async Task<string?> ReceiveAsync(CancellationToken ct)
            {
                using var message = new MemoryStream();
                while (true)
                {
                    var result = await _socket.ReceiveAsync(new ArraySegment<byte>(_buffer), ct).ConfigureAwait(false);
                    if (result.MessageType == WebSocketMessageType.Close) return null;
                    message.Write(_buffer, 0, result.Count);
                    if (message.Length > MaxMessageBytes)
                        throw new IOException($"a server message exceeded {MaxMessageBytes} bytes");
                    if (result.EndOfMessage) return Encoding.UTF8.GetString(message.GetBuffer(), 0, (int)message.Length);
                }
            }

            public void Abort() => _socket.Abort();

            public async Task CloseAsync()
            {
                try
                {
                    using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(2));
                    await _socket.CloseOutputAsync(WebSocketCloseStatus.NormalClosure, "", timeout.Token).ConfigureAwait(false);
                }
                catch
                {
                    _socket.Abort();
                }
            }

            public void Dispose() => _socket.Dispose();
        }
    }

    /// What a session reports to its owner (ServerSource). Called on the
    /// session's receive thread, one message at a time.
    internal interface ISessionEvents
    {
        void Connected(ServerSession session, string sessionId);
        void Valuation(ServerSession session, Valuation valuation);
        void Subscribed(ServerSession session, string[] ids);
        void SubscribeRefused(ServerSession session, string[] ids, string code, string message);
        void LimitsSubscribed(ServerSession session, string[] bookIds);
        void LimitsRefused(ServerSession session, string[] bookIds, string code, string message);
        /// A recorded change of one limit's status.
        void LimitStatus(ServerSession session, string bookId, LimitItem item);
        /// A followed book changed and the session may not read it.
        void LimitForbidden(ServerSession session, string bookId);
        /// RESYNC_REQUIRED, RESULT_TOO_LARGE, or an error that answers nothing.
        void StreamError(ServerSession session, string code, string message);
        void Closed(ServerSession session, string reason, LinkRefusedException? refusal);
    }

    /// One WebSocket session with convex-server. It sends subscription
    /// changes in order and matches each reply to its request: the server
    /// answers every client message, in order, with exactly one of
    /// `subscribed`, `unsubscribed` or an `error`. RESYNC_REQUIRED and
    /// RESULT_TOO_LARGE are stream events, not replies (RESULT_TOO_LARGE can
    /// arrive inside a subscription's seed, before its `subscribed`), and so
    /// is a FORBIDDEN naming a followed book (DX19). A reply whose
    /// subscription type is not its request's ends the session.
    internal sealed class ServerSession
    {
        private enum RequestKind { Subscribe, Unsubscribe, SubscribeLimits, UnsubscribeLimits }

        private sealed class Request
        {
            public readonly RequestKind Kind;
            public readonly string[] Ids;
            public Request(RequestKind kind, string[] ids)
            {
                Kind = kind;
                Ids = ids;
            }
        }

        private static readonly HashSet<string> StreamCodes = new(StringComparer.Ordinal)
        {
            "RESYNC_REQUIRED", "RESULT_TOO_LARGE",
        };

        private readonly ILiveTransport _transport;
        private readonly Uri _uri;
        private readonly ISessionEvents _events;
        private readonly CancellationTokenSource _stop = new();
        private readonly object _lock = new();
        private readonly Queue<Request> _pending = new();
        private readonly ConcurrentQueue<string> _outbox = new();
        private readonly SemaphoreSlim _outboxSignal = new(0);
        private ILiveConnection? _connection;
        private long _lastMessageTicks;
        private int _closed;

        public ServerSession(ILiveTransport transport, Uri uri, string? token, ISessionEvents events)
        {
            _transport = transport;
            _uri = uri;
            Token = token;
            _events = events;
            OpenedUtc = DateTime.UtcNow;
            _lastMessageTicks = OpenedUtc.Ticks;
        }

        /// The token this session was opened with.
        public string? Token { get; }
        public DateTime OpenedUtc { get; }
        /// Set by the server's `connected` message.
        public string? SessionId { get; private set; }
        public DateTime LastMessageUtc => new(Interlocked.Read(ref _lastMessageTicks), DateTimeKind.Utc);
        public bool IsClosed => Volatile.Read(ref _closed) != 0;

        /// Subscriptions (valuations or limits) sent and not yet answered.
        public int PendingSubscribes
        {
            get { lock (_lock) return _pending.Count(r => r.Kind is RequestKind.Subscribe or RequestKind.SubscribeLimits); }
        }

        /// Connect, then read until the server or Abort ends the session.
        public void Start(TimeSpan connectTimeout) => Task.Run(() => RunAsync(connectTimeout));

        public void Subscribe(IReadOnlyCollection<string> ids) => Send(RequestKind.Subscribe, ids);

        public void Unsubscribe(IReadOnlyCollection<string> ids) => Send(RequestKind.Unsubscribe, ids);

        public void SubscribeLimits(IReadOnlyCollection<string> bookIds) => Send(RequestKind.SubscribeLimits, bookIds);

        public void UnsubscribeLimits(IReadOnlyCollection<string> bookIds) => Send(RequestKind.UnsubscribeLimits, bookIds);

        private void Send(RequestKind kind, IReadOnlyCollection<string> ids)
        {
            if (ids.Count == 0 || IsClosed) return;
            var array = ids.ToArray();
            bool limits = kind is RequestKind.SubscribeLimits or RequestKind.UnsubscribeLimits;
            var json = new JObject
            {
                ["type"] = kind switch
                {
                    RequestKind.Subscribe => "subscribe_valuations",
                    RequestKind.Unsubscribe => "unsubscribe_valuations",
                    RequestKind.SubscribeLimits => "subscribe_limits",
                    _ => "unsubscribe_limits",
                },
                [limits ? "book_ids" : "instrument_ids"] = new JArray(array),
            }.ToString(Formatting.None);
            // The pending queue and the outbox are filled in the same order,
            // and one loop sends the outbox, so replies match by position.
            lock (_lock)
            {
                _pending.Enqueue(new Request(kind, array));
                _outbox.Enqueue(json);
            }
            _outboxSignal.Release();
        }

        /// End the session now; Closed reports `reason` on another thread, so
        /// a caller holding its own lock is not re-entered.
        public void Abort(string reason)
        {
            if (!MarkClosed()) return;
            _stop.Cancel();
            try { _connection?.Abort(); } catch { /* already gone */ }
            _ = Task.Run(() => _events.Closed(this, reason, null));
        }

        /// Close politely (a superseded session); Closed is not reported.
        public void Retire()
        {
            if (!MarkClosed()) return;
            var connection = _connection;
            _ = Task.Run(async () =>
            {
                if (connection != null) await connection.CloseAsync().ConfigureAwait(false);
                _stop.Cancel();
                connection?.Dispose();
            });
        }

        private bool MarkClosed() => Interlocked.Exchange(ref _closed, 1) == 0;

        private async Task RunAsync(TimeSpan connectTimeout)
        {
            try
            {
                using (var timeout = CancellationTokenSource.CreateLinkedTokenSource(_stop.Token))
                {
                    timeout.CancelAfter(connectTimeout);
                    _connection = await _transport.ConnectAsync(_uri, Token, timeout.Token).ConfigureAwait(false);
                }
                if (IsClosed)
                {
                    _connection.Abort();
                    return;
                }
                _ = Task.Run(SendLoopAsync);
                while (!_stop.IsCancellationRequested)
                {
                    var text = await _connection.ReceiveAsync(_stop.Token).ConfigureAwait(false);
                    if (text == null)
                    {
                        Fail("the server closed the connection", null);
                        return;
                    }
                    Interlocked.Exchange(ref _lastMessageTicks, DateTime.UtcNow.Ticks);
                    Handle(text);
                }
            }
            catch (LinkRefusedException refusal)
            {
                Fail(refusal.Message, refusal);
            }
            catch (OperationCanceledException) when (!IsClosed)
            {
                Fail("the server did not answer within " +
                     connectTimeout.TotalSeconds.ToString("0", CultureInfo.InvariantCulture) + " s", null);
            }
            catch (Exception ex)
            {
                if (!IsClosed) Fail("connection lost: " + Cause(ex), null);
            }
        }

        private async Task SendLoopAsync()
        {
            try
            {
                while (!_stop.IsCancellationRequested)
                {
                    await _outboxSignal.WaitAsync(_stop.Token).ConfigureAwait(false);
                    if (!_outbox.TryDequeue(out var json)) continue;
                    using var timeout = CancellationTokenSource.CreateLinkedTokenSource(_stop.Token);
                    timeout.CancelAfter(TimeSpan.FromSeconds(10));
                    await _connection!.SendAsync(json, timeout.Token).ConfigureAwait(false);
                }
            }
            catch (Exception ex)
            {
                if (!IsClosed) Fail("could not send to the server: " + Cause(ex), null);
            }
        }

        private void Fail(string reason, LinkRefusedException? refusal)
        {
            if (!MarkClosed()) return;
            _stop.Cancel();
            try { _connection?.Abort(); } catch { /* already gone */ }
            _events.Closed(this, reason, refusal);
        }

        /// The innermost exception's message: ClientWebSocket wraps the
        /// socket's own reason in a generic "internal WebSocket error".
        private static string Cause(Exception ex)
        {
            while (ex.InnerException != null) ex = ex.InnerException;
            return ex.Message.TrimEnd(' ', '.');
        }

        private void Handle(string text)
        {
            JObject message;
            try
            {
                message = ServerJson.Parse(text);
            }
            catch (JsonException)
            {
                return; // not a message of ours; the session goes on
            }
            switch (ServerJson.Text(message["type"]))
            {
                case "connected":
                    SessionId = ServerJson.Text(message["session_id"]) ?? "?";
                    _events.Connected(this, SessionId);
                    break;
                case "valuation":
                    var valuation = Valuation.Parse(message);
                    if (valuation != null) _events.Valuation(this, valuation);
                    break;
                case "subscribed":
                case "unsubscribed":
                    bool subscribe = ServerJson.Text(message["type"]) == "subscribed";
                    bool limits = ServerJson.Text(message["subscription_type"]) == "limits";
                    var expected = (subscribe, limits) switch
                    {
                        (true, false) => RequestKind.Subscribe,
                        (false, false) => RequestKind.Unsubscribe,
                        (true, true) => RequestKind.SubscribeLimits,
                        _ => RequestKind.UnsubscribeLimits,
                    };
                    var reply = Dequeue();
                    if (reply == null || reply.Kind != expected)
                    {
                        // Every later reply would be matched to the wrong request.
                        Fail("the server's replies no longer match the requests", null);
                        return;
                    }
                    if (expected == RequestKind.Subscribe) _events.Subscribed(this, reply.Ids);
                    else if (expected == RequestKind.SubscribeLimits) _events.LimitsSubscribed(this, reply.Ids);
                    break;
                case "limit_status":
                    if (LimitItem.ParseChange(message) is { } change) _events.LimitStatus(this, change.BookId, change.Item);
                    break;
                case "error":
                    var code = ServerJson.Text(message["code"]) ?? "";
                    var text2 = ServerJson.Text(message["message"]) ?? "";
                    if (StreamCodes.Contains(code))
                    {
                        _events.StreamError(this, code, text2);
                        break;
                    }
                    if (code == "FORBIDDEN" && LimitErrors.TryParseForbiddenBook(text2, out var book))
                    {
                        _events.LimitForbidden(this, book);
                        break;
                    }
                    var answered = Dequeue();
                    if (answered is { Kind: RequestKind.Subscribe })
                        _events.SubscribeRefused(this, answered.Ids, code, text2);
                    else if (answered is { Kind: RequestKind.SubscribeLimits })
                        _events.LimitsRefused(this, answered.Ids, code, text2);
                    else if (answered == null)
                        _events.StreamError(this, code, text2);
                    break;
                // heartbeat and pong only refresh the watchdog.
            }
        }

        private Request? Dequeue()
        {
            lock (_lock) return _pending.Count > 0 ? _pending.Dequeue() : null;
        }
    }
}
