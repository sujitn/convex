using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Net;
using System.Net.Http;
using System.Net.Sockets;
using System.Security.Cryptography;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using Newtonsoft.Json.Linq;

namespace Convex.Excel.Server
{
    /// Where and how to sign in. The client is a public desktop client: it has
    /// no secret, and proves itself to the token endpoint with PKCE.
    internal sealed class OidcSettings
    {
        /// The identity provider's issuer URL, e.g. https://login.example.com/realms/desk.
        public string Issuer { get; set; } = "";

        /// The client id registered for this add-in.
        public string ClientId { get; set; } = "";

        /// Space-separated scopes. offline_access asks for a refresh token.
        public string Scope { get; set; } = "openid offline_access";

        /// Optional RFC 8707 resource indicator, sent on authorize and token requests when set.
        public string? Resource { get; set; }

        /// Host name used in the loopback redirect URI: 127.0.0.1 or localhost.
        public string RedirectHost { get; set; } = "127.0.0.1";
    }

    /// The tokens one sign-in or refresh produced.
    internal sealed class TokenSet
    {
        public TokenSet(string accessToken, DateTime issuedAtUtc, TimeSpan lifetime, string? refreshToken, string? idToken)
        {
            AccessToken = accessToken;
            IssuedAtUtc = issuedAtUtc;
            ExpiresAtUtc = issuedAtUtc + lifetime;
            RefreshToken = refreshToken;
            IdToken = idToken;
            User = (idToken != null ? Jwt.DisplayName(idToken) : null)
                ?? Jwt.DisplayName(accessToken)
                ?? "(signed in)";
        }

        /// The bearer token to send to the server.
        public string AccessToken { get; }

        /// When the token was received.
        public DateTime IssuedAtUtc { get; }

        /// When the access token stops being valid (issue time plus expires_in, one hour if absent).
        public DateTime ExpiresAtUtc { get; }

        /// The refresh token, when the provider issued one.
        public string? RefreshToken { get; }

        /// The OpenID identity token, when issued.
        public string? IdToken { get; }

        /// Who is signed in, for display.
        public string User { get; }
    }

    /// A sign-in problem. The message is shown to a trader, so it says what happened and what to do.
    internal sealed class OidcException : Exception
    {
        public OidcException(string message) : base(message) { }
        public OidcException(string message, Exception inner) : base(message, inner) { }
    }

    /// OpenID Connect authorization code flow with PKCE (S256) over a loopback
    /// redirect (RFC 8252). Hand-rolled so that no extra assemblies load into
    /// Excel's shared process.
    internal sealed class OidcClient
    {
        private static readonly TimeSpan ConnectionReadTimeout = TimeSpan.FromSeconds(10);
        private const int MaxRequestBytes = 16 * 1024;

        private readonly OidcSettings _settings;
        private readonly HttpClient _http;
        private readonly Action<Uri> _openBrowser;
        private readonly TimeSpan _signInTimeout;
        private readonly object _discoveryLock = new();
        private (string Authorization, string Token)? _discovery;

        /// openBrowser is called with the authorization URL and must return promptly.
        public OidcClient(OidcSettings settings, HttpClient http, Action<Uri> openBrowser, TimeSpan? signInTimeout = null)
        {
            _settings = settings;
            _http = http;
            _openBrowser = openBrowser;
            _signInTimeout = signInTimeout ?? TimeSpan.FromMinutes(5);
        }

        /// The loopback redirect host, lowercased: 127.0.0.1, or localhost for an
        /// identity provider that needs it. Any other host could send the
        /// authorization code off this machine, so it is refused.
        public static string RedirectHostOf(string? host)
        {
            var h = (host ?? "").Trim().ToLowerInvariant();
            if (h == "127.0.0.1" || h == "localhost") return h;
            throw new OidcException("The redirect host must be 127.0.0.1 or localhost; check Server settings.");
        }

        /// Signs in through the user's browser and exchanges the returned code for tokens.
        public async Task<TokenSet> SignInAsync(CancellationToken ct)
        {
            var host = RedirectHostOf(_settings.RedirectHost);
            var (authEndpoint, tokenEndpoint) = await DiscoverAsync(ct).ConfigureAwait(false);

            var verifier = Base64Url(RandomBytes(32));
            string challenge;
            using (var sha = SHA256.Create())
                challenge = Base64Url(sha.ComputeHash(Encoding.ASCII.GetBytes(verifier)));
            var state = Base64Url(RandomBytes(16));

            var listeners = LoopbackListeners(host);
            try
            {
                var port = ((IPEndPoint)listeners[0].LocalEndpoint).Port;
                var redirectUri = "http://" + host + ":" + port.ToString(CultureInfo.InvariantCulture) + "/";

                var query = new List<KeyValuePair<string, string>>
                {
                    new("response_type", "code"),
                    new("client_id", _settings.ClientId),
                    new("redirect_uri", redirectUri),
                    new("scope", _settings.Scope),
                    new("state", state),
                    new("code_challenge", challenge),
                    new("code_challenge_method", "S256"),
                };
                if (!string.IsNullOrEmpty(_settings.Resource))
                    query.Add(new("resource", _settings.Resource!));
                var authorizeUri = new Uri(authEndpoint + (authEndpoint.Contains("?") ? "&" : "?") + EncodeQuery(query));

                try
                {
                    _openBrowser(authorizeUri);
                }
                catch (Exception ex)
                {
                    throw new OidcException("Could not open your web browser to sign in: " + ex.Message, ex);
                }

                var callback = await WaitForCallbackAsync(listeners, ct).ConfigureAwait(false);

                if (callback.TryGetValue("error", out var error) && !string.IsNullOrEmpty(error))
                {
                    callback.TryGetValue("error_description", out var desc);
                    throw new OidcException("The sign-in was not completed: " + error
                        + (string.IsNullOrEmpty(desc) ? "" : " (" + desc + ")") + ". Try Sign in again.");
                }
                if (!callback.TryGetValue("state", out var returnedState) || returnedState != state)
                    throw new OidcException("The sign-in response did not match this sign-in attempt and was ignored. Try Sign in again.");
                if (!callback.TryGetValue("code", out var code) || string.IsNullOrEmpty(code))
                    throw new OidcException("The sign-in server returned no authorization code. Try Sign in again.");

                var form = new List<KeyValuePair<string, string>>
                {
                    new("grant_type", "authorization_code"),
                    new("code", code),
                    new("redirect_uri", redirectUri),
                    new("client_id", _settings.ClientId),
                    new("code_verifier", verifier),
                };
                if (!string.IsNullOrEmpty(_settings.Resource))
                    form.Add(new("resource", _settings.Resource!));
                return await PostTokenAsync(tokenEndpoint, form, null, ct).ConfigureAwait(false);
            }
            finally
            {
                foreach (var listener in listeners) listener.Stop();
            }
        }

        /// Gets a new access token with a refresh token. When the response carries
        /// no new refresh token, the old one is kept.
        public async Task<TokenSet> RefreshAsync(string refreshToken, CancellationToken ct)
        {
            var (_, tokenEndpoint) = await DiscoverAsync(ct).ConfigureAwait(false);
            var form = new List<KeyValuePair<string, string>>
            {
                new("grant_type", "refresh_token"),
                new("refresh_token", refreshToken),
                new("client_id", _settings.ClientId),
            };
            if (!string.IsNullOrEmpty(_settings.Scope))
                form.Add(new("scope", _settings.Scope));
            if (!string.IsNullOrEmpty(_settings.Resource))
                form.Add(new("resource", _settings.Resource!));
            return await PostTokenAsync(tokenEndpoint, form, refreshToken, ct).ConfigureAwait(false);
        }

        // ---- discovery ----

        /// Fetches (once per client) the provider's endpoints and checks the issuer matches.
        private async Task<(string Authorization, string Token)> DiscoverAsync(CancellationToken ct)
        {
            lock (_discoveryLock)
            {
                if (_discovery != null) return _discovery.Value;
            }

            if (string.IsNullOrWhiteSpace(_settings.Issuer))
                throw new OidcException("No sign-in server (issuer) is configured. Set it in Convex Server settings.");
            RequireSecure(_settings.Issuer, "The sign-in server address");
            var issuer = _settings.Issuer.TrimEnd('/');

            var json = await GetJsonAsync(issuer + "/.well-known/openid-configuration", ct).ConfigureAwait(false);
            var docIssuer = (string?)json["issuer"];
            if (docIssuer == null || !string.Equals(docIssuer.TrimEnd('/'), issuer, StringComparison.Ordinal))
                throw new OidcException("The sign-in server identifies itself as '" + (docIssuer ?? "(nothing)")
                    + "', not '" + issuer + "'. Check the issuer in Convex Server settings.");
            var auth = (string?)json["authorization_endpoint"];
            var token = (string?)json["token_endpoint"];
            if (string.IsNullOrEmpty(auth) || string.IsNullOrEmpty(token))
                throw new OidcException("The sign-in server does not describe an authorization and token endpoint. Check the issuer in Convex Server settings.");
            RequireSecure(auth!, "The sign-in server's authorization address");
            RequireSecure(token!, "The sign-in server's token address");

            lock (_discoveryLock)
                _discovery = (auth!, token!);
            return (auth!, token!);
        }

        /// Refuses plain http unless the host is this machine (for testing).
        private static void RequireSecure(string url, string what)
        {
            if (!Uri.TryCreate(url, UriKind.Absolute, out var uri))
                throw new OidcException(what + " '" + url + "' is not a valid web address.");
            if (uri.Scheme == Uri.UriSchemeHttps) return;
            if (uri.Scheme == Uri.UriSchemeHttp && uri.IsLoopback) return;
            throw new OidcException(what + " '" + url + "' must start with https:// so your sign-in is protected.");
        }

        private async Task<JObject> GetJsonAsync(string url, CancellationToken ct)
        {
            string text;
            try
            {
                using var resp = await _http.GetAsync(url, ct).ConfigureAwait(false);
                text = await resp.Content.ReadAsStringAsync().ConfigureAwait(false);
                if (!resp.IsSuccessStatusCode)
                    throw new OidcException("The sign-in server answered " + (int)resp.StatusCode + " when asked for its settings. Check the issuer in Convex Server settings.");
            }
            catch (OidcException) { throw; }
            catch (Exception ex) when (ex is HttpRequestException || (ex is OperationCanceledException && !ct.IsCancellationRequested))
            {
                throw new OidcException("Could not reach the sign-in server at " + url + ". Check your network and try again.", ex);
            }
            return ParseObject(text, "settings");
        }

        private static JObject ParseObject(string text, string what)
        {
            try
            {
                return JObject.Parse(text);
            }
            catch (Newtonsoft.Json.JsonException ex)
            {
                throw new OidcException("The sign-in server's " + what + " were not understood. Check the issuer in Convex Server settings.", ex);
            }
        }

        // ---- token endpoint ----

        private async Task<TokenSet> PostTokenAsync(string endpoint, List<KeyValuePair<string, string>> form, string? oldRefreshToken, CancellationToken ct)
        {
            HttpStatusCode status;
            string text;
            DateTime issuedAt;
            try
            {
                using var content = new StringContent(EncodeQuery(form), Encoding.UTF8, "application/x-www-form-urlencoded");
                using var resp = await _http.PostAsync(endpoint, content, ct).ConfigureAwait(false);
                status = resp.StatusCode;
                text = await resp.Content.ReadAsStringAsync().ConfigureAwait(false);
                issuedAt = DateTime.UtcNow;
            }
            catch (Exception ex) when (ex is HttpRequestException || (ex is OperationCanceledException && !ct.IsCancellationRequested))
            {
                throw new OidcException("Could not reach the sign-in server. Check your network and try again.", ex);
            }

            if ((int)status < 200 || (int)status > 299)
            {
                string? err = null, desc = null;
                try
                {
                    var j = JObject.Parse(text);
                    err = (string?)j["error"];
                    desc = (string?)j["error_description"];
                }
                catch (Newtonsoft.Json.JsonException) { }
                var detail = err != null ? err + (desc != null ? ": " + desc : "") : "HTTP " + (int)status;
                throw new OidcException("The sign-in server refused the request (" + detail + "). Sign in again.");
            }

            var json = ParseObject(text, "token response");
            var access = (string?)json["access_token"];
            if (string.IsNullOrEmpty(access))
                throw new OidcException("The sign-in server returned no access token. Try Sign in again.");
            var type = (string?)json["token_type"];
            if (type != null && !string.Equals(type, "Bearer", StringComparison.OrdinalIgnoreCase))
                throw new OidcException("The sign-in server issued a '" + type + "' token; Convex needs a Bearer token.");

            double seconds = 3600;
            if (json["expires_in"] is JValue ev && ev.Type != JTokenType.Null)
            {
                try { seconds = Convert.ToDouble(ev.Value, CultureInfo.InvariantCulture); }
                catch (Exception ex) when (ex is FormatException || ex is InvalidCastException) { seconds = 3600; }
            }
            var refresh = (string?)json["refresh_token"];
            if (string.IsNullOrEmpty(refresh)) refresh = oldRefreshToken;
            return new TokenSet(access!, issuedAt, TimeSpan.FromSeconds(seconds), refresh, (string?)json["id_token"]);
        }

        // ---- loopback redirect ----

        /// Started listeners for the redirect: a free port on 127.0.0.1 and, for
        /// localhost, the same port on ::1, because a browser may resolve
        /// localhost to either. Without IPv6, localhost is 127.0.0.1 alone.
        private static List<TcpListener> LoopbackListeners(string host)
        {
            for (var attempt = 1; ; attempt++)
            {
                var v4 = new TcpListener(IPAddress.Loopback, 0);
                v4.Start();
                if (host != "localhost" || !Socket.OSSupportsIPv6) return new List<TcpListener> { v4 };
                var v6 = new TcpListener(IPAddress.IPv6Loopback, ((IPEndPoint)v4.LocalEndpoint).Port);
                try
                {
                    v6.Start();
                    return new List<TcpListener> { v4, v6 };
                }
                catch (SocketException ex) when (ex.SocketErrorCode == SocketError.AddressAlreadyInUse)
                {
                    // Another program holds this port on ::1, where the browser might
                    // send the code: try another port.
                    v4.Stop();
                    if (attempt == 5)
                        throw new OidcException("Could not listen for the sign-in response: no port is free on both 127.0.0.1 and ::1.", ex);
                }
                catch (SocketException)
                {
                    return new List<TcpListener> { v4 }; // no IPv6 loopback
                }
            }
        }

        /// Accepts connections on any of `listeners` until one carries the redirect
        /// (GET /?...), answers it, and returns its query parameters. Honours the
        /// sign-in timeout and ct.
        private async Task<Dictionary<string, string>> WaitForCallbackAsync(IReadOnlyList<TcpListener> listeners, CancellationToken ct)
        {
            using var timeout = new CancellationTokenSource(_signInTimeout);
            using var linked = CancellationTokenSource.CreateLinkedTokenSource(ct, timeout.Token);
            // Stopping a listener is the only way to abort a pending accept on .NET Framework.
            using var stopReg = linked.Token.Register(() =>
            {
                foreach (var listener in listeners)
                {
                    try { listener.Stop(); } catch { }
                }
            });

            var accepts = new Task<TcpClient>[listeners.Count];
            for (var i = 0; i < accepts.Length; i++) accepts[i] = listeners[i].AcceptTcpClientAsync();
            try
            {
                while (true)
                {
                    var done = await Task.WhenAny(accepts).ConfigureAwait(false);
                    var index = Array.IndexOf(accepts, done);
                    TcpClient client;
                    try
                    {
                        client = await done.ConfigureAwait(false);
                    }
                    catch (Exception ex) when (ex is ObjectDisposedException || ex is SocketException || ex is InvalidOperationException)
                    {
                        ct.ThrowIfCancellationRequested();
                        if (linked.IsCancellationRequested)
                            throw new OidcException("Sign-in timed out: the browser did not return to Convex. Try Sign in again.");
                        throw new OidcException("Could not listen for the sign-in response: " + ex.Message, ex);
                    }

                    using (client)
                    {
                        var result = await HandleConnectionAsync(client, linked.Token).ConfigureAwait(false);
                        if (result != null) return result;
                    }
                    if (linked.IsCancellationRequested)
                    {
                        ct.ThrowIfCancellationRequested();
                        throw new OidcException("Sign-in timed out: the browser did not return to Convex. Try Sign in again.");
                    }
                    accepts[index] = listeners[index].AcceptTcpClientAsync();
                }
            }
            finally
            {
                // The other accepts end when the caller stops the listeners: close a
                // connection one took meanwhile and observe their faults.
                foreach (var accept in accepts)
                {
                    _ = accept.ContinueWith(t =>
                    {
                        if (t.Status == TaskStatus.RanToCompletion) t.Result.Close();
                        else _ = t.Exception;
                    }, TaskScheduler.Default);
                }
            }
        }

        /// Reads one request; returns the redirect's query, or null for any other request (answered 404).
        private static async Task<Dictionary<string, string>?> HandleConnectionAsync(TcpClient client, CancellationToken overall)
        {
            using var perConn = new CancellationTokenSource(ConnectionReadTimeout);
            using var linked = CancellationTokenSource.CreateLinkedTokenSource(overall, perConn.Token);
            using var closeReg = linked.Token.Register(() => { try { client.Close(); } catch { } });
            try
            {
                var stream = client.GetStream();
                var buf = new byte[MaxRequestBytes];
                var len = 0;
                string? requestLine = null;
                while (requestLine == null && len < buf.Length)
                {
                    var n = await stream.ReadAsync(buf, len, buf.Length - len, linked.Token).ConfigureAwait(false);
                    if (n <= 0) return null;
                    len += n;
                    for (var i = 0; i + 1 < len; i++)
                    {
                        if (buf[i] == '\r' && buf[i + 1] == '\n')
                        {
                            requestLine = Encoding.ASCII.GetString(buf, 0, i);
                            break;
                        }
                    }
                }
                if (requestLine == null) return null;

                var parts = requestLine.Split(' ');
                if (parts.Length < 2 || parts[0] != "GET" || !parts[1].StartsWith("/?", StringComparison.Ordinal))
                {
                    await WriteResponseAsync(stream, "404 Not Found", "Not found.", linked.Token).ConfigureAwait(false);
                    return null;
                }

                var query = ParseQuery(parts[1].Substring(2));
                string message;
                if (query.TryGetValue("error", out var error) && !string.IsNullOrEmpty(error))
                    message = "Sign-in failed: " + error + ". You can close this tab and try again in Excel.";
                else
                    message = "Signed in to Convex. You can close this tab.";
                await WriteResponseAsync(stream, "200 OK", message, linked.Token).ConfigureAwait(false);
                return query;
            }
            catch (Exception ex) when (ex is IOException || ex is ObjectDisposedException || ex is SocketException || ex is OperationCanceledException || ex is InvalidOperationException)
            {
                return null;
            }
        }

        private static async Task WriteResponseAsync(Stream stream, string status, string message, CancellationToken ct)
        {
            var body = Encoding.UTF8.GetBytes("<!doctype html><html><head><meta charset=\"utf-8\"><title>Convex</title></head><body><p>"
                + WebUtility.HtmlEncode(message) + "</p></body></html>");
            var head = Encoding.ASCII.GetBytes("HTTP/1.1 " + status + "\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: "
                + body.Length.ToString(CultureInfo.InvariantCulture) + "\r\nConnection: close\r\n\r\n");
            await stream.WriteAsync(head, 0, head.Length, ct).ConfigureAwait(false);
            await stream.WriteAsync(body, 0, body.Length, ct).ConfigureAwait(false);
            await stream.FlushAsync(ct).ConfigureAwait(false);
        }

        // ---- helpers ----

        private static Dictionary<string, string> ParseQuery(string query)
        {
            var result = new Dictionary<string, string>(StringComparer.Ordinal);
            var hash = query.IndexOf('#');
            if (hash >= 0) query = query.Substring(0, hash);
            foreach (var pair in query.Split(new[] { '&' }, StringSplitOptions.RemoveEmptyEntries))
            {
                var eq = pair.IndexOf('=');
                var key = eq < 0 ? pair : pair.Substring(0, eq);
                var value = eq < 0 ? "" : pair.Substring(eq + 1);
                result[Decode(key)] = Decode(value);
            }
            return result;
        }

        private static string Decode(string s)
        {
            try { return Uri.UnescapeDataString(s.Replace('+', ' ')); }
            catch (UriFormatException) { return s; }
        }

        private static string EncodeQuery(IEnumerable<KeyValuePair<string, string>> pairs)
        {
            var sb = new StringBuilder();
            foreach (var p in pairs)
            {
                if (sb.Length > 0) sb.Append('&');
                sb.Append(Uri.EscapeDataString(p.Key)).Append('=').Append(Uri.EscapeDataString(p.Value));
            }
            return sb.ToString();
        }

        private static byte[] RandomBytes(int n)
        {
            var bytes = new byte[n];
            using (var rng = new RNGCryptoServiceProvider())
                rng.GetBytes(bytes);
            return bytes;
        }

        private static string Base64Url(byte[] bytes) =>
            Convert.ToBase64String(bytes).TrimEnd('=').Replace('+', '-').Replace('/', '_');
    }

    /// ITokenProvider over OidcClient: sign in interactively, renew at 80 % of each token's lifetime.
    internal sealed class OidcTokenProvider : ITokenProvider, IDisposable
    {
        private static readonly TimeSpan RetryStart = TimeSpan.FromSeconds(5);
        private static readonly TimeSpan RetryMax = TimeSpan.FromSeconds(60);
        private static readonly TimeSpan MaxTimerDue = TimeSpan.FromDays(20);

        private readonly OidcClient _client;
        private readonly Func<DateTime> _utcNow;
        private readonly object _lock = new();
        private TokenSet? _tokens;
        private string? _problem = "not signed in: use Sign in on the Convex ribbon";
        private string _user = "(not signed in)";
        private Timer? _timer;
        private long _generation;
        private bool _refreshing;
        private TimeSpan _retryDelay = RetryStart;
        private bool _disposed;

        public OidcTokenProvider(OidcClient client, Func<DateTime>? utcNow = null)
        {
            _client = client;
            _utcNow = utcNow ?? (() => DateTime.UtcNow);
        }

        public string? Token { get { lock (_lock) return _tokens?.AccessToken; } }
        public bool Ready { get { lock (_lock) return _tokens != null; } }
        public string User { get { lock (_lock) return _user; } }
        public string? Problem { get { lock (_lock) return _problem; } }
        public event Action? Changed;

        /// Signs in through the browser. On success the token is set and renewal is scheduled.
        public async Task SignInAsync(CancellationToken ct)
        {
            var tokens = await _client.SignInAsync(ct).ConfigureAwait(false);
            lock (_lock)
            {
                if (_disposed) return;
                _generation++;
                _refreshing = false;
                _retryDelay = RetryStart;
                Install(tokens);
            }
            Ops.CxLog.Connection("signed in as " + tokens.User);
            RaiseChanged();
        }

        /// Forgets the tokens and stops renewal.
        public void SignOut()
        {
            lock (_lock)
            {
                _generation++;
                _refreshing = false;
                _timer?.Dispose();
                _timer = null;
                _tokens = null;
                _user = "(not signed in)";
                _problem = "signed out";
            }
            Ops.CxLog.Connection("signed out");
            RaiseChanged();
        }

        /// The server refused the token: refresh now if possible, otherwise sign out.
        public void Rejected()
        {
            var signedOut = false;
            lock (_lock)
            {
                if (_tokens == null || _disposed) return;
                if (_tokens.RefreshToken != null)
                {
                    StartRefreshLocked(_generation);
                }
                else
                {
                    _generation++;
                    _timer?.Dispose();
                    _timer = null;
                    _tokens = null;
                    _problem = "the server refused your sign-in: sign in again";
                    signedOut = true;
                }
            }
            if (signedOut)
            {
                Ops.CxLog.Connection("the server refused the sign-in");
                RaiseChanged();
            }
        }

        /// Stops the renewal timer.
        public void Dispose()
        {
            lock (_lock)
            {
                _disposed = true;
                _generation++;
                _timer?.Dispose();
                _timer = null;
            }
        }

        /// Stores new tokens and arms the timer. Caller holds the lock.
        private void Install(TokenSet tokens)
        {
            _tokens = tokens;
            _user = tokens.User;
            _problem = null;
            Arm(tokens.RefreshToken != null ? RenewAt(tokens) : tokens.ExpiresAtUtc);
        }

        /// Eighty percent of the way through the token's lifetime.
        private static DateTime RenewAt(TokenSet t) =>
            t.IssuedAtUtc + TimeSpan.FromTicks((long)((t.ExpiresAtUtc - t.IssuedAtUtc).Ticks * 0.8));

        /// Arms a one-shot timer for a moment in time. Caller holds the lock.
        private void Arm(DateTime dueUtc)
        {
            var delay = dueUtc - _utcNow();
            if (delay < TimeSpan.Zero) delay = TimeSpan.Zero;
            if (delay > MaxTimerDue) delay = MaxTimerDue;
            var gen = _generation;
            _timer?.Dispose();
            _timer = new Timer(_ => OnTimer(gen), null, delay, Timeout.InfiniteTimeSpan);
        }

        private void OnTimer(long gen)
        {
            lock (_lock)
            {
                if (gen != _generation || _tokens == null || _disposed) return;
                var tokens = _tokens;
                if (tokens.RefreshToken != null)
                {
                    var renewAt = RenewAt(tokens);
                    if (_utcNow() < renewAt) Arm(renewAt);
                    else StartRefreshLocked(gen);
                    return;
                }
                if (_utcNow() < tokens.ExpiresAtUtc)
                {
                    Arm(tokens.ExpiresAtUtc);
                    return;
                }
                GoExpired();
            }
            RaiseChanged();
        }

        /// Starts a refresh unless one is already in flight. Caller holds the lock.
        private void StartRefreshLocked(long gen)
        {
            if (_refreshing || gen != _generation || _tokens?.RefreshToken == null) return;
            _refreshing = true;
            var refreshToken = _tokens.RefreshToken;
            _ = Task.Run(() => RefreshAsync(gen, refreshToken));
        }

        private async Task RefreshAsync(long gen, string refreshToken)
        {
            TokenSet? fresh = null;
            try
            {
                fresh = await _client.RefreshAsync(refreshToken, CancellationToken.None).ConfigureAwait(false);
                Ops.CxLog.Connection("token renewed for " + fresh.User);
            }
            catch (Exception ex)
            {
                // handled below as a failed renewal
                Ops.CxLog.Connection("token renewal failed: " + ex.Message);
            }

            var raise = false;
            lock (_lock)
            {
                if (gen != _generation || _disposed) return; // a sign-in or sign-out won
                _refreshing = false;
                if (fresh != null)
                {
                    _generation++;
                    _retryDelay = RetryStart;
                    Install(fresh);
                    raise = true;
                }
                else if (_tokens == null || _utcNow() >= _tokens.ExpiresAtUtc)
                {
                    GoExpired();
                    raise = true;
                }
                else
                {
                    var untilExpiry = _tokens.ExpiresAtUtc - _utcNow();
                    var delay = _retryDelay < untilExpiry ? _retryDelay : untilExpiry;
                    _retryDelay = TimeSpan.FromTicks(Math.Min(RetryMax.Ticks, _retryDelay.Ticks * 2));
                    Arm(_utcNow() + delay);
                }
            }
            if (raise) RaiseChanged();
        }

        /// The access token ran out and could not be renewed. Caller holds the lock.
        private void GoExpired()
        {
            _generation++;
            _timer?.Dispose();
            _timer = null;
            _tokens = null;
            _problem = "your Convex sign-in expired: sign in again";
            Ops.CxLog.Connection("the sign-in expired");
        }

        private void RaiseChanged()
        {
            try { Changed?.Invoke(); }
            catch (Exception) { /* a subscriber's failure must not break the renewal thread */ }
        }
    }
}
