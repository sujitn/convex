using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;
using System.Net;
using System.Net.Sockets;
using System.Net.WebSockets;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;

namespace Convex.Excel.Tests
{
    /// An in-process stand-in for convex-server's WebSocket and resolve route,
    /// on HttpListener (no supported Kestrel on net472). It follows the real
    /// protocol: `connected` and heartbeats; a subscription sends each listed
    /// instrument's current result, then `subscribed`; a refused message gets
    /// one error; RESULT_TOO_LARGE uses the server's message template.
    internal sealed class FakeConvexServer : IDisposable
    {
        private readonly HttpListener _listener = new();
        private readonly CancellationTokenSource _stop = new();
        private readonly List<Session> _sessions = new();

        public int Port { get; }
        public string Url => $"http://localhost:{Port}";

        /// Each instrument's current `valuation` message.
        public ConcurrentDictionary<string, JObject> Current { get; } = new();
        /// A subscription listing one of these is refused with FORBIDDEN.
        public ConcurrentDictionary<string, byte> Forbidden { get; } = new();
        /// These are sent as RESULT_TOO_LARGE instead of a valuation.
        public ConcurrentDictionary<string, byte> TooLarge { get; } = new();
        /// CUSIP or ISIN → instrument id for the resolve route.
        public ConcurrentDictionary<string, string> Resolutions { get; } = new();
        /// Statuses the resolve route answers first, in order.
        public ConcurrentQueue<(int Status, int? RetryAfter)> ResolveScript { get; } = new();
        /// Statuses the WebSocket upgrade answers first, in order.
        public ConcurrentQueue<(int Status, int? RetryAfter)> UpgradeScript { get; } = new();
        /// When set, only these bearer tokens may connect.
        public HashSet<string>? AcceptTokens { get; set; }

        // ---- Limits (server P14b-P14d).
        /// Each book's status read body; a book missing here answers 404.
        public ConcurrentDictionary<string, JObject> LimitStatus { get; } = new();
        /// Statuses the status route answers first, in order.
        public ConcurrentQueue<(int Status, int? RetryAfter)> StatusScript { get; } = new();
        /// When set, a status read waits for it before answering (with the body current then).
        public TaskCompletionSource<bool>? StatusGate { get; set; }
        /// A subscribe_limits listing one of these is refused with FORBIDDEN.
        public ConcurrentDictionary<string, byte> ForbiddenBooks { get; } = new();
        /// Without a monitor, subscribe_limits is refused as the server refuses it.
        public volatile bool Monitored = true;
        /// Answer subscribe_limits with the valuations subscription type (a broken server).
        public volatile bool WrongLimitsReply;
        /// Before answering the next subscribe_valuations, send this book's
        /// stream FORBIDDEN (which answers no request).
        public string? ForbidBookBeforeNextReply { get; set; }
        /// Answers of the check route, by book; the request bodies arrive in CheckBodies.
        public ConcurrentDictionary<string, (int Status, JObject Body)> CheckAnswers { get; } = new();
        public ConcurrentQueue<(string Book, JObject Body)> CheckBodies { get; } = new();
        /// Status reads, by book, in order.
        public ConcurrentQueue<string> StatusReads { get; } = new();
        public volatile bool Heartbeats = true;
        public TimeSpan HeartbeatInterval { get; set; } = TimeSpan.FromMilliseconds(200);

        /// Each upgrade's Authorization and Origin headers.
        public ConcurrentQueue<(string? Authorization, string? Origin, DateTime At)> Upgrades { get; } = new();
        /// Every client message, with the session that sent it.
        public ConcurrentQueue<(int Session, JObject Message)> Received { get; } = new();
        public int ResolveCalls => _resolveCalls;
        private int _resolveCalls;
        private int _nextSession;

        public FakeConvexServer()
        {
            var probe = new TcpListener(IPAddress.Loopback, 0);
            probe.Start();
            Port = ((IPEndPoint)probe.LocalEndpoint).Port;
            probe.Stop();
            _listener.Prefixes.Add($"http://localhost:{Port}/");
            _listener.Start();
            _ = Task.Run(AcceptLoopAsync);
        }

        public int OpenSessions
        {
            get { lock (_sessions) return _sessions.Count(s => s.Socket.State == WebSocketState.Open); }
        }

        public int SessionsOpened => Volatile.Read(ref _nextSession);

        /// Subscribe messages received, as instrument id lists.
        public List<string[]> Subscribes(int? session = null) => Received
            .Where(r => (session == null || r.Session == session) && (string?)r.Message["type"] == "subscribe_valuations")
            .Select(r => r.Message["instrument_ids"]!.Select(t => (string)t!).ToArray()).ToList();

        public List<string[]> Unsubscribes() => Received
            .Where(r => (string?)r.Message["type"] == "unsubscribe_valuations")
            .Select(r => r.Message["instrument_ids"]!.Select(t => (string)t!).ToArray()).ToList();

        public List<string[]> LimitSubscribes(int? session = null) => Received
            .Where(r => (session == null || r.Session == session) && (string?)r.Message["type"] == "subscribe_limits")
            .Select(r => r.Message["book_ids"]!.Select(t => (string)t!).ToArray()).ToList();

        public List<string[]> LimitUnsubscribes() => Received
            .Where(r => (string?)r.Message["type"] == "unsubscribe_limits")
            .Select(r => r.Message["book_ids"]!.Select(t => (string)t!).ToArray()).ToList();

        /// Stream a recorded limit status change to the book's followers.
        public void PublishLimit(JObject change)
        {
            var book = (string)change["book_id"]!;
            foreach (var session in Snapshot())
                if (session.FollowsBook(book)) _ = session.SendAsync(change);
        }

        /// The stream FORBIDDEN a follower of `book` gets when it may not read a change.
        public void ForbidBook(string book)
        {
            foreach (var session in Snapshot())
                if (session.FollowsBook(book)) _ = session.SendAsync(BookForbidden(book));
        }

        private static JObject BookForbidden(string book) => Error("FORBIDDEN",
            $"limit status of book {book} needs read access to each instrument it holds or a limit names");

        public const string LimitsResyncText = "Read /api/v1/books/{book_id}/limits/status for each subscribed book";
        public const string ValuationsResyncText =
            "Subscribe again to receive each instrument's current result, or read /api/v1/runtime/results; keep the newer by (runtime_id, revision)";

        /// Set an instrument's current result and stream it to its followers.
        public void Publish(JObject valuation)
        {
            var id = (string)valuation["result"]!["provenance"]!["subject"]!["instrument_id"]!;
            Current[id] = valuation;
            foreach (var session in Snapshot())
                if (session.Follows(id)) _ = session.SendAsync(Frame(id, valuation));
        }

        public void SendAll(JObject message)
        {
            foreach (var session in Snapshot()) _ = session.SendAsync(message);
        }

        /// Drop every session, as a restart or network loss would.
        public void DropAll()
        {
            foreach (var session in Snapshot()) session.Socket.Abort();
        }

        private Session[] Snapshot()
        {
            lock (_sessions) return _sessions.Where(s => s.Socket.State == WebSocketState.Open).ToArray();
        }

        private JObject Frame(string id, JObject valuation)
        {
            if (!TooLarge.ContainsKey(id)) return valuation;
            return new JObject
            {
                ["type"] = "error",
                ["code"] = "RESULT_TOO_LARGE",
                ["message"] = $"The result of instrument {id} (runtime {(string)valuation["runtime_id"]!}, revision " +
                              $"{(ulong)valuation["revision"]!}) exceeds the 65536-byte message limit; read it from /api/v1/runtime/results",
            };
        }

        /// A `valuation` message shaped like the server's.
        public static JObject Valuation(string instrument, string runtime, ulong revision,
            IDictionary<string, object> measures, string status = "Complete", string quality = "Valid",
            JObject? lastGood = null, string? retainedQuality = null)
        {
            var result = Envelope(instrument, measures, status, quality);
            return new JObject
            {
                ["type"] = "valuation",
                ["runtime_id"] = runtime,
                ["revision"] = revision,
                ["generation"] = 1,
                ["context_key"] = "ctx",
                ["result"] = result,
                ["last_good"] = lastGood,
                ["retained_quality"] = retainedQuality,
            };
        }

        public static JObject Envelope(string instrument, IDictionary<string, object> measures,
            string status = "Complete", string quality = "Valid", JArray? errors = null)
        {
            var results = new JObject();
            foreach (var m in measures)
                results[m.Key] = new JObject
                {
                    ["value"] = new JObject { ["value"] = JToken.FromObject(m.Value), ["unit"] = "price_per_100" },
                    ["quality"] = quality,
                    ["method"] = "model",
                    ["source_observation_id"] = null,
                };
            return new JObject
            {
                ["schema_version"] = "2.0",
                ["request_id"] = "req-1",
                ["status"] = status,
                ["quality"] = quality,
                ["snapshot_id"] = "snap-7",
                ["configuration_version"] = 3,
                ["results"] = results,
                ["errors"] = errors ?? new JArray(),
                ["provenance"] = new JObject
                {
                    ["subject"] = new JObject { ["kind"] = "instrument_version", ["instrument_id"] = instrument, ["version"] = 1 },
                    ["snapshot_id"] = "snap-7",
                    ["configuration_version"] = 3,
                    ["valuation_time"] = "2026-11-07T10:00:00Z",
                    ["known_at"] = "2026-11-07T10:00:00Z",
                    ["calculated_at"] = "2026-11-07T10:00:01.123456789Z",
                    ["model_version"] = "m1",
                    ["build_id"] = "b1",
                    ["input_versions"] = new JArray(),
                },
            };
        }

        /// One limit's evaluation, as a status item or inside a change.
        public static JObject LimitEval(string limitId, string status, string? value, string? upper = "1000000",
            string? utilisation = null, string measure = "dv01", string? member = null, JObject? reason = null)
        {
            var o = new JObject
            {
                ["measure"] = new JObject { ["kind"] = measure },
                ["limit_id"] = limitId,
                ["status"] = status,
            };
            if (upper != null) o["upper"] = upper;
            if (value != null) o["value"] = value;
            if (member != null) o["member"] = member;
            if (utilisation != null) o["utilisation"] = utilisation;
            if (reason != null) o["reason"] = reason;
            return o;
        }

        /// A status read body.
        public static JObject LimitStatusBody(string book, ulong bookVersion, ulong setVersion, string evaluatedAt,
            params JObject[] limits) => new()
        {
            ["book_id"] = book,
            ["book_version"] = bookVersion,
            ["limit_set_version"] = setVersion,
            ["evaluated_at"] = evaluatedAt,
            ["snapshot_ids"] = new JArray("snap-7"),
            ["limits"] = new JArray(limits.Cast<object>().ToArray()),
        };

        /// A `limit_status` message: a recorded change of one limit.
        public static JObject LimitChange(string book, ulong recordId, string previous, JObject eval,
            string evaluatedAt, ulong setVersion = 1, ulong bookVersion = 1)
        {
            var o = new JObject
            {
                ["type"] = "limit_status",
                ["record_id"] = recordId,
                ["book_id"] = book,
                ["book_version"] = bookVersion,
                ["limit_set_version"] = setVersion,
                ["evaluated_at"] = evaluatedAt,
                ["monitor_started_at"] = "2026-11-09T08:00:00Z",
                ["previous"] = previous,
                ["snapshot_ids"] = new JArray("snap-7"),
            };
            foreach (var p in eval.Properties()) o[p.Name] = p.Value.DeepClone();
            return o;
        }

        public void Dispose()
        {
            _stop.Cancel();
            DropAll();
            try { _listener.Stop(); } catch { /* already stopped */ }
            try { _listener.Close(); } catch { /* already closed */ }
        }

        private async Task AcceptLoopAsync()
        {
            while (!_stop.IsCancellationRequested)
            {
                HttpListenerContext context;
                try
                {
                    context = await _listener.GetContextAsync().ConfigureAwait(false);
                }
                catch
                {
                    return;
                }
                _ = Task.Run(() => HandleAsync(context));
            }
        }

        private async Task HandleAsync(HttpListenerContext context)
        {
            try
            {
                var path = context.Request.Url!.AbsolutePath;
                if (path == "/api/v1/ws") await WebSocketAsync(context).ConfigureAwait(false);
                else if (path == "/api/v1/reference/resolve") await ResolveAsync(context).ConfigureAwait(false);
                else if (path.StartsWith("/api/v1/books/", StringComparison.Ordinal) && path.EndsWith("/limits/status", StringComparison.Ordinal))
                    await StatusAsync(context, Uri.UnescapeDataString(path.Substring(14, path.Length - 14 - 14))).ConfigureAwait(false);
                else if (path.StartsWith("/api/v1/books/", StringComparison.Ordinal) && path.EndsWith("/limits/check", StringComparison.Ordinal))
                    await CheckAsync(context, Uri.UnescapeDataString(path.Substring(14, path.Length - 14 - 13))).ConfigureAwait(false);
                else Respond(context, 404, "{}", null);
            }
            catch
            {
                // A test ended mid-request.
            }
        }

        private async Task WebSocketAsync(HttpListenerContext context)
        {
            var authorization = context.Request.Headers["Authorization"];
            Upgrades.Enqueue((authorization, context.Request.Headers["Origin"], DateTime.UtcNow));
            if (UpgradeScript.TryDequeue(out var scripted))
            {
                Respond(context, scripted.Status, "{}", scripted.RetryAfter);
                return;
            }
            var token = authorization?.StartsWith("Bearer ", StringComparison.Ordinal) == true
                ? authorization.Substring(7)
                : null;
            if (AcceptTokens != null && (token == null || !AcceptTokens.Contains(token)))
            {
                Respond(context, 401, "{\"error\":\"UNAUTHENTICATED\"}", null);
                return;
            }
            var accepted = await context.AcceptWebSocketAsync(null).ConfigureAwait(false);
            var session = new Session(Interlocked.Increment(ref _nextSession), accepted.WebSocket);
            lock (_sessions) _sessions.Add(session);
            await session.SendAsync(new JObject
            {
                ["type"] = "connected",
                ["session_id"] = "ws-" + session.Id.ToString(CultureInfo.InvariantCulture),
                ["server_time"] = 0,
                ["profile"] = "demo",
                ["data_quality"] = "demo",
            }).ConfigureAwait(false);
            _ = Task.Run(() => HeartbeatAsync(session));
            var buffer = new byte[64 * 1024];
            while (session.Socket.State == WebSocketState.Open && !_stop.IsCancellationRequested)
            {
                WebSocketReceiveResult received;
                try
                {
                    received = await session.Socket.ReceiveAsync(new ArraySegment<byte>(buffer), _stop.Token).ConfigureAwait(false);
                }
                catch
                {
                    break;
                }
                if (received.MessageType == WebSocketMessageType.Close) break;
                var message = JObject.Parse(Encoding.UTF8.GetString(buffer, 0, received.Count));
                Received.Enqueue((session.Id, message));
                await OnMessageAsync(session, message).ConfigureAwait(false);
            }
        }

        private async Task OnMessageAsync(Session session, JObject message)
        {
            var ids = (message["instrument_ids"] as JArray ?? new JArray()).Select(t => (string)t!).ToArray();
            var books = (message["book_ids"] as JArray ?? new JArray()).Select(t => (string)t!).ToArray();
            switch ((string?)message["type"])
            {
                case "subscribe_limits":
                    if (!Monitored)
                    {
                        await session.SendAsync(Error("INVALID_MESSAGE", "this server does not monitor limits")).ConfigureAwait(false);
                        return;
                    }
                    if (books.Any(ForbiddenBooks.ContainsKey))
                    {
                        await session.SendAsync(Error("FORBIDDEN", "FORBIDDEN")).ConfigureAwait(false);
                        return;
                    }
                    if (session.BookCount(books) > 64)
                    {
                        await session.SendAsync(Error("INVALID_MESSAGE", "at most 64 limit subscriptions")).ConfigureAwait(false);
                        return;
                    }
                    session.AddBooks(books);
                    await session.SendAsync(new JObject
                    {
                        ["type"] = "subscribed",
                        ["subscription_type"] = WrongLimitsReply ? "valuations" : "limits",
                        ["count"] = books.Distinct().Count(),
                    }).ConfigureAwait(false);
                    break;
                case "unsubscribe_limits":
                    session.RemoveBooks(books);
                    await session.SendAsync(new JObject
                    {
                        ["type"] = "unsubscribed", ["subscription_type"] = "limits", ["count"] = books.Length,
                    }).ConfigureAwait(false);
                    break;
                case "subscribe_valuations":
                    if (ForbidBookBeforeNextReply is { } forbid)
                    {
                        ForbidBookBeforeNextReply = null;
                        await session.SendAsync(BookForbidden(forbid)).ConfigureAwait(false);
                    }
                    if (ids.Any(Forbidden.ContainsKey))
                    {
                        await session.SendAsync(Error("FORBIDDEN", "FORBIDDEN")).ConfigureAwait(false);
                        return;
                    }
                    session.Add(ids);
                    foreach (var id in ids.Distinct())
                        if (Current.TryGetValue(id, out var valuation))
                            await session.SendAsync(Frame(id, valuation)).ConfigureAwait(false);
                    await session.SendAsync(new JObject
                    {
                        ["type"] = "subscribed",
                        ["subscription_type"] = "valuations",
                        ["count"] = ids.Distinct().Count(),
                    }).ConfigureAwait(false);
                    break;
                case "unsubscribe_valuations":
                    session.Remove(ids);
                    await session.SendAsync(new JObject
                    {
                        ["type"] = "unsubscribed", ["subscription_type"] = "valuations", ["count"] = ids.Length,
                    }).ConfigureAwait(false);
                    break;
                default:
                    await session.SendAsync(Error("INVALID_MESSAGE", "unknown message")).ConfigureAwait(false);
                    break;
            }
        }

        private async Task HeartbeatAsync(Session session)
        {
            while (session.Socket.State == WebSocketState.Open && !_stop.IsCancellationRequested)
            {
                try
                {
                    await Task.Delay(HeartbeatInterval, _stop.Token).ConfigureAwait(false);
                }
                catch
                {
                    return;
                }
                if (Heartbeats) await session.SendAsync(new JObject { ["type"] = "heartbeat", ["server_time"] = 0 }).ConfigureAwait(false);
            }
        }

        private async Task ResolveAsync(HttpListenerContext context)
        {
            Interlocked.Increment(ref _resolveCalls);
            string body;
            using (var reader = new System.IO.StreamReader(context.Request.InputStream, Encoding.UTF8)) body = await reader.ReadToEndAsync().ConfigureAwait(false);
            if (ResolveScript.TryDequeue(out var scripted))
            {
                Respond(context, scripted.Status, "{\"error\":\"scripted\",\"code\":\"SCRIPTED\"}", scripted.RetryAfter);
                return;
            }
            var request = JObject.Parse(body);
            var value = (string?)request["identifier"]?["value"] ?? "";
            if (request["lookup"]?["as_of"] == null || request["lookup"]?["known_at"] == null)
            {
                Respond(context, 400, "{\"error\":\"lookup is required\",\"code\":\"INVALID_INPUT\"}", null);
                return;
            }
            if (!Resolutions.TryGetValue(value, out var id))
            {
                Respond(context, 404, "{\"error\":\"not found\",\"code\":\"NOT_FOUND\"}", null);
                return;
            }
            Respond(context, 200, new JObject
            {
                ["mapping"] = new JObject(),
                ["target"] = new JObject { ["kind"] = "instrument", ["instrument_id"] = id },
                ["target_version"] = 1,
            }.ToString(Formatting.None), null);
        }

        private async Task StatusAsync(HttpListenerContext context, string book)
        {
            StatusReads.Enqueue(book);
            if (StatusGate is { } gate) await gate.Task.ConfigureAwait(false);
            if (StatusScript.TryDequeue(out var scripted))
            {
                Respond(context, scripted.Status, "{\"error\":\"scripted\",\"code\":\"SCRIPTED\"}", scripted.RetryAfter);
                return;
            }
            if (!LimitStatus.TryGetValue(book, out var body))
            {
                Respond(context, 404, "{\"error\":\"reference record not found\",\"code\":\"NOT_FOUND\"}", null);
                return;
            }
            context.Response.Headers["x-convex-data-quality"] = "demo";
            Respond(context, 200, body.ToString(Formatting.None), null);
        }

        private async Task CheckAsync(HttpListenerContext context, string book)
        {
            string text;
            using (var reader = new System.IO.StreamReader(context.Request.InputStream, Encoding.UTF8)) text = await reader.ReadToEndAsync().ConfigureAwait(false);
            CheckBodies.Enqueue((book, JObject.Parse(text)));
            if (CheckAnswers.TryGetValue(book, out var answer))
            {
                if (answer.Status == 200) context.Response.Headers["x-convex-data-quality"] = "demo";
                Respond(context, answer.Status, answer.Body.ToString(Formatting.None), null);
                return;
            }
            Respond(context, 404, "{\"error\":\"reference record not found\",\"code\":\"NOT_FOUND\"}", null);
        }

        private static void Respond(HttpListenerContext context, int status, string body, int? retryAfter)
        {
            context.Response.StatusCode = status;
            if (retryAfter is { } s) context.Response.Headers["Retry-After"] = s.ToString(CultureInfo.InvariantCulture);
            var bytes = Encoding.UTF8.GetBytes(body);
            context.Response.ContentType = "application/json";
            context.Response.ContentLength64 = bytes.Length;
            context.Response.OutputStream.Write(bytes, 0, bytes.Length);
            context.Response.Close();
        }

        private static JObject Error(string code, string message) =>
            new() { ["type"] = "error", ["code"] = code, ["message"] = message };

        private sealed class Session
        {
            private readonly SemaphoreSlim _send = new(1);
            private readonly HashSet<string> _follows = new(StringComparer.Ordinal);

            public Session(int id, WebSocket socket)
            {
                Id = id;
                Socket = socket;
            }

            public int Id { get; }
            public WebSocket Socket { get; }

            public bool Follows(string id)
            {
                lock (_follows) return _follows.Contains(id);
            }

            public void Add(IEnumerable<string> ids)
            {
                lock (_follows) _follows.UnionWith(ids);
            }

            public void Remove(IEnumerable<string> ids)
            {
                lock (_follows) _follows.ExceptWith(ids);
            }

            private readonly HashSet<string> _books = new(StringComparer.Ordinal);

            public bool FollowsBook(string id)
            {
                lock (_books) return _books.Contains(id);
            }

            /// Books followed if `ids` were added (distinct, as the server counts them).
            public int BookCount(IEnumerable<string> ids)
            {
                lock (_books) return _books.Union(ids).Count();
            }

            public void AddBooks(IEnumerable<string> ids)
            {
                lock (_books) _books.UnionWith(ids);
            }

            public void RemoveBooks(IEnumerable<string> ids)
            {
                lock (_books) _books.ExceptWith(ids);
            }

            public async Task SendAsync(JObject message)
            {
                var bytes = Encoding.UTF8.GetBytes(message.ToString(Formatting.None));
                await _send.WaitAsync().ConfigureAwait(false);
                try
                {
                    if (Socket.State == WebSocketState.Open)
                        await Socket.SendAsync(new ArraySegment<byte>(bytes), WebSocketMessageType.Text, true, CancellationToken.None)
                            .ConfigureAwait(false);
                }
                catch
                {
                    // The client went away.
                }
                finally
                {
                    _send.Release();
                }
            }
        }
    }
}
