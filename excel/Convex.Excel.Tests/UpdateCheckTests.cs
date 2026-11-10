using System;
using System.Collections.Concurrent;
using System.IO;
using System.Linq;
using System.Net;
using System.Net.Http;
using System.Net.Sockets;
using System.Threading;
using System.Threading.Tasks;
using Convex.Excel.Ops;
using Xunit;

namespace Convex.Excel.Tests
{
    public class UpdateCheckTests : IDisposable
    {
        private const string GitHubPage = "https://github.com/sujitn/convex/releases/tag/";
        private static readonly DateTime Noon = new(2026, 11, 10, 12, 0, 0, DateTimeKind.Utc);
        private static readonly CancellationToken None = CancellationToken.None;

        private readonly string _dir = Path.Combine(Path.GetTempPath(), "convex-update-" + Guid.NewGuid().ToString("N"));
        private readonly HttpClient _http = UpdateFeed.NewClient();

        public void Dispose()
        {
            _http.Dispose();
            try { Directory.Delete(_dir, true); } catch { }
        }

        private static UpdateConfig On(string feed = UpdateConfig.GitHubFeed) => new(UpdateSwitch.On, feed, false);

        private static UpdateResult Found(string latest, DateTime? at = null, string feed = UpdateConfig.GitHubFeed) =>
            new(at ?? Noon, feed, latest, GitHubPage + latest, null);

        // ---- Where the switch and the feed come from.

        [Theory]
        [InlineData(null, null, true, "On")]
        [InlineData("", "", true, "On")]
        [InlineData(1, 1, true, "On")]
        [InlineData(0, null, true, "OffByPolicy")]
        [InlineData("0", 1, true, "OffByPolicy")]
        [InlineData(0, 0, false, "OffByPolicy")]
        [InlineData(null, 0, true, "OffByInstaller")]
        [InlineData(1, " 0 ", true, "OffByInstaller")]
        [InlineData(null, 0, false, "OffByInstaller")]
        [InlineData(null, null, false, "OffBySettings")]
        [InlineData(1, 1, false, "OffBySettings")]
        public void Policy_then_the_installer_then_Settings_decide(object? policy, object? installer, bool settings, string expected) =>
            Assert.Equal(expected, UpdateConfig.Resolve(policy, installer, settings, null, null).Switch.ToString());

        [Theory]
        [InlineData(null, null, UpdateConfig.GitHubFeed, false)]
        [InlineData(null, "  ", UpdateConfig.GitHubFeed, false)]
        [InlineData(null, " https://mirror.example/latest ", "https://mirror.example/latest", false)]
        [InlineData("", "https://mirror.example/latest", "https://mirror.example/latest", false)]
        [InlineData("https://policy.example/latest", "https://mirror.example/latest", "https://policy.example/latest", true)]
        public void Policy_then_Settings_choose_the_feed(string? policyFeed, string? settingsFeed, string feed, bool byPolicy)
        {
            var config = UpdateConfig.Resolve(null, null, true, policyFeed, settingsFeed);
            Assert.Equal(feed, config.Feed);
            Assert.Equal(byPolicy, config.FeedByPolicy);
        }

        // ---- What a release page names, and what counts as newer.

        [Theory]
        [InlineData(GitHubPage + "0.14.0", "0.14.0")]
        [InlineData(GitHubPage + "v0.15.1", "0.15.1")]
        [InlineData("https://mirror.example/convex/v1.2.3/", "1.2.3")]
        [InlineData(GitHubPage + "0.15.0-rc.1", null)]
        [InlineData(GitHubPage + "convex-0.15.0", null)]
        [InlineData(GitHubPage + "0.15", null)]
        [InlineData("https://github.com/sujitn/convex/releases", null)]
        public void A_release_page_names_its_version_last(string page, string? version) =>
            Assert.Equal(version, UpdateFeed.VersionOf(new Uri(page))?.ToString(3));

        [Theory]
        [InlineData("0.15.0", "0.14.0", true)]
        [InlineData("0.14.1", "0.14.0", true)]
        [InlineData("1.0.0", "0.14.0", true)]
        [InlineData("0.10.0", "0.9.0", true)]
        [InlineData("0.14.0", "0.14.0", false)]
        [InlineData("0.13.9", "0.14.0", false)]
        public void The_button_shows_only_for_a_newer_release(string latest, string running, bool newer)
        {
            var status = new UpdateStatus(On(), Found(latest), running);
            Assert.Equal(newer, status.Newer);
            Assert.Equal(newer ? GitHubPage + latest : null, status.Page);
        }

        [Fact]
        public void A_newer_release_stays_hidden_when_the_check_is_off_or_read_another_feed()
        {
            Assert.False(new UpdateStatus(new UpdateConfig(UpdateSwitch.OffByPolicy, UpdateConfig.GitHubFeed, false), Found("9.0.0"), "0.14.0").Newer);
            Assert.False(new UpdateStatus(new UpdateConfig(UpdateSwitch.OffBySettings, UpdateConfig.GitHubFeed, false), Found("9.0.0"), "0.14.0").Newer);
            Assert.False(new UpdateStatus(On("https://mirror.example/latest"), Found("9.0.0"), "0.14.0").Newer);
        }

        [Fact]
        public void Diag_says_what_the_check_knows()
        {
            Assert.Equal("not started", UpdateStatus.NotStarted.Describe());
            Assert.Equal("off (policy)", new UpdateStatus(new UpdateConfig(UpdateSwitch.OffByPolicy, UpdateConfig.GitHubFeed, false), Found("9.0.0"), "0.14.0").Describe());
            Assert.Equal("off (installer)", new UpdateStatus(new UpdateConfig(UpdateSwitch.OffByInstaller, UpdateConfig.GitHubFeed, false), null, "0.14.0").Describe());
            Assert.Equal("off (Settings)", new UpdateStatus(new UpdateConfig(UpdateSwitch.OffBySettings, UpdateConfig.GitHubFeed, false), null, "0.14.0").Describe());
            Assert.Equal("on, not checked yet", new UpdateStatus(On(), null, "0.14.0").Describe());
            Assert.Equal("0.15.0 available (checked 2026-11-10 12:00Z)", new UpdateStatus(On(), Found("0.15.0"), "0.14.0").Describe());
            Assert.Equal("up to date: the latest is 0.14.0 (checked 2026-11-10 12:00Z)", new UpdateStatus(On(), Found("0.14.0"), "0.14.0").Describe());
            var failed = new UpdateResult(Noon, UpdateConfig.GitHubFeed, null, null, "the feed has no release (HTTP 404)");
            Assert.Equal("failed: the feed has no release (HTTP 404) (checked 2026-11-10 12:00Z)", new UpdateStatus(On(), failed, "0.14.0").Describe());
        }

        [Fact]
        public void The_feed_is_due_once_a_day_or_when_it_changes()
        {
            var last = Found("0.15.0");
            Assert.True(UpdateChecker.IsDue(null, UpdateConfig.GitHubFeed, Noon));
            Assert.False(UpdateChecker.IsDue(last, UpdateConfig.GitHubFeed, Noon.AddHours(23.9)));
            Assert.True(UpdateChecker.IsDue(last, UpdateConfig.GitHubFeed, Noon.AddDays(1)));
            Assert.True(UpdateChecker.IsDue(last, "https://mirror.example/latest", Noon.AddMinutes(1)));
            // A clock set back must not stop the check for good.
            Assert.True(UpdateChecker.IsDue(last, UpdateConfig.GitHubFeed, Noon.AddMinutes(-1)));
        }

        // ---- The feed over HTTP.

        [Fact]
        public async Task The_redirect_is_read_and_not_followed()
        {
            using var feed = new FeedServer(_ => (302, "/sujitn/convex/releases/tag/0.15.0"));
            var result = await UpdateFeed.ReadAsync(_http, feed.Url("/latest"), "0.14.0", Noon, None);

            Assert.Null(result.Failure);
            Assert.Equal("0.15.0", result.Latest);
            Assert.Equal(feed.Url("/sujitn/convex/releases/tag/0.15.0"), result.Page);
            Assert.Equal(new[] { "/latest" }, feed.Paths.ToArray());
            Assert.Equal("Convex-Excel/0.14.0", feed.UserAgents.Single());
        }

        [Theory]
        [InlineData(404, null, "the feed has no release (HTTP 404)")]
        [InlineData(200, null, "the feed answered HTTP 200 without a redirect")]
        [InlineData(500, null, "the feed answered HTTP 500 without a redirect")]
        [InlineData(302, "/sujitn/convex/releases/tag/latest", "which names no version")]
        [InlineData(302, "file:///C:/Windows/0.15.0", "not a web page")]
        public async Task A_feed_that_does_not_redirect_to_a_version_fails(int status, string? location, string why)
        {
            using var feed = new FeedServer(_ => (status, location));
            var result = await UpdateFeed.ReadAsync(_http, feed.Url("/latest"), "0.14.0", Noon, None);

            Assert.Contains(why, result.Failure);
            Assert.Null(result.Latest);
            Assert.False(new UpdateStatus(On(feed.Url("/latest")), result, "0.14.0").Newer);
        }

        [Fact]
        public async Task An_unreachable_or_unusable_feed_fails_without_throwing()
        {
            var closed = new TcpListener(IPAddress.Loopback, 0);
            closed.Start();
            var port = ((IPEndPoint)closed.LocalEndpoint).Port;
            closed.Stop();

            var unreachable = await UpdateFeed.ReadAsync(_http, $"http://127.0.0.1:{port}/latest", "0.14.0", Noon, None);
            Assert.NotNull(unreachable.Failure);

            var ftp = await UpdateFeed.ReadAsync(_http, "ftp://mirror.example/latest", "0.14.0", Noon, None);
            Assert.Equal("the feed is not an http or https address", ftp.Failure);
        }

        // ---- The checker: once a day, shared by every Excel the user runs.

        private UpdateCache Cache() => new(Path.Combine(_dir, "update-check.json"));

        [Fact]
        public async Task A_restarted_Excel_uses_the_days_result_and_asks_again_a_day_later()
        {
            using var feed = new FeedServer(_ => (302, "/tag/0.15.0"));
            var now = Noon;
            UpdateChecker Excel() => new(() => On(feed.Url("/latest")), Cache(), _http, () => now, "0.14.0");

            var first = Excel();
            await first.RunAsync(ask: false, None);
            Assert.Empty(feed.Paths);
            Assert.Equal("on, not checked yet", first.Status.Describe());

            await first.RunAsync(ask: true, None);
            Assert.Single(feed.Paths);
            Assert.True(first.Status.Newer);
            Assert.Equal(feed.Url("/tag/0.15.0"), first.Status.Page);

            now = Noon.AddHours(23);
            var second = Excel();
            await second.RunAsync(ask: false, None);
            Assert.True(second.Status.Newer);
            await second.RunAsync(ask: true, None);
            await first.RunAsync(ask: true, None);
            Assert.Single(feed.Paths);

            now = Noon.AddDays(1);
            await second.RunAsync(ask: true, None);
            Assert.Equal(2, feed.Paths.Count);
            // The first Excel reads the second's result from the file instead of asking.
            await first.RunAsync(ask: true, None);
            Assert.Equal(2, feed.Paths.Count);
        }

        [Fact]
        public async Task A_check_turned_off_never_asks_and_hides_a_known_release()
        {
            using var feed = new FeedServer(_ => (302, "/tag/0.15.0"));
            var cache = Cache();
            cache.Write(Found("0.15.0", Noon, feed.Url("/latest")));
            var config = new UpdateConfig(UpdateSwitch.OffByInstaller, feed.Url("/latest"), false);
            var checker = new UpdateChecker(() => config, cache, _http, () => Noon.AddDays(3), "0.14.0");

            await checker.RunAsync(ask: true, None);

            Assert.Empty(feed.Paths);
            Assert.False(checker.Status.Newer);
            Assert.Equal("off (installer)", checker.Status.Describe());
        }

        [Fact]
        public async Task A_failure_also_waits_a_day()
        {
            using var feed = new FeedServer(_ => (500, null));
            var now = Noon;
            var checker = new UpdateChecker(() => On(feed.Url("/latest")), Cache(), _http, () => now, "0.14.0");

            await checker.RunAsync(ask: true, None);
            now = Noon.AddHours(12);
            await checker.RunAsync(ask: true, None);

            Assert.Single(feed.Paths);
            Assert.StartsWith("failed: the feed answered HTTP 500", checker.Status.Describe());
        }

        [Fact]
        public async Task The_ribbon_hears_when_a_newer_release_appears()
        {
            var tag = "0.14.0";
            using var feed = new FeedServer(_ => (302, "/tag/" + tag));
            var now = Noon;
            var checker = new UpdateChecker(() => On(feed.Url("/latest")), Cache(), _http, () => now, "0.14.0");
            var heard = 0;
            checker.Changed += () => heard++;

            await checker.RunAsync(ask: true, None);
            Assert.False(checker.Status.Newer);
            var afterFirst = heard;

            now = Noon.AddDays(1);
            tag = "0.15.0";
            await checker.RunAsync(ask: true, None);
            Assert.True(checker.Status.Newer);
            Assert.Equal(afterFirst + 1, heard);
        }

        [Fact]
        public void The_result_file_round_trips_and_a_damaged_one_makes_the_check_due()
        {
            var cache = Cache();
            Assert.Null(cache.Read());

            var found = new UpdateResult(Noon.AddTicks(1234567), UpdateConfig.GitHubFeed, "0.15.0", GitHubPage + "0.15.0", null);
            cache.Write(found);
            var read = cache.Read()!;
            Assert.Equal(found.CheckedUtc, read.CheckedUtc);
            Assert.Equal(DateTimeKind.Utc, read.CheckedUtc.Kind);
            Assert.Equal(found.Feed, read.Feed);
            Assert.Equal(found.Latest, read.Latest);
            Assert.Equal(found.Page, read.Page);

            File.WriteAllText(cache.Path, "{ not json");
            Assert.Null(cache.Read());

            // A page that is not a web address is not trusted to the button.
            cache.Write(new UpdateResult(Noon, UpdateConfig.GitHubFeed, "0.15.0", "file:///C:/Windows/0.15.0", null));
            Assert.Null(cache.Read());
        }

        /// A feed on HttpListener: answers each request with a status and a Location.
        private sealed class FeedServer : IDisposable
        {
            private readonly HttpListener _listener = new();
            private readonly Func<string, (int Status, string? Location)> _answer;

            public FeedServer(Func<string, (int Status, string? Location)> answer)
            {
                _answer = answer;
                var probe = new TcpListener(IPAddress.Loopback, 0);
                probe.Start();
                Port = ((IPEndPoint)probe.LocalEndpoint).Port;
                probe.Stop();
                _listener.Prefixes.Add($"http://localhost:{Port}/");
                _listener.Start();
                _ = Task.Run(LoopAsync);
            }

            public int Port { get; }
            public ConcurrentQueue<string> Paths { get; } = new();
            public ConcurrentQueue<string> UserAgents { get; } = new();

            public string Url(string path) => $"http://localhost:{Port}{path}";

            private async Task LoopAsync()
            {
                while (true)
                {
                    HttpListenerContext context;
                    try { context = await _listener.GetContextAsync().ConfigureAwait(false); }
                    catch { return; }
                    var path = context.Request.Url!.AbsolutePath;
                    UserAgents.Enqueue(context.Request.UserAgent ?? "");
                    Paths.Enqueue(path);
                    var (status, location) = _answer(path);
                    context.Response.StatusCode = status;
                    if (location != null) context.Response.RedirectLocation = location;
                    context.Response.Close();
                }
            }

            public void Dispose()
            {
                try { _listener.Stop(); _listener.Close(); } catch { }
            }
        }
    }
}
