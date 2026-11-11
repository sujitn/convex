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
using Convex.Excel.Server;
using Newtonsoft.Json.Linq;
using Xunit;

namespace Convex.Excel.Tests
{
    /// A fake identity provider on HttpListener (localhost prefix, no admin needed).
    internal sealed class FakeIdp : IDisposable
    {
        private readonly HttpListener _listener = new();
        private readonly Dictionary<string, string> _challenges = new();
        private int _counter;

        public string BaseUrl { get; }
        public string? IssuerOverride { get; set; }
        public bool AuthorizeError { get; set; }
        public bool WrongState { get; set; }
        public bool FailToken { get; set; }
        public bool RotateRefresh { get; set; }
        public bool IssueRefresh { get; set; } = true;
        public int ExpiresIn { get; set; } = 3600;
        public int RefreshExpiresIn { get; set; } = 3600;
        public int RefreshCount;
        public Dictionary<string, string> LastAuthorize = new();
        public Dictionary<string, string> LastTokenForm = new();
        public bool VerifierMatched;

        public FakeIdp()
        {
            var probe = new TcpListener(IPAddress.Loopback, 0);
            probe.Start();
            var port = ((IPEndPoint)probe.LocalEndpoint).Port;
            probe.Stop();
            BaseUrl = "http://localhost:" + port + "/realms/desk";
            _listener.Prefixes.Add("http://localhost:" + port + "/");
            _listener.Start();
            _ = Task.Run(Loop);
        }

        private async Task Loop()
        {
            while (_listener.IsListening)
            {
                HttpListenerContext ctx;
                try { ctx = await _listener.GetContextAsync().ConfigureAwait(false); }
                catch { return; }
                try { Handle(ctx); }
                catch { try { ctx.Response.Abort(); } catch { } }
            }
        }

        private void Handle(HttpListenerContext ctx)
        {
            var path = ctx.Request.Url!.AbsolutePath;
            if (path.EndsWith("/.well-known/openid-configuration", StringComparison.Ordinal))
            {
                Json(ctx, 200, new JObject
                {
                    ["issuer"] = IssuerOverride ?? BaseUrl,
                    ["authorization_endpoint"] = BaseUrl + "/authorize",
                    ["token_endpoint"] = BaseUrl + "/token",
                });
            }
            else if (path.EndsWith("/authorize", StringComparison.Ordinal))
            {
                var q = ParseForm(ctx.Request.Url.Query.TrimStart('?'));
                LastAuthorize = q;
                var redirect = q["redirect_uri"];
                string location;
                if (AuthorizeError)
                {
                    location = redirect + "?error=access_denied&error_description=user%20said%20no&state=" + Uri.EscapeDataString(q["state"]);
                }
                else
                {
                    var code = "code-" + Interlocked.Increment(ref _counter);
                    lock (_challenges) _challenges[code] = q["code_challenge"];
                    var state = WrongState ? "not-the-state" : q["state"];
                    location = redirect + "?code=" + code + "&state=" + Uri.EscapeDataString(state);
                }
                ctx.Response.StatusCode = 302;
                ctx.Response.RedirectLocation = location;
                ctx.Response.Close();
            }
            else if (path.EndsWith("/token", StringComparison.Ordinal))
            {
                string body;
                using (var r = new StreamReader(ctx.Request.InputStream, Encoding.UTF8)) body = r.ReadToEnd();
                var f = ParseForm(body);
                LastTokenForm = f;
                if (FailToken)
                {
                    Json(ctx, 400, new JObject { ["error"] = "invalid_grant", ["error_description"] = "the code is stale" });
                    return;
                }
                if (f["grant_type"] == "authorization_code")
                {
                    string? challenge;
                    lock (_challenges) _challenges.TryGetValue(f["code"], out challenge);
                    using var sha = SHA256.Create();
                    var computed = Base64Url(sha.ComputeHash(Encoding.ASCII.GetBytes(f["code_verifier"])));
                    VerifierMatched = challenge == computed;
                    if (!VerifierMatched)
                    {
                        Json(ctx, 400, new JObject { ["error"] = "invalid_grant", ["error_description"] = "PKCE verification failed" });
                        return;
                    }
                    Json(ctx, 200, Tokens(ExpiresIn, IssueRefresh ? "refresh-0" : null));
                }
                else if (f["grant_type"] == "refresh_token")
                {
                    Interlocked.Increment(ref RefreshCount);
                    Json(ctx, 200, Tokens(RefreshExpiresIn, RotateRefresh ? "refresh-" + RefreshCount : null));
                }
                else
                {
                    Json(ctx, 400, new JObject { ["error"] = "unsupported_grant_type" });
                }
            }
            else
            {
                ctx.Response.StatusCode = 404;
                ctx.Response.Close();
            }
        }

        private JObject Tokens(int expiresIn, string? refresh)
        {
            var n = Interlocked.Increment(ref _counter);
            var o = new JObject
            {
                ["access_token"] = "access-" + n,
                ["token_type"] = "bearer",
                ["expires_in"] = expiresIn,
                ["id_token"] = Jwt("{\"alg\":\"none\"}", "{\"preferred_username\":\"trader1\"}"),
            };
            if (refresh != null) o["refresh_token"] = refresh;
            return o;
        }

        private static string Jwt(string header, string payload) =>
            Base64Url(Encoding.UTF8.GetBytes(header)) + "." + Base64Url(Encoding.UTF8.GetBytes(payload)) + ".";

        private static void Json(HttpListenerContext ctx, int status, JObject body)
        {
            var bytes = Encoding.UTF8.GetBytes(body.ToString());
            ctx.Response.StatusCode = status;
            ctx.Response.ContentType = "application/json";
            ctx.Response.ContentLength64 = bytes.Length;
            ctx.Response.OutputStream.Write(bytes, 0, bytes.Length);
            ctx.Response.Close();
        }

        public static Dictionary<string, string> ParseForm(string s)
        {
            var d = new Dictionary<string, string>();
            foreach (var pair in s.Split(new[] { '&' }, StringSplitOptions.RemoveEmptyEntries))
            {
                var eq = pair.IndexOf('=');
                var k = eq < 0 ? pair : pair.Substring(0, eq);
                var v = eq < 0 ? "" : pair.Substring(eq + 1);
                d[Uri.UnescapeDataString(k.Replace('+', ' '))] = Uri.UnescapeDataString(v.Replace('+', ' '));
            }
            return d;
        }

        public static string Base64Url(byte[] b) => Convert.ToBase64String(b).TrimEnd('=').Replace('+', '-').Replace('/', '_');

        public void Dispose()
        {
            try { _listener.Close(); } catch { }
        }
    }

    /// An HttpMessageHandler that counts requests and always fails.
    internal sealed class CountingHandler : HttpMessageHandler
    {
        public int Requests;
        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            Interlocked.Increment(ref Requests);
            throw new HttpRequestException("no network in this test");
        }
    }

    public class OidcTests : IDisposable
    {
        private readonly FakeIdp _idp = new();
        private readonly HttpClient _http = new();
        private readonly HttpClient _browserHttp = new(new HttpClientHandler { AllowAutoRedirect = false });

        public void Dispose()
        {
            _idp.Dispose();
            _http.Dispose();
            _browserHttp.Dispose();
        }

        private OidcSettings Settings() => new() { Issuer = _idp.BaseUrl, ClientId = "convex-excel" };

        /// The fake browser: follows the authorize request, then calls the loopback redirect itself.
        private void Browser(Uri authorize)
        {
            _ = Task.Run(async () =>
            {
                try
                {
                    using var r = await _browserHttp.GetAsync(authorize).ConfigureAwait(false);
                    var loc = r.Headers.Location;
                    if (loc != null)
                    {
                        using var done = await _http.GetAsync(loc).ConfigureAwait(false);
                    }
                }
                catch { }
            });
        }

        private OidcClient Client(OidcSettings? s = null, TimeSpan? timeout = null) =>
            new(s ?? Settings(), _http, Browser, timeout ?? TimeSpan.FromSeconds(10));

        private static async Task<bool> WaitFor(Func<bool> cond, int ms = 5000)
        {
            var sw = System.Diagnostics.Stopwatch.StartNew();
            while (sw.ElapsedMilliseconds < ms)
            {
                if (cond()) return true;
                await Task.Delay(25);
            }
            return cond();
        }

        [Fact]
        public async Task Sign_in_succeeds_with_pkce_loopback_redirect_and_state_round_trip()
        {
            var tokens = await Client().SignInAsync(CancellationToken.None);

            var q = _idp.LastAuthorize;
            Assert.Equal("code", q["response_type"]);
            Assert.Equal("convex-excel", q["client_id"]);
            Assert.Equal("S256", q["code_challenge_method"]);
            Assert.Equal("openid offline_access", q["scope"]);
            Assert.StartsWith("http://127.0.0.1:", q["redirect_uri"]);
            Assert.EndsWith("/", q["redirect_uri"]);
            Assert.True(int.Parse(new Uri(q["redirect_uri"]).Port.ToString(CultureInfo.InvariantCulture)) > 0);
            Assert.False(q.ContainsKey("resource"));

            Assert.True(_idp.VerifierMatched);
            Assert.Equal(q["redirect_uri"], _idp.LastTokenForm["redirect_uri"]);
            Assert.Equal("convex-excel", _idp.LastTokenForm["client_id"]);

            Assert.StartsWith("access-", tokens.AccessToken);
            Assert.Equal("refresh-0", tokens.RefreshToken);
            Assert.Equal("trader1", tokens.User);
            Assert.True(tokens.ExpiresAtUtc - tokens.IssuedAtUtc == TimeSpan.FromSeconds(3600));
        }

        [Fact]
        public async Task Resource_parameter_is_sent_on_authorize_and_token_requests()
        {
            var s = Settings();
            s.Resource = "https://convex.example/api";
            await Client(s).SignInAsync(CancellationToken.None);
            Assert.Equal("https://convex.example/api", _idp.LastAuthorize["resource"]);
            Assert.Equal("https://convex.example/api", _idp.LastTokenForm["resource"]);
        }

        [Fact]
        public async Task A_wrong_state_on_the_redirect_fails()
        {
            _idp.WrongState = true;
            await Assert.ThrowsAsync<OidcException>(() => Client().SignInAsync(CancellationToken.None));
        }

        [Fact]
        public async Task An_error_on_the_redirect_is_named_in_the_exception()
        {
            _idp.AuthorizeError = true;
            var ex = await Assert.ThrowsAsync<OidcException>(() => Client().SignInAsync(CancellationToken.None));
            Assert.Contains("access_denied", ex.Message);
        }

        [Fact]
        public async Task A_discovery_document_with_a_different_issuer_is_refused()
        {
            _idp.IssuerOverride = "http://localhost:1/other";
            var ex = await Assert.ThrowsAsync<OidcException>(() => Client().SignInAsync(CancellationToken.None));
            Assert.Contains("other", ex.Message);
        }

        [Fact]
        public async Task A_non_https_non_loopback_issuer_is_refused_before_any_request()
        {
            var handler = new CountingHandler();
            using var http = new HttpClient(handler);
            var client = new OidcClient(new OidcSettings { Issuer = "http://idp.example.com/realm", ClientId = "x" }, http, _ => { });
            var ex = await Assert.ThrowsAsync<OidcException>(() => client.SignInAsync(CancellationToken.None));
            Assert.Contains("https", ex.Message);
            Assert.Equal(0, handler.Requests);
        }

        [Fact]
        public async Task A_token_endpoint_invalid_grant_surfaces_the_error_text()
        {
            _idp.FailToken = true;
            var ex = await Assert.ThrowsAsync<OidcException>(() => Client().SignInAsync(CancellationToken.None));
            Assert.Contains("invalid_grant", ex.Message);
            Assert.Contains("the code is stale", ex.Message);
        }

        [Fact]
        public async Task Refresh_keeps_the_old_refresh_token_when_the_response_has_none()
        {
            var client = Client();
            var t = await client.RefreshAsync("refresh-keep", CancellationToken.None);
            Assert.Equal("refresh-keep", t.RefreshToken);
            Assert.Equal("refresh_token", _idp.LastTokenForm["grant_type"]);
            Assert.Equal(1, _idp.RefreshCount);
        }

        [Fact]
        public async Task Provider_renews_at_eighty_percent_of_the_token_lifetime()
        {
            _idp.ExpiresIn = 2;
            using var provider = new OidcTokenProvider(Client());
            Assert.False(provider.Ready);
            Assert.Null(provider.Token);
            Assert.Equal("(not signed in)", provider.User);

            await provider.SignInAsync(CancellationToken.None);
            var first = provider.Token;
            Assert.True(provider.Ready);
            Assert.Null(provider.Problem);
            Assert.Equal("trader1", provider.User);

            var changed = 0;
            provider.Changed += () => Interlocked.Increment(ref changed);
            var sw = System.Diagnostics.Stopwatch.StartNew();
            Assert.True(await WaitFor(() => provider.Token != first));
            Assert.InRange(sw.ElapsedMilliseconds, 1000, 4500);
            Assert.True(changed >= 1);
            Assert.Equal(1, _idp.RefreshCount);
            Assert.True(provider.Ready);
        }

        [Fact]
        public async Task Provider_without_a_refresh_token_goes_not_ready_after_expiry()
        {
            _idp.ExpiresIn = 1;
            _idp.IssueRefresh = false;
            using var provider = new OidcTokenProvider(Client());
            await provider.SignInAsync(CancellationToken.None);
            Assert.True(provider.Ready);

            Assert.True(await WaitFor(() => !provider.Ready));
            Assert.Null(provider.Token);
            Assert.Contains("expired", provider.Problem);
            Assert.Equal(0, _idp.RefreshCount);
        }

        [Fact]
        public async Task Rejected_with_a_refresh_token_refreshes_immediately()
        {
            using var provider = new OidcTokenProvider(Client());
            await provider.SignInAsync(CancellationToken.None);
            var first = provider.Token;

            provider.Rejected();
            Assert.True(await WaitFor(() => provider.Token != first));
            Assert.Equal(1, _idp.RefreshCount);
            Assert.True(provider.Ready);
        }

        [Fact]
        public async Task Rejected_without_a_refresh_token_signs_out()
        {
            _idp.IssueRefresh = false;
            using var provider = new OidcTokenProvider(Client());
            await provider.SignInAsync(CancellationToken.None);
            provider.Rejected();
            Assert.False(provider.Ready);
            Assert.Contains("refused", provider.Problem);
        }

        [Fact]
        public async Task Sign_out_clears_the_token_and_cancels_renewal()
        {
            _idp.ExpiresIn = 1;
            using var provider = new OidcTokenProvider(Client());
            await provider.SignInAsync(CancellationToken.None);
            var raised = 0;
            provider.Changed += () => Interlocked.Increment(ref raised);

            provider.SignOut();
            Assert.Null(provider.Token);
            Assert.False(provider.Ready);
            Assert.Equal("signed out", provider.Problem);
            Assert.Equal(1, raised);

            await Task.Delay(1500);
            Assert.Equal(0, _idp.RefreshCount);
            Assert.Equal(1, raised);
        }

        [Fact]
        public async Task Sign_in_times_out_when_the_browser_never_returns_and_releases_the_port()
        {
            Uri? seen = null;
            var client = new OidcClient(Settings(), _http, u => seen = u, TimeSpan.FromMilliseconds(500));
            var sw = System.Diagnostics.Stopwatch.StartNew();
            var ex = await Assert.ThrowsAsync<OidcException>(() => client.SignInAsync(CancellationToken.None));
            Assert.Contains("timed out", ex.Message);
            Assert.InRange(sw.ElapsedMilliseconds, 300, 5000);

            Assert.NotNull(seen);
            var redirect = FakeIdp.ParseForm(seen!.Query.TrimStart('?'))["redirect_uri"];
            var port = new Uri(redirect).Port;
            var probe = new TcpListener(IPAddress.Loopback, port);
            probe.Start();
            probe.Stop();
        }

        /// A browser that resolves the redirect's host to `address`.
        private Action<Uri> BrowserAt(string address) => authorize => _ = Task.Run(async () =>
        {
            try
            {
                using var r = await _browserHttp.GetAsync(authorize).ConfigureAwait(false);
                var loc = r.Headers.Location;
                if (loc != null)
                {
                    var at = new Uri("http://" + address + ":" + loc.Port.ToString(CultureInfo.InvariantCulture) + loc.PathAndQuery);
                    using var done = await _http.GetAsync(at).ConfigureAwait(false);
                }
            }
            catch { }
        });

        [Theory]
        [InlineData("127.0.0.1")]
        [InlineData("[::1]")]
        public async Task A_localhost_redirect_is_answered_on_either_loopback_address(string address)
        {
            if (address == "[::1]" && !Socket.OSSupportsIPv6) return;
            var s = Settings();
            s.RedirectHost = " LocalHost ";
            var tokens = await new OidcClient(s, _http, BrowserAt(address), TimeSpan.FromSeconds(10)).SignInAsync(CancellationToken.None);
            Assert.StartsWith("http://localhost:", _idp.LastAuthorize["redirect_uri"]);
            Assert.Equal(_idp.LastAuthorize["redirect_uri"], _idp.LastTokenForm["redirect_uri"]);
            Assert.StartsWith("access-", tokens.AccessToken);
        }

        [Theory]
        [InlineData("login.example.com")]
        [InlineData("127.0.0.2")]
        [InlineData("")]
        public async Task A_redirect_host_off_this_machine_is_refused_before_the_browser_opens(string host)
        {
            Uri? opened = null;
            var s = Settings();
            s.RedirectHost = host;
            var ex = await Assert.ThrowsAsync<OidcException>(() => new OidcClient(s, _http, u => opened = u).SignInAsync(CancellationToken.None));
            Assert.Contains("127.0.0.1 or localhost", ex.Message);
            Assert.Null(opened);
        }

        [Fact]
        public async Task Cancelling_sign_in_throws_operation_canceled_and_releases_the_port()
        {
            using var cts = new CancellationTokenSource(300);
            var client = new OidcClient(Settings(), _http, _ => { }, TimeSpan.FromSeconds(30));
            await Assert.ThrowsAnyAsync<OperationCanceledException>(() => client.SignInAsync(cts.Token));
        }

        [Fact]
        public async Task A_cancel_as_the_browser_opens_throws_operation_canceled()
        {
            // Cancelled before the wait for the redirect starts: the listeners are stopped
            // before their first accept.
            using var cts = new CancellationTokenSource();
            var client = new OidcClient(Settings(), _http, _ => cts.Cancel(), TimeSpan.FromSeconds(30));
            await Assert.ThrowsAnyAsync<OperationCanceledException>(() => client.SignInAsync(cts.Token));
        }
    }
}
