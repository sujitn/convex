using System;
using System.Collections.Generic;
using System.Globalization;
using System.Net;
using System.Net.Http;
using System.Net.Http.Headers;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;

namespace Convex.Excel.Server
{
    /// CUSIP and ISIN → server instrument id, through
    /// POST /api/v1/reference/resolve (DX16), cached for the process.
    ///
    /// - At most 8 lookups are in flight, and identical lookups share one call.
    /// - A 429 waits for its Retry-After; a server or network failure retries
    ///   with backoff. HTTP calls and WebSocket messages share one admission
    ///   window per sign-in, so the add-in never hammers the route.
    /// - Successes are kept (10,000, least recently used first out); a
    ///   definite failure (unknown identifier, no reference access) is kept
    ///   for 5 minutes, so a sheet of bad identifiers costs one call each.
    internal sealed class IdentifierResolver
    {
        public const int CacheSize = 10_000;
        public const int MaxInFlight = 8;
        private static readonly TimeSpan FailureLifetime = TimeSpan.FromMinutes(5);

        private readonly ServerSource _source;
        private readonly HttpClient _http;
        private readonly ServerTimings _timings;
        private readonly Func<DateTime> _now;
        private readonly SemaphoreSlim _slots = new(MaxInFlight);
        private readonly object _lock = new();
        private readonly Dictionary<string, LinkedListNode<(string Key, string Id)>> _hits = new(StringComparer.Ordinal);
        private readonly LinkedList<(string Key, string Id)> _order = new();
        private readonly Dictionary<string, (Exception Error, DateTime Until)> _failures = new(StringComparer.Ordinal);
        private readonly Dictionary<string, Task<string>> _inFlight = new(StringComparer.Ordinal);
        private long _calls;

        public IdentifierResolver(ServerSource source, HttpClient http, ServerTimings? timings = null, Func<DateTime>? utcNow = null)
        {
            _source = source;
            _http = http;
            _timings = timings ?? new ServerTimings();
            _now = utcNow ?? (() => DateTime.UtcNow);
        }

        /// HTTP lookups made so far (diagnostics and tests).
        public long Calls => Interlocked.Read(ref _calls);

        /// A cached answer: true with `instrumentId` set, or true with `error`
        /// set for a remembered failure; false when a lookup is needed.
        public bool TryGetCached(IdentifierKind kind, string value, out string? instrumentId, out Exception? error)
        {
            var key = Key(kind, value);
            lock (_lock)
            {
                if (_hits.TryGetValue(key, out var node))
                {
                    _order.Remove(node);
                    _order.AddFirst(node);
                    instrumentId = node.Value.Id;
                    error = null;
                    return true;
                }
                if (_failures.TryGetValue(key, out var failure) && failure.Until > _now())
                {
                    instrumentId = null;
                    error = failure.Error;
                    return true;
                }
            }
            instrumentId = null;
            error = null;
            return false;
        }

        /// Look `value` up; completes with the instrument id, or faults with a
        /// ConvexException (#N/A with the reason) when the answer is definite.
        public Task<string> ResolveAsync(IdentifierKind kind, string value)
        {
            var key = Key(kind, value);
            lock (_lock)
            {
                if (_inFlight.TryGetValue(key, out var running)) return running;
                var task = Task.Run(() => LookUpAsync(kind, value, key));
                _inFlight[key] = task;
                return task;
            }
        }

        /// Forget everything (a new server or sign-in).
        public void Clear()
        {
            lock (_lock)
            {
                _hits.Clear();
                _order.Clear();
                _failures.Clear();
            }
        }

        private async Task<string> LookUpAsync(IdentifierKind kind, string value, string key)
        {
            try
            {
                var id = await LookUpWithRetryAsync(kind, value).ConfigureAwait(false);
                lock (_lock)
                {
                    if (_hits.TryGetValue(key, out var old)) _order.Remove(old);
                    _hits[key] = _order.AddFirst((key, id));
                    while (_hits.Count > CacheSize)
                    {
                        var last = _order.Last!;
                        _order.RemoveLast();
                        _hits.Remove(last.Value.Key);
                    }
                    _failures.Remove(key);
                }
                return id;
            }
            catch (ConvexException ex)
            {
                Ops.CxLog.Connection($"identifier {value} not resolved: {ex.Message}");
                lock (_lock)
                {
                    if (_failures.Count >= CacheSize) _failures.Clear();
                    _failures[key] = (ex, _now() + FailureLifetime);
                }
                throw;
            }
            finally
            {
                lock (_lock) _inFlight.Remove(key);
            }
        }

        private async Task<string> LookUpWithRetryAsync(IdentifierKind kind, string value)
        {
            var name = kind == IdentifierKind.Cusip ? "CUSIP" : "ISIN";
            var backoff = _timings.BackoffInitial;
            while (true)
            {
                var endpoint = _source.HttpEndpoint();
                if (endpoint == null)
                {
                    // No server or no sign-in yet: wait for one.
                    await Task.Delay(TimeSpan.FromSeconds(1)).ConfigureAwait(false);
                    continue;
                }
                var (status, body, retryAfter) = await PostAsync(endpoint.Value.Base, endpoint.Value.Token, kind, value)
                    .ConfigureAwait(false);
                switch (status)
                {
                    case HttpStatusCode.OK:
                        var target = body?["target"];
                        if (ServerJson.Text(target?["kind"]) == "instrument" &&
                            ServerJson.Text(target?["instrument_id"]) is { } id)
                            return id;
                        throw ConvexException.Unavailable("NOT_AN_INSTRUMENT",
                            $"{name} {value} names a {ServerJson.Text(target?["kind"]) ?? "record"}, not an instrument");
                    case HttpStatusCode.NotFound:
                        throw ConvexException.Unavailable("UNKNOWN_IDENTIFIER", $"the server has no {name} {value}");
                    case HttpStatusCode.Forbidden:
                        throw ConvexException.Unavailable("FORBIDDEN",
                            $"looking up a {name} needs reference read on every instrument, which your sign-in " +
                            "lacks; use the server's instrument id instead");
                    case HttpStatusCode.BadRequest:
                        throw ConvexException.Unavailable(ServerJson.Text(body?["code"]) ?? "INVALID_INPUT",
                            $"the server refused {name} {value}: " + (ServerJson.Text(body?["error"]) ?? "bad request"));
                    case HttpStatusCode.Unauthorized:
                        _source.Tokens.Rejected();
                        break;
                    case (HttpStatusCode)429:
                        await Task.Delay(retryAfter ?? _timings.TooManyRequests).ConfigureAwait(false);
                        continue;
                }
                await Task.Delay(backoff).ConfigureAwait(false);
                backoff = TimeSpan.FromTicks(Math.Min(_timings.BackoffMax.Ticks, backoff.Ticks * 2));
            }
        }

        private async Task<(HttpStatusCode? Status, JObject? Body, TimeSpan? RetryAfter)> PostAsync(
            Uri baseUri, string? token, IdentifierKind kind, string value)
        {
            // as_of and known_at are both "now": the current mapping, as known now.
            var now = _now().ToString("yyyy-MM-ddTHH:mm:ss'Z'", CultureInfo.InvariantCulture);
            var request = new JObject
            {
                ["identifier"] = new JObject { ["kind"] = kind == IdentifierKind.Cusip ? "cusip" : "isin", ["value"] = value },
                ["lookup"] = new JObject { ["as_of"] = now, ["known_at"] = now },
            };
            await _slots.WaitAsync().ConfigureAwait(false);
            try
            {
                Interlocked.Increment(ref _calls);
                using var message = new HttpRequestMessage(HttpMethod.Post, new Uri(baseUri, "api/v1/reference/resolve"))
                {
                    Content = new StringContent(request.ToString(Formatting.None), Encoding.UTF8, "application/json"),
                };
                if (token != null) message.Headers.Authorization = new AuthenticationHeaderValue("Bearer", token);
                using var response = await _http.SendAsync(message).ConfigureAwait(false);
                var text = await response.Content.ReadAsStringAsync().ConfigureAwait(false);
                JObject? body = null;
                try { body = text.Length > 0 ? ServerJson.Parse(text) : null; }
                catch (JsonException) { /* not JSON: the status says enough */ }
                TimeSpan? retry = response.Headers.RetryAfter?.Delta;
                return (response.StatusCode, body, retry);
            }
            catch (Exception ex) when (ex is HttpRequestException or TaskCanceledException)
            {
                return (null, null, null);
            }
            finally
            {
                _slots.Release();
            }
        }

        private static string Key(IdentifierKind kind, string value) => (kind == IdentifierKind.Cusip ? "C:" : "I:") + value;
    }
}
