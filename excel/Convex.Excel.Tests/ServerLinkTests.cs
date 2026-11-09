using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Linq;
using System.Net.Http;
using System.Threading;
using System.Threading.Tasks;
using Convex.Excel;
using Convex.Excel.Live;
using Convex.Excel.Ops;
using Convex.Excel.Server;
using ExcelDna.Integration;
using Newtonsoft.Json.Linq;
using Xunit;

namespace Convex.Excel.Tests
{
    /// The server link against FakeConvexServer: real ClientWebSocket, real
    /// HTTP, short timings.
    public sealed class ServerLinkTests : IDisposable
    {
        private readonly FakeConvexServer _server = new();
        private readonly ServerSource _source;
        private readonly IdentifierResolver _resolver;
        private readonly ServerCells _cells;
        private readonly TestTokens _tokens = new("t1");
        private int _nextTopic;

        private static ServerTimings Fast() => new()
        {
            Tick = TimeSpan.FromMilliseconds(20),
            Linger = TimeSpan.FromMilliseconds(300),
            Watchdog = TimeSpan.FromSeconds(5),
            ConnectTimeout = TimeSpan.FromSeconds(5),
            BackoffInitial = TimeSpan.FromMilliseconds(50),
            BackoffMax = TimeSpan.FromMilliseconds(200),
            HealthyAfter = TimeSpan.FromSeconds(30),
            RetryRefused = TimeSpan.FromSeconds(60),
            TooManyRequests = TimeSpan.FromSeconds(60),
            ReseedInterval = TimeSpan.Zero,
        };

        public ServerLinkTests() : this(Fast()) { }

        private ServerLinkTests(ServerTimings timings)
        {
            _source = new ServerSource(new ClientWebSocketTransport(), timings);
            _resolver = new IdentifierResolver(_source, new HttpClient(), timings);
            _cells = new ServerCells(_source, _resolver);
        }

        public void Dispose()
        {
            _source.Dispose();
            _server.Dispose();
        }

        private void Connect(ITokenProvider? tokens = null) =>
            _source.Configure(new ServerConfig { Url = _server.Url, SignIn = SignInMode.Token }, tokens ?? _tokens);

        private static Dictionary<string, object> Price(double clean) => new() { ["clean_price_mid"] = clean, ["ytm_mid"] = 0.0425 };

        private Cell Live(string identifier, string measure = "clean_price_mid", string field = "value")
        {
            var cell = new Cell();
            cell.First = _cells.Connect(Interlocked.Increment(ref _nextTopic),
                ServerCells.TopicStrings(identifier, measure, field), cell);
            cell.Key = string.Join("\u001f", ServerCells.TopicStrings(identifier, measure, field));
            return cell;
        }

        private string? Detail(Cell cell) => _cells.TryGetError(cell.Key, out var e) ? e.Message : null;

        [Fact]
        public async Task A_subscription_seeds_each_cell_then_streams_newer_results()
        {
            _server.Publish(FakeConvexServer.Valuation("BOND-A", "rt-1", 5, Price(99.5)));
            var a = Live("BOND-A");
            var quality = Live("BOND-A", field: "quality");
            var b = Live("BOND-B");
            Assert.Equal(ExcelError.ExcelErrorNA, a.First); // nothing before the stream
            Connect();

            await Eventually(() => a.Last is 99.5);
            Assert.Equal("Valid", quality.Last);
            await Eventually(() => Detail(b)?.Contains("has no result for BOND-B yet") == true);
            Assert.Equal(ExcelError.ExcelErrorNA, b.Last);
            // One batch, both instruments; the bearer token went on the upgrade with no Origin.
            Assert.Equal(new[] { new[] { "BOND-A", "BOND-B" } }, _server.Subscribes().Select(s => s.OrderBy(x => x).ToArray()));
            var upgrade = _server.Upgrades.Single();
            Assert.Equal("Bearer t1", upgrade.Authorization);
            Assert.Null(upgrade.Origin);

            _server.Publish(FakeConvexServer.Valuation("BOND-A", "rt-1", 6, Price(100.25)));
            await Eventually(() => a.Last is 100.25);
            // An older revision of the same runtime is ignored.
            _server.Publish(FakeConvexServer.Valuation("BOND-A", "rt-1", 4, Price(98.0)));
            _server.Publish(FakeConvexServer.Valuation("BOND-B", "rt-1", 7, Price(101.0)));
            await Eventually(() => b.Last is 101.0);
            Assert.Equal(100.25, a.Last);
        }

        [Fact]
        public async Task Detail_fields_read_the_envelope()
        {
            _server.Publish(FakeConvexServer.Valuation("BOND-A", "rt-1", 5, Price(99.5)));
            var revision = Live("BOND-A", field: "revision");
            var status = Live("BOND-A", field: "status");
            var calculated = Live("BOND-A", field: "calculated_at");
            var snapshot = Live("BOND-A", field: "snapshot_id");
            var version = Live("BOND-A", field: "configuration_version");
            var missing = Live("BOND-A", measure: "dv01_mid");
            Connect();
            await Eventually(() => revision.Last is 5.0);
            Assert.Equal("Complete", status.Last);
            Assert.Equal(new DateTime(2026, 11, 7, 10, 0, 1, DateTimeKind.Utc).AddTicks(1234567).ToOADate(), (double)calculated.Last!, 9);
            Assert.Equal("snap-7", snapshot.Last);
            Assert.Equal(3.0, version.Last);
            Assert.Equal(ExcelError.ExcelErrorNA, missing.Last);
            Assert.Contains("no measure dv01_mid; it has clean_price_mid, ytm_mid", Detail(missing));
        }

        [Fact]
        public async Task A_lost_session_keeps_values_marks_them_stale_and_reseeds_on_reconnect()
        {
            _server.Publish(FakeConvexServer.Valuation("BOND-A", "rt-1", 5, Price(99.5)));
            var a = Live("BOND-A");
            var quality = Live("BOND-A", field: "quality");
            Connect();
            await Eventually(() => quality.Last is "Valid");

            _server.Current["BOND-A"] = FakeConvexServer.Valuation("BOND-A", "rt-1", 9, Price(97.0));
            _server.DropAll();
            await Eventually(() => quality.Values.Contains("Stale"));
            Assert.Contains(99.5, a.Values); // the value stayed while disconnected
            await Eventually(() => a.Last is 97.0 && quality.Last is "Valid");
            Assert.True(_server.SessionsOpened >= 2);
        }

        [Fact]
        public async Task A_restarted_server_that_numbers_revisions_afresh_is_followed()
        {
            _server.Publish(FakeConvexServer.Valuation("BOND-A", "rt-1", 50, Price(99.5)));
            var a = Live("BOND-A");
            var quality = Live("BOND-A", field: "quality");
            Connect();
            await Eventually(() => a.Last is 99.5);

            // Same runtime id, revisions from 1 again.
            _server.Current["BOND-A"] = FakeConvexServer.Valuation("BOND-A", "rt-1", 1, Price(98.0));
            _server.DropAll();
            await Eventually(() => a.Last is 98.0 && quality.Last is "Valid");
            _server.Publish(FakeConvexServer.Valuation("BOND-A", "rt-1", 2, Price(98.5)));
            await Eventually(() => a.Last is 98.5);
        }

        [Fact]
        public async Task Silence_past_the_watchdog_reconnects()
        {
            var timings = Fast();
            timings.Watchdog = TimeSpan.FromMilliseconds(600);
            using var test = new ServerLinkTests(timings);
            test._server.Publish(FakeConvexServer.Valuation("BOND-A", "rt-1", 5, Price(99.5)));
            var a = test.Live("BOND-A");
            test.Connect();
            await Eventually(() => a.Last is 99.5);
            test._server.Heartbeats = false;
            await Eventually(() => test._server.SessionsOpened >= 2, TimeSpan.FromSeconds(5));
        }

        [Fact]
        public async Task Resync_required_resubscribes_what_the_session_follows()
        {
            _server.Publish(FakeConvexServer.Valuation("BOND-A", "rt-1", 5, Price(99.5)));
            var a = Live("BOND-A");
            Connect();
            await Eventually(() => a.Last is 99.5);
            _server.Current["BOND-A"] = FakeConvexServer.Valuation("BOND-A", "rt-1", 8, Price(99.75));
            _server.SendAll(new JObject { ["type"] = "error", ["code"] = "RESYNC_REQUIRED", ["message"] = "lagged" });
            await Eventually(() => a.Last is 99.75);
            Assert.Equal(2, _server.Subscribes().Count(s => s.Contains("BOND-A")));
        }

        [Fact]
        public async Task A_refused_batch_is_split_until_the_refused_instrument_stands_alone()
        {
            foreach (var id in new[] { "A1", "A2", "A3", "A4" })
                _server.Publish(FakeConvexServer.Valuation(id, "rt-1", 1, Price(100)));
            _server.Forbidden["A3"] = 0;
            var cells = new[] { "A1", "A2", "A3", "A4" }.Select(id => Live(id)).ToArray();
            Connect();
            await Eventually(() => cells[0].Last is 100.0 && cells[1].Last is 100.0 && cells[3].Last is 100.0);
            await Eventually(() => Detail(cells[2])?.Contains("not entitled to the results of A3") == true);

            // The refusal is remembered: a reconnect does not send A3 again.
            int before = _server.Subscribes().Count;
            _server.DropAll();
            await Eventually(() => _server.Subscribes().Count > before);
            await Task.Delay(200);
            Assert.DoesNotContain(_server.Subscribes().Skip(before), s => s.Contains("A3"));
        }

        [Fact]
        public async Task A_result_too_large_marks_its_cells_and_is_not_resubscribed()
        {
            _server.Publish(FakeConvexServer.Valuation("BIG-1", "rt-1", 3, Price(100)));
            _server.TooLarge["BIG-1"] = 0;
            var big = Live("BIG-1");
            Connect();
            await Eventually(() => Detail(big)?.Contains("larger than one 64 KiB message") == true);
            Assert.Equal(ExcelError.ExcelErrorNA, big.Last);
            await Task.Delay(200);
            Assert.Single(_server.Subscribes());

            // A later result that fits clears it.
            _server.TooLarge.TryRemove("BIG-1", out _);
            _server.Publish(FakeConvexServer.Valuation("BIG-1", "rt-1", 4, Price(100.5)));
            await Eventually(() => big.Last is 100.5);
        }

        [Fact]
        public async Task A_new_runtime_resubscribes_everything()
        {
            _server.Publish(FakeConvexServer.Valuation("BOND-A", "rt-1", 5, Price(99.5)));
            _server.Publish(FakeConvexServer.Valuation("BOND-B", "rt-1", 6, Price(98.5)));
            var a = Live("BOND-A");
            var bQuality = Live("BOND-B", field: "quality");
            Connect();
            await Eventually(() => a.Last is 99.5 && bQuality.Last is "Valid");

            _server.Current["BOND-B"] = FakeConvexServer.Valuation("BOND-B", "rt-2", 1, Price(98.25));
            _server.Publish(FakeConvexServer.Valuation("BOND-A", "rt-2", 1, Price(99.0)));
            await Eventually(() => a.Last is 99.0);
            await Eventually(() => _server.Subscribes().Count >= 2 && bQuality.Last is "Valid");
            Assert.Equal(new[] { "BOND-A", "BOND-B" }, _server.Subscribes()[1].OrderBy(x => x));
        }

        [Fact]
        public async Task Instruments_past_the_session_cap_wait_for_a_free_slot()
        {
            Connect();
            for (int i = 0; i < ServerSource.MaxInstruments; i++) _source.Acquire("I" + i);
            _source.Acquire("EXTRA");
            Assert.Equal(Subscription.OverCap, _source.View("EXTRA").Sub);
            Assert.Contains("at most 256 instruments", _source.View("EXTRA").ProblemMessage);
            await Eventually(() => _source.Status().Instruments == ServerSource.MaxInstruments);
            Assert.All(_server.Subscribes(), s => Assert.True(s.Length <= ServerSource.MaxInstruments));

            _source.Release("I0"); // lingers 300 ms, then frees its slot
            await Eventually(() => _server.Unsubscribes().Any(u => u.Contains("I0")));
            await Eventually(() => _server.Subscribes().Any(s => s.Contains("EXTRA")));
        }

        [Fact]
        public async Task A_released_instrument_lingers_before_it_is_unsubscribed()
        {
            Connect();
            _source.Acquire("BOND-A");
            await Eventually(() => _source.View("BOND-A").Sub == Subscription.Live);
            _source.Release("BOND-A");
            _source.Acquire("BOND-A"); // a recalculation reconnects the topic
            await Task.Delay(500);
            Assert.Empty(_server.Unsubscribes());
            _source.Release("BOND-A");
            await Eventually(() => _server.Unsubscribes().Any(u => u.Contains("BOND-A")));
        }

        [Fact]
        public async Task A_renewed_token_switches_sessions_without_a_stale_gap()
        {
            _server.AcceptTokens = new HashSet<string> { "t1", "t2" };
            _server.Publish(FakeConvexServer.Valuation("BOND-A", "rt-1", 5, Price(99.5)));
            var quality = Live("BOND-A", field: "quality");
            Connect();
            await Eventually(() => quality.Last is "Valid");
            var first = _source.Status().SessionId;

            _tokens.Set("t2");
            await Eventually(() => _source.Status().SessionId != first && _server.OpenSessions == 1);
            Assert.Equal("Bearer t2", _server.Upgrades.Last().Authorization);
            Assert.DoesNotContain("Stale", quality.Values);
            Assert.Equal("Valid", quality.Last);
        }

        [Fact]
        public async Task An_upgrade_refused_with_401_tells_the_token_provider()
        {
            _server.AcceptTokens = new HashSet<string> { "other" };
            var a = Live("BOND-A");
            Connect();
            await Eventually(() => _tokens.Rejections > 0);
            await Eventually(() => Detail(a)?.Contains("refused the sign-in") == true);
        }

        [Fact]
        public async Task A_429_upgrade_waits_for_its_retry_after()
        {
            _server.UpgradeScript.Enqueue((429, 1));
            Connect();
            await Eventually(() => _server.Upgrades.Count >= 2, TimeSpan.FromSeconds(5));
            var times = _server.Upgrades.Select(u => u.At).ToArray();
            Assert.True(times[1] - times[0] >= TimeSpan.FromMilliseconds(900), $"retried after {(times[1] - times[0]).TotalMilliseconds} ms");
        }

        [Fact]
        public async Task Cusips_and_isins_resolve_once_and_are_cached()
        {
            _server.Resolutions["037833100"] = "BOND-A";
            _server.Resolutions["US0378331005"] = "BOND-A";
            _server.Publish(FakeConvexServer.Valuation("BOND-A", "rt-1", 5, Price(99.5)));
            _server.ResolveScript.Enqueue((429, 1)); // the first lookup waits a second
            Connect();
            var byCusip = Live("037833100");
            var byLowercase = Live("037833100", field: "quality");
            await Eventually(() => byCusip.Last is 99.5, TimeSpan.FromSeconds(5));
            var byIsin = Live("us0378331005");
            await Eventually(() => byIsin.Last is 99.5);
            Assert.Equal(3, _server.ResolveCalls); // one 429, then one per identifier
            Assert.Equal("Valid", byLowercase.Last);
            Assert.Single(_server.Subscribes(), s => s.Contains("BOND-A"));

            var unknown = Live("594918104"); // a valid CUSIP the server does not know
            await Eventually(() => Detail(unknown)?.Contains("the server has no CUSIP 594918104") == true);
        }

        [Fact]
        public async Task A_failed_lookup_is_tried_again_once_its_failure_expires_or_after_a_new_sign_in()
        {
            var now = DateTime.UtcNow;
            var resolver = new IdentifierResolver(_source, new HttpClient(), Fast(), () => now);
            var cells = new ServerCells(_source, resolver, retryEvery: TimeSpan.FromMilliseconds(50));
            Cell LiveOn(string identifier)
            {
                var cell = new Cell();
                cell.First = cells.Connect(Interlocked.Increment(ref _nextTopic),
                    ServerCells.TopicStrings(identifier, "clean_price_mid", "value"), cell);
                cell.Key = string.Join("\u001f", ServerCells.TopicStrings(identifier, "clean_price_mid", "value"));
                return cell;
            }
            string? Why(Cell cell) => cells.TryGetError(cell.Key, out var e) ? e.Message : null;

            _server.Publish(FakeConvexServer.Valuation("BOND-A", "rt-1", 5, Price(99.5)));
            _server.Publish(FakeConvexServer.Valuation("BOND-B", "rt-1", 5, Price(101.25)));
            Connect();
            var byCusip = LiveOn("594918104");
            await Eventually(() => Why(byCusip)?.Contains("the server has no CUSIP 594918104") == true);
            Assert.Equal(1, _server.ResolveCalls);

            // The server learns the CUSIP; the failure is held for 5 minutes, so
            // the timer costs no call meanwhile.
            _server.Resolutions["594918104"] = "BOND-A";
            await Task.Delay(300);
            Assert.Equal(1, _server.ResolveCalls);
            Assert.Contains("the server has no CUSIP", Why(byCusip));

            now += TimeSpan.FromMinutes(6);
            await Eventually(() => byCusip.Last is 99.5);
            Assert.Equal(2, _server.ResolveCalls);

            // A refused lookup is tried again at once after a new sign-in, which
            // clears the resolver.
            _server.ResolveScript.Enqueue((403, null));
            var byIsin = LiveOn("US5949181045");
            await Eventually(() => Why(byIsin)?.Contains("your sign-in lacks") == true);
            _server.Resolutions["US5949181045"] = "BOND-B";
            resolver.Clear();
            cells.RetryFailedLookups();
            await Eventually(() => byIsin.Last is 101.25);
            Assert.Equal(4, _server.ResolveCalls);
            Assert.Equal(99.5, byCusip.Last); // a bound cell is left alone
        }

        [Fact]
        public async Task A_snapshot_follows_briefly_and_returns_the_current_result()
        {
            _server.Publish(FakeConvexServer.Valuation("BOND-A", "rt-1", 5, Price(99.5)));
            Connect();
            var view = await _source.SnapshotAsync("BOND-A", TimeSpan.FromSeconds(5), CancellationToken.None);
            Assert.True(view.Fresh);
            Assert.Equal(99.5, ServerFields.Select(view, "clean_price_mid", "value"));
            await Eventually(() => _server.Unsubscribes().Any(u => u.Contains("BOND-A")));

            var none = await _source.SnapshotAsync("BOND-NONE", TimeSpan.FromSeconds(5), CancellationToken.None);
            Assert.True(none.Acked);
            var ex = Assert.Throws<ConvexException>(() => ServerFields.Select(none, "clean_price_mid", "value"));
            Assert.Equal("NO_RESULT", ex.Code);
        }

        [Fact]
        public async Task Status_cells_follow_the_connection()
        {
            var connection = new Cell();
            connection.First = _cells.Connect(1000, ServerCells.StatusTopicStrings("connection"), connection);
            var user = new Cell();
            user.First = _cells.Connect(1001, ServerCells.StatusTopicStrings("user"), user);
            Assert.Equal("Not configured", connection.First);
            Connect();
            await Eventually(() => connection.Last is "Connected");
            Assert.Equal("tester", user.Last ?? user.First);
            _source.Configure(null, _tokens);
            await Eventually(() => connection.Last is "Not configured");
        }

        [Fact]
        public void Malformed_identifiers_and_fields_are_refused_at_once()
        {
            Assert.Throws<ConvexException>(() => _cells.Connect(1, new[] { ServerCells.Kind, "has space", "clean_price_mid", "value" }, new Cell()));
            var field = Assert.Throws<ConvexException>(() => ServerCells.TopicStrings("BOND-A", "clean_price_mid", "grid"));
            Assert.Equal(ErrorCodes.UnknownToken, field.Code);
            Assert.Throws<ConvexException>(() => ServerCells.TopicStrings("BOND-A", "Clean Price", "value"));
            Assert.Equal(new[] { "S1", "BOND-A", "clean_price_mid", "quality" },
                ServerCells.TopicStrings(" BOND-A ", " Clean_Price_Mid ", "QUALITY"));
        }

        [Fact]
        public async Task A_frame_is_timed_to_the_topics_it_changed_and_one_that_changed_none_is_not()
        {
            _server.Publish(FakeConvexServer.Valuation("BOND-A", "rt-1", 5, Price(99.5)));
            var a = Live("BOND-A");
            Connect();
            await Eventually(() => a.Last is 99.5);
            await Eventually(() => _source.Frames.Count >= 3); // the hello, the seed, the acknowledgement
            await Task.Delay(100);
            int timed = _source.Frames.Latency.Count;
            Assert.True(timed >= 1);
            Assert.NotNull(_source.Frames.Latency.Percentile(0.5));

            _server.Publish(FakeConvexServer.Valuation("BOND-A", "rt-1", 4, Price(98.0))); // older: ignored
            _server.Publish(FakeConvexServer.Valuation("BOND-A", "rt-1", 6, Price(100.25)));
            await Eventually(() => a.Last is 100.25);
            await Task.Delay(100);
            Assert.Equal(timed + 1, _source.Frames.Latency.Count);
        }

        [Fact]
        public async Task The_connection_history_shows_a_session_open_and_lost()
        {
            var a = Live("BOND-A");
            Connect();
            await Eventually(() => History($"session ws-1 open ({_server.Url})"));
            _server.DropAll();
            await Eventually(() => History($"session closed ({_server.Url})"));
            await Eventually(() => History($"session ws-2 open ({_server.Url})"));
            Assert.Contains(CxLog.Events.Snapshot(), e => e.Text == "server set to " + _server.Url);
        }

        private static bool History(string start) => CxLog.Events.Snapshot().Any(e => e.Text.StartsWith(start, StringComparison.Ordinal));

        // ------------------------------------------------------------------

        internal static async Task Eventually(Func<bool> condition, TimeSpan? timeout = null)
        {
            var until = DateTime.UtcNow + (timeout ?? TimeSpan.FromSeconds(4));
            while (!condition())
            {
                if (DateTime.UtcNow > until) Assert.Fail("condition not met in time");
                await Task.Delay(20);
            }
        }

        internal sealed class Cell : ITopicSink
        {
            public object? First;
            public string Key = "";
            public readonly ConcurrentQueue<object> Pushed = new();
            public void Push(object value) => Pushed.Enqueue(value);
            public object? Last => Pushed.LastOrDefault() ?? First;
            public IEnumerable<object> Values => new[] { First! }.Concat(Pushed);
        }

        internal sealed class TestTokens : ITokenProvider
        {
            private string? _token;
            private int _rejections;
            public TestTokens(string token) => _token = token;
            public string? Token => Volatile.Read(ref _token);
            public bool Ready => true;
            public string User => "tester";
            public string? Problem => null;
            public int Rejections => Volatile.Read(ref _rejections);
            public event Action? Changed;
            public void Rejected() => Interlocked.Increment(ref _rejections);

            public void Set(string token)
            {
                Volatile.Write(ref _token, token);
                Changed?.Invoke();
            }
        }
    }

    public class ServerWireTests
    {
        [Fact]
        public void Measures_read_numbers_and_exact_decimal_strings()
        {
            var json = FakeConvexServer.Valuation("BOND-A", "rt-1", 5,
                new Dictionary<string, object> { ["clean_price_mid"] = "99.531250", ["dv01_mid"] = 0.0812 });
            json["profile"] = "demo"; // the demo profile adds labels; they are ignored
            json["data_quality"] = "demo";
            var v = Valuation.Parse(Server.ServerJson.Parse(json.ToString()))!;
            Assert.Equal("BOND-A", v.InstrumentId);
            Assert.Equal(5UL, v.Revision);
            Assert.Equal(99.53125, v.Result.Results["clean_price_mid"].Value);
            Assert.Equal(0.0812, v.Result.Results["dv01_mid"].Value);
            Assert.Equal(new DateTime(2026, 11, 7, 10, 0, 1, DateTimeKind.Utc).AddTicks(1234567), v.Result.CalculatedAtUtc);
        }

        [Fact]
        public void A_failed_result_shows_the_last_good_value_with_the_retained_quality()
        {
            var lastGood = FakeConvexServer.Envelope("BOND-A", new Dictionary<string, object> { ["clean_price_mid"] = 99.0 });
            var failed = FakeConvexServer.Valuation("BOND-A", "rt-1", 6, new Dictionary<string, object>(),
                status: "Failed", quality: "Invalid", lastGood: lastGood, retainedQuality: "Stale");
            failed["result"]!["errors"] = new JArray(new JObject
            {
                ["code"] = "MISSING_MARKET_DATA", ["field"] = "ytm_mid", ["message"] = "no quote", ["retryable"] = true,
            });
            var view = new InstrumentView
            {
                InstrumentId = "BOND-A", Sub = Subscription.Live, Fresh = true, Link = LinkState.Connected,
                Latest = Valuation.Parse(Server.ServerJson.Parse(failed.ToString())),
            };
            Assert.Equal(99.0, ServerFields.Select(view, "clean_price_mid", "value"));
            Assert.Equal("Stale", ServerFields.Select(view, "clean_price_mid", "quality"));
            Assert.Equal("Failed", ServerFields.Select(view, "clean_price_mid", "status"));
            var ex = Assert.Throws<ConvexException>(() => ServerFields.Select(view, "ytm_mid", "value"));
            Assert.True(ex.NotAvailable);
            Assert.Equal("MISSING_MARKET_DATA", ex.Code);
            Assert.Equal(ExcelError.ExcelErrorNA, ErrorMapper.ToExcelError(ex));
        }

        [Fact]
        public void A_value_from_a_lost_session_is_stale()
        {
            var view = new InstrumentView
            {
                InstrumentId = "BOND-A", Sub = Subscription.Wanted, Fresh = false, Link = LinkState.Reconnecting,
                Latest = Valuation.Parse(Server.ServerJson.Parse(FakeConvexServer.Valuation("BOND-A", "rt-1", 5,
                    new Dictionary<string, object> { ["clean_price_mid"] = 99.5 }).ToString())),
            };
            Assert.Equal(99.5, ServerFields.Select(view, "clean_price_mid", "value"));
            Assert.Equal("Stale", ServerFields.Select(view, "clean_price_mid", "quality"));
        }

        [Fact]
        public void A_cell_with_no_value_says_why()
        {
            ConvexException Why(LinkState link, bool acked = false) => Assert.Throws<ConvexException>(() =>
                ServerFields.Select(new InstrumentView { InstrumentId = "X", Link = link, Acked = acked, Server = "https://s" }, "m", "value"));
            Assert.Equal("NOT_CONFIGURED", Why(LinkState.NotConfigured).Code);
            Assert.Equal("SIGNED_OUT", Why(LinkState.SignedOut).Code);
            Assert.Equal("CONNECTING", Why(LinkState.Connecting).Code);
            Assert.Equal("DISCONNECTED", Why(LinkState.Reconnecting).Code);
            Assert.Equal("WAITING", Why(LinkState.Connected).Code);
            Assert.Equal("NO_RESULT", Why(LinkState.Connected, acked: true).Code);
        }

        [Fact]
        public void Result_too_large_names_its_instrument_in_the_servers_template()
        {
            // The exact text of crates/convex-server/src/websocket.rs `encode`.
            const string message = "The result of instrument BOND-001 (runtime runtime-1, revision 7) exceeds the " +
                                   "65536-byte message limit; read it from /api/v1/runtime/results";
            Assert.True(TooLarge.TryParse(message, out var id, out var runtime, out var revision));
            Assert.Equal("BOND-001", id);
            Assert.Equal("runtime-1", runtime);
            Assert.Equal(7UL, revision);
            Assert.False(TooLarge.TryParse("something else", out _, out _, out _));
        }

        [Theory]
        [InlineData("037833100", "Cusip")]
        [InlineData("594918104", "Cusip")]
        [InlineData("us0378331005", "Isin")]
        [InlineData("037833101", "Instrument")] // bad check digit: an instrument id
        [InlineData("US0378331006", "Instrument")]
        [InlineData("INSTRUMENT-001", "Instrument")]
        [InlineData("ust:2034/11.15_a", "Instrument")]
        public void Identifiers_are_cusips_and_isins_by_check_digit_else_instrument_ids(string raw, string kind) =>
            Assert.Equal(kind, Identifiers.Classify(raw).Kind.ToString());

        [Theory]
        [InlineData("")]
        [InlineData("-leading-dash")]
        [InlineData("has space")]
        [InlineData("semi;colon")]
        public void Malformed_identifiers_are_refused(string raw) =>
            Assert.Throws<ConvexException>(() => Identifiers.Classify(raw));

        [Fact]
        public void Identifiers_over_128_characters_are_refused() =>
            Assert.Throws<ConvexException>(() => Identifiers.Classify(new string('a', 129)));

        [Theory]
        [InlineData("https://convex.example.com", "wss://convex.example.com/api/v1/ws")]
        [InlineData("https://convex.example.com:8443/", "wss://convex.example.com:8443/api/v1/ws")]
        [InlineData("http://localhost:8080", "ws://localhost:8080/api/v1/ws")]
        [InlineData("http://127.0.0.1:8080", "ws://127.0.0.1:8080/api/v1/ws")]
        public void The_websocket_endpoint_sits_beside_the_server_url(string url, string ws) =>
            Assert.Equal(ws, new ServerConfig { Url = url }.WebSocketUri().AbsoluteUri);

        [Theory]
        [InlineData("http://convex.example.com")] // a token in clear across a network
        [InlineData("ftp://convex.example.com")]
        [InlineData("https://convex.example.com/api")]
        [InlineData("not a url")]
        public void Unsafe_or_malformed_server_urls_are_refused(string url) =>
            Assert.Throws<ConvexException>(() => new ServerConfig { Url = url }.HttpBase());

        [Fact]
        public void The_config_file_round_trips_without_any_token()
        {
            var config = new ServerConfig
            {
                Url = "https://convex.example.com", SignIn = SignInMode.Oidc, Issuer = "https://idp", ClientId = "excel",
                Scope = "openid offline_access convex", Resource = "https://convex.example.com", RedirectHost = "localhost",
            };
            var json = config.ToJson();
            Assert.DoesNotContain("token", json.ToString(), StringComparison.OrdinalIgnoreCase);
            var back = ServerConfig.FromJson(json);
            Assert.Equal(config.ToJson().ToString(), back.ToJson().ToString());
        }
    }
}
