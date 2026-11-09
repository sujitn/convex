using System;
using System.Collections.Generic;
using System.Linq;
using System.Net.Http;
using System.Threading;
using System.Threading.Tasks;
using Convex.Excel;
using Convex.Excel.Server;
using ExcelDna.Integration;
using Newtonsoft.Json.Linq;
using Xunit;
using static Convex.Excel.Tests.ServerLinkTests;

namespace Convex.Excel.Tests
{
    /// CX.LIMIT against FakeConvexServer (Excel plan X4): books follow on the
    /// valuations' session, are read from the status route once subscribed,
    /// and change with each recorded status change.
    public sealed class LimitLinkTests : IDisposable
    {
        private readonly FakeConvexServer _server = new();
        private readonly ServerSource _source;
        private readonly ServerCells _cells;
        private readonly TestTokens _tokens = new("t1");
        private int _nextTopic;

        private const string T0 = "2026-11-09T10:00:00Z";
        private const string T1 = "2026-11-09T10:00:01Z";
        private const string T2 = "2026-11-09T10:00:02Z";

        public LimitLinkTests()
        {
            var timings = new ServerTimings
            {
                Tick = TimeSpan.FromMilliseconds(20),
                Linger = TimeSpan.FromMilliseconds(300),
                Watchdog = TimeSpan.FromSeconds(5),
                ConnectTimeout = TimeSpan.FromSeconds(5),
                BackoffInitial = TimeSpan.FromMilliseconds(50),
                BackoffMax = TimeSpan.FromMilliseconds(200),
                StatusRetry = TimeSpan.FromMilliseconds(50),
                TooManyRequests = TimeSpan.FromSeconds(60),
                ReseedInterval = TimeSpan.Zero,
            };
            var http = new HttpClient();
            _source = new ServerSource(new ClientWebSocketTransport(), timings, http: http);
            _cells = new ServerCells(_source, new IdentifierResolver(_source, http, timings));
        }

        public void Dispose()
        {
            _source.Dispose();
            _server.Dispose();
        }

        private void Connect() =>
            _source.Configure(new ServerConfig { Url = _server.Url, SignIn = SignInMode.Token }, _tokens);

        private Cell Limit(string book, string limit, string field = "status")
        {
            var cell = new Cell();
            var strings = ServerCells.LimitTopicStrings(book, limit, field);
            cell.First = _cells.Connect(Interlocked.Increment(ref _nextTopic), strings, cell);
            cell.Key = string.Join("\u001f", strings);
            return cell;
        }

        private string? Detail(Cell cell) => _cells.TryGetError(cell.Key, out var e) ? e.Message : null;

        private static JObject Dv01(string status, string value, string? utilisation = null) =>
            FakeConvexServer.LimitEval("dv01", status, value, upper: "50000", utilisation: utilisation);

        [Fact]
        public async Task A_book_is_read_once_subscribed_then_follows_each_recorded_change()
        {
            _server.LimitStatus["BOOK-A"] = FakeConvexServer.LimitStatusBody("BOOK-A", 3, 1, T0,
                Dv01("within", "12000", "0.24"),
                FakeConvexServer.LimitEval("face", "within", "1000000", upper: "2000000", measure: "face", member: "BOND-1"));
            var status = Limit("BOOK-A", "dv01");
            var value = Limit("BOOK-A", "dv01", "value");
            var quality = Limit("BOOK-A", "dv01", "quality");
            var record = Limit("BOOK-A", "dv01", "record_id");
            var member = Limit("BOOK-A", "face", "member");
            Assert.Equal(ExcelError.ExcelErrorNA, status.First); // nothing before the server answers
            Connect();

            await Eventually(() => status.Last is "within" && value.Last is 12000.0);
            Assert.Equal("Demo", quality.Last); // the demo server labels its responses
            Assert.Equal("BOND-1", member.Last);
            Assert.Equal(ExcelError.ExcelErrorNA, record.Last);
            Assert.Contains("a status read carries no record id", Detail(record));
            // Subscribed first, then read: the read came after the acknowledgement.
            Assert.Equal(new[] { new[] { "BOOK-A" } }, _server.LimitSubscribes());
            Assert.Equal(new[] { "BOOK-A" }, _server.StatusReads.ToArray());

            _server.PublishLimit(FakeConvexServer.LimitChange("BOOK-A", 41, "within", Dv01("breach", "61000", "1.22"), T1));
            await Eventually(() => status.Last is "breach" && value.Last is 61000.0 && record.Last is 41.0);
            Assert.Single(_server.StatusReads); // a change needs no read
        }

        [Fact]
        public async Task Detail_fields_read_the_limit()
        {
            _server.LimitStatus["BOOK-A"] = FakeConvexServer.LimitStatusBody("BOOK-A", 3, 2, T0,
                FakeConvexServer.LimitEval("krd", "unknown", null, upper: "500", measure: "key_rate_dv01",
                    reason: new JObject
                    {
                        ["code"] = "missing_result",
                        ["instruments"] = new JObject { ["count"] = 3, ["first"] = new JArray("BOND-1", "BOND-2") },
                    }));
            _server.LimitStatus["BOOK-A"]["limits"]![0]!["measure"]!["tenor"] = "5Y";
            var fields = new[] { "upper", "lower", "measure", "reason", "evaluated_at", "book_version", "limit_set_version", "value" }
                .ToDictionary(f => f, f => Limit("BOOK-A", "krd", f));
            Connect();
            await Eventually(() => fields["upper"].Last is 500.0);
            Assert.Equal("key_rate_dv01 5Y", fields["measure"].Last);
            Assert.Equal("missing_result: BOND-1, BOND-2 (3 in all)", fields["reason"].Last);
            Assert.Equal(new DateTime(2026, 11, 9, 10, 0, 0, DateTimeKind.Utc).ToOADate(), fields["evaluated_at"].Last);
            Assert.Equal(3.0, fields["book_version"].Last);
            Assert.Equal(2.0, fields["limit_set_version"].Last);
            Assert.Equal(ExcelError.ExcelErrorNA, fields["lower"].Last);
            Assert.Contains("has no lower bound", Detail(fields["lower"]));
            Assert.Equal(ExcelError.ExcelErrorNA, fields["value"].Last);
            Assert.Contains("is unknown: missing_result", Detail(fields["value"]));
        }

        [Fact]
        public async Task A_change_newer_than_the_read_wins_and_an_older_one_is_ignored()
        {
            _server.StatusGate = new TaskCompletionSource<bool>();
            _server.LimitStatus["BOOK-A"] = FakeConvexServer.LimitStatusBody("BOOK-A", 1, 1, T1, Dv01("warning", "45000"));
            var status = Limit("BOOK-A", "dv01");
            Connect();
            await Eventually(() => _server.StatusReads.Count == 1);
            // A change recorded after the read began arrives first.
            _server.PublishLimit(FakeConvexServer.LimitChange("BOOK-A", 7, "warning", Dv01("breach", "52000"), T2));
            await Eventually(() => status.Last is "breach");
            _server.StatusGate.SetResult(true); // the read (evaluated earlier) answers
            await Task.Delay(300);
            Assert.Equal("breach", status.Last);
            // A change older than what is shown is ignored.
            _server.PublishLimit(FakeConvexServer.LimitChange("BOOK-A", 6, "within", Dv01("warning", "41000"), T0));
            await Task.Delay(300);
            Assert.Equal("breach", status.Last);
        }

        [Fact]
        public async Task A_book_with_no_limit_set_waits_for_one_and_a_limit_not_in_the_set_says_so()
        {
            var status = Limit("BOOK-A", "dv01");
            var missing = Limit("BOOK-A", "gross");
            Connect();
            await Eventually(() => Detail(status)?.Contains("no limit set for book BOOK-A") == true);

            // Committing a set publishes each limit's first status; the book is read again.
            _server.LimitStatus["BOOK-A"] = FakeConvexServer.LimitStatusBody("BOOK-A", 1, 1, T1, Dv01("within", "1000"));
            _server.PublishLimit(FakeConvexServer.LimitChange("BOOK-A", 1, "unevaluated", Dv01("within", "1000"), T1));
            await Eventually(() => status.Last is "within");
            await Eventually(() => Detail(missing)?.Contains("limit set has no limit gross") == true);
            Assert.Equal(2, _server.StatusReads.Count);
        }

        [Fact]
        public async Task A_change_under_a_newer_set_reads_the_book_again_so_a_dropped_limit_goes()
        {
            _server.LimitStatus["BOOK-A"] = FakeConvexServer.LimitStatusBody("BOOK-A", 1, 1, T0,
                Dv01("within", "1000"), FakeConvexServer.LimitEval("mv", "within", "5000"));
            var dv01 = Limit("BOOK-A", "dv01");
            var mv = Limit("BOOK-A", "mv");
            Connect();
            await Eventually(() => dv01.Last is "within" && mv.Last is "within");

            // Set version 2 drops `mv`; the stream says nothing about it.
            _server.LimitStatus["BOOK-A"] = FakeConvexServer.LimitStatusBody("BOOK-A", 1, 2, T1, Dv01("warning", "42000"));
            _server.PublishLimit(FakeConvexServer.LimitChange("BOOK-A", 2, "within", Dv01("warning", "42000"), T1, setVersion: 2));
            await Eventually(() => dv01.Last is "warning");
            await Eventually(() => Detail(mv)?.Contains("version 2 of book BOOK-A's limit set has no limit mv") == true);
        }

        [Fact]
        public async Task A_followed_books_forbidden_is_not_taken_as_a_reply_and_refuses_the_book()
        {
            _server.LimitStatus["BOOK-A"] = FakeConvexServer.LimitStatusBody("BOOK-A", 1, 1, T0, Dv01("within", "1000"));
            _server.Publish(FakeConvexServer.Valuation("BOND-A", "rt-1", 5,
                new Dictionary<string, object> { ["clean_price_mid"] = 99.5 }));
            var status = Limit("BOOK-A", "dv01");
            Connect();
            await Eventually(() => status.Last is "within");

            // The book's FORBIDDEN arrives while a valuation subscription waits for its reply.
            _server.ForbidBookBeforeNextReply = "BOOK-A";
            var price = new Cell();
            price.First = _cells.Connect(Interlocked.Increment(ref _nextTopic),
                ServerCells.TopicStrings("BOND-A", "clean_price_mid", "value"), price);
            await Eventually(() => price.Last is 99.5);
            await Eventually(() => Detail(status)?.Contains("not entitled to the limits of book BOOK-A") == true);
            Assert.Equal(ExcelError.ExcelErrorNA, status.Last);
            await Eventually(() => _server.LimitUnsubscribes().Any(u => u.Contains("BOOK-A")));
            Assert.Equal(1, _server.SessionsOpened); // the session went on

            // Remembered until Reconnect.
            _source.Reconnect();
            await Eventually(() => status.Last is "within");
        }

        [Fact]
        public async Task A_refused_batch_of_books_is_split_until_the_refused_book_stands_alone()
        {
            foreach (var book in new[] { "B1", "B2", "B3" })
                _server.LimitStatus[book] = FakeConvexServer.LimitStatusBody(book, 1, 1, T0, Dv01("within", "1"));
            _server.ForbiddenBooks["B2"] = 0;
            var cells = new[] { "B1", "B2", "B3" }.Select(b => Limit(b, "dv01")).ToArray();
            Connect();
            await Eventually(() => cells[0].Last is "within" && cells[2].Last is "within");
            await Eventually(() => Detail(cells[1])?.Contains("not entitled to the limits of book B2") == true);
            Assert.DoesNotContain("B2", _server.StatusReads);
            Assert.Empty(_server.LimitUnsubscribes()); // a refused subscription follows nothing
        }

        [Fact]
        public async Task A_server_without_a_limit_monitor_says_so_and_instruments_still_follow()
        {
            _server.Monitored = false;
            _server.Publish(FakeConvexServer.Valuation("BOND-A", "rt-1", 5,
                new Dictionary<string, object> { ["clean_price_mid"] = 99.5 }));
            var status = Limit("BOOK-A", "dv01");
            var price = new Cell();
            price.First = _cells.Connect(Interlocked.Increment(ref _nextTopic),
                ServerCells.TopicStrings("BOND-A", "clean_price_mid", "value"), price);
            Connect();
            await Eventually(() => price.Last is 99.5);
            await Eventually(() => Detail(status)?.Contains("does not monitor limits") == true);
        }

        [Fact]
        public async Task A_lost_session_keeps_limits_marks_them_stale_and_reads_again()
        {
            _server.LimitStatus["BOOK-A"] = FakeConvexServer.LimitStatusBody("BOOK-A", 1, 1, T0, Dv01("within", "1000"));
            var status = Limit("BOOK-A", "dv01");
            var quality = Limit("BOOK-A", "dv01", "quality");
            Connect();
            await Eventually(() => status.Last is "within" && quality.Last is "Demo");

            _server.LimitStatus["BOOK-A"] = FakeConvexServer.LimitStatusBody("BOOK-A", 1, 1, T1, Dv01("warning", "41000"));
            _server.DropAll();
            await Eventually(() => quality.Values.Contains("Stale"));
            // The status stayed: nothing after it was first shown is #N/A.
            Assert.DoesNotContain(ExcelError.ExcelErrorNA, status.Values.SkipWhile(v => !"within".Equals(v)));
            await Eventually(() => status.Last is "warning" && quality.Last is "Demo");
            Assert.Equal(2, _server.LimitSubscribes().Count);
        }

        [Fact]
        public async Task Resync_reads_books_for_the_limit_stream_and_resubscribes_for_the_valuation_stream()
        {
            _server.LimitStatus["BOOK-A"] = FakeConvexServer.LimitStatusBody("BOOK-A", 1, 1, T0, Dv01("within", "1000"));
            _server.Publish(FakeConvexServer.Valuation("BOND-A", "rt-1", 5,
                new Dictionary<string, object> { ["clean_price_mid"] = 99.5 }));
            var status = Limit("BOOK-A", "dv01");
            var price = new Cell();
            price.First = _cells.Connect(Interlocked.Increment(ref _nextTopic),
                ServerCells.TopicStrings("BOND-A", "clean_price_mid", "value"), price);
            Connect();
            await Eventually(() => status.Last is "within" && price.Last is 99.5);

            _server.LimitStatus["BOOK-A"] = FakeConvexServer.LimitStatusBody("BOOK-A", 1, 1, T1, Dv01("breach", "60000"));
            _server.SendAll(new JObject { ["type"] = "error", ["code"] = "RESYNC_REQUIRED", ["message"] = FakeConvexServer.LimitsResyncText });
            await Eventually(() => status.Last is "breach");
            await Task.Delay(200);
            Assert.Single(_server.Subscribes()); // valuations were not resubscribed

            _server.SendAll(new JObject { ["type"] = "error", ["code"] = "RESYNC_REQUIRED", ["message"] = FakeConvexServer.ValuationsResyncText });
            await Eventually(() => _server.Subscribes().Count == 2);
            await Task.Delay(200);
            Assert.Equal(2, _server.StatusReads.Count); // books were not read again
            Assert.Single(_server.LimitSubscribes()); // and never resubscribed on the same session
        }

        [Fact]
        public async Task Books_past_the_session_cap_wait_for_a_free_slot()
        {
            Connect();
            for (int i = 0; i < ServerSource.MaxBooks; i++) _source.AcquireBook("K" + i);
            _source.AcquireBook("EXTRA");
            Assert.Equal(Subscription.OverCap, _source.LimitView("EXTRA", "x").Sub);
            Assert.Contains("at most 64 books", _source.LimitView("EXTRA", "x").ProblemMessage);
            await Eventually(() => _source.Status().Books == ServerSource.MaxBooks);

            _source.ReleaseBook("K0"); // lingers 300 ms, then frees its slot
            await Eventually(() => _server.LimitUnsubscribes().Any(u => u.Contains("K0")));
            await Eventually(() => _server.LimitSubscribes().Any(s => s.Contains("EXTRA")));
        }

        [Fact]
        public async Task A_failed_read_is_retried_and_a_429_waits_for_its_retry_after()
        {
            _server.LimitStatus["BOOK-A"] = FakeConvexServer.LimitStatusBody("BOOK-A", 1, 1, T0, Dv01("within", "1000"));
            _server.StatusScript.Enqueue((503, null));
            _server.StatusScript.Enqueue((429, 1));
            var status = Limit("BOOK-A", "dv01");
            Connect();
            await Eventually(() => _server.StatusReads.Count >= 2);
            await Eventually(() => Detail(status)?.Contains("too many requests") == true);
            var asked = DateTime.UtcNow;
            await Eventually(() => status.Last is "within", TimeSpan.FromSeconds(5));
            Assert.True(DateTime.UtcNow - asked >= TimeSpan.FromMilliseconds(800));
            Assert.Equal(3, _server.StatusReads.Count);
        }

        [Fact]
        public async Task A_renewed_token_takes_the_books_to_the_new_session_without_a_stale_gap()
        {
            _server.AcceptTokens = new HashSet<string> { "t1", "t2" };
            _server.LimitStatus["BOOK-A"] = FakeConvexServer.LimitStatusBody("BOOK-A", 1, 1, T0, Dv01("within", "1000"));
            var quality = Limit("BOOK-A", "dv01", "quality");
            var status = Limit("BOOK-A", "dv01");
            Connect();
            await Eventually(() => quality.Last is "Demo");
            var first = _source.Status().SessionId;

            _tokens.Set("t2");
            await Eventually(() => _source.Status().SessionId != first && _server.OpenSessions == 1);
            Assert.DoesNotContain("Stale", quality.Values);
            Assert.Equal(2, _server.LimitSubscribes().Count); // the standby followed the book first
            _server.PublishLimit(FakeConvexServer.LimitChange("BOOK-A", 9, "within", Dv01("warning", "42000"), T1));
            await Eventually(() => status.Last is "warning");
        }

        [Fact]
        public async Task A_reply_of_the_wrong_subscription_type_ends_the_session()
        {
            _server.WrongLimitsReply = true;
            Limit("BOOK-A", "dv01");
            Connect();
            await Eventually(() => _server.SessionsOpened >= 2);
        }

        [Fact]
        public async Task The_check_route_sends_signed_changes_and_reads_the_answer()
        {
            _server.CheckAnswers["BOOK-A"] = (200, FakeConvexServer.LimitStatusBody("BOOK-A", 4, 2, T1, Dv01("breach", "61000", "1.22")));
            using var http = new HttpClient();
            var rows = new[]
            {
                new LimitChangeRow { Identifier = "BOND-1", Quantity = LimitChangeRow.ParseQuantity(-250000.0) },
                new LimitChangeRow { Identifier = "037833100", Quantity = LimitChangeRow.ParseQuantity("1,000,000"), Rating = "A" },
            };
            var answer = await LimitRoutes.CheckAsync(http, new Uri(_server.Url + "/"), "t1", "BOOK-A", rows, CancellationToken.None);
            Assert.NotNull(answer.Read);
            Assert.Equal("breach", answer.Read!.Limits.Single().Status);
            Assert.True(answer.Read.Limits.Single().Demo);
            var sent = _server.CheckBodies.Single().Body["changes"]!;
            Assert.Equal("BOND-1", (string)sent[0]!["instrument_id"]!);
            Assert.Equal("-250000", (string)sent[0]!["quantity"]!);
            Assert.Equal("cusip", (string)sent[1]!["identifier"]!["kind"]!);
            Assert.Equal("1000000", (string)sent[1]!["quantity"]!);
            Assert.Equal("A", (string)sent[1]!["rating"]!);

            _server.CheckAnswers["BOOK-A"] = (422, new JObject
            {
                ["error"] = "a limit check row was rejected", ["code"] = "BOOK_ROWS_REJECTED",
                ["rows"] = new JArray(new JObject { ["index"] = 1, ["code"] = "INVALID_ROW", ["reason"] = "a change keeps the held position's rating" }),
            });
            answer = await LimitRoutes.CheckAsync(http, new Uri(_server.Url + "/"), "t1", "BOOK-A", rows, CancellationToken.None);
            Assert.Null(answer.Read);
            Assert.Equal("BOOK_ROWS_REJECTED", answer.Code);
            Assert.Equal((1, "INVALID_ROW: a change keeps the held position's rating"), answer.Rows.Single());
        }
    }

    public class LimitWireTests
    {
        [Fact]
        public void A_limit_status_message_reads_its_change()
        {
            var message = FakeConvexServer.LimitChange("BOOK-A", 41, "within",
                FakeConvexServer.LimitEval("issuer", "warning", "0.31", upper: "0.35", utilisation: "0.8857", measure: "group_share", member: "ISSUER-9"),
                "2026-11-09T10:00:01.123456789Z", setVersion: 3, bookVersion: 7);
            message["measure"]!["by"] = "issuer";
            message["measure"]!["group"] = "each";
            message["warning_fraction"] = "0.8";
            message["acknowledgement"] = new JObject { ["by"] = "risk" }; // a later server field is ignored
            var (book, item) = LimitItem.ParseChange(ServerJson.Parse(message.ToString()))!.Value;
            Assert.Equal("BOOK-A", book);
            Assert.Equal("issuer", item.LimitId);
            Assert.Equal("warning", item.Status);
            Assert.Equal("group_share issuer each", item.Measure);
            Assert.Equal(0.31, item.Value);
            Assert.Equal(0.8857, item.Utilisation);
            Assert.Equal(0.35, item.Upper);
            Assert.Null(item.Lower);
            Assert.Equal(0.8, item.WarningFraction);
            Assert.Equal("ISSUER-9", item.Member);
            Assert.Equal("within", item.Previous);
            Assert.Equal(41UL, item.RecordId);
            Assert.Equal(7UL, item.BookVersion);
            Assert.Equal(3UL, item.LimitSetVersion);
            Assert.Equal(new DateTime(2026, 11, 9, 10, 0, 1, DateTimeKind.Utc).AddTicks(1234567), item.EvaluatedAtUtc);
        }

        [Fact]
        public void Measures_and_reasons_read_as_text()
        {
            Assert.Equal("face BOND-1", LimitItem.MeasureText(JObject.Parse("{\"kind\":\"face\",\"instrument\":{\"one\":\"BOND-1\"}}")));
            Assert.Equal("face each", LimitItem.MeasureText(JObject.Parse("{\"kind\":\"face\",\"instrument\":\"each\"}")));
            Assert.Equal("dv01", LimitItem.MeasureText(JObject.Parse("{\"kind\":\"dv01\"}")));
            Assert.Equal("quote_refused: BOND-1: stale quote",
                LimitItem.ReasonText(JObject.Parse("{\"code\":\"quote_refused\",\"instrument\":\"BOND-1\",\"detail\":\"stale quote\"}")));
            Assert.Equal("measure_not_computed: cs01_mid: BOND-2",
                LimitItem.ReasonText(JObject.Parse("{\"code\":\"measure_not_computed\",\"measure\":\"cs01_mid\",\"instruments\":{\"count\":1,\"first\":[\"BOND-2\"]}}")));
            Assert.Equal("zero_net_value", LimitItem.ReasonText(JObject.Parse("{\"code\":\"zero_net_value\"}")));
        }

        [Fact]
        public void Limit_errors_are_told_apart_by_the_server_text()
        {
            Assert.True(LimitErrors.TryParseForbiddenBook(
                "limit status of book BOOK-7 needs read access to each instrument it holds or a limit names", out var book));
            Assert.Equal("BOOK-7", book);
            Assert.False(LimitErrors.TryParseForbiddenBook("FORBIDDEN", out _));
            Assert.True(LimitErrors.IsLimitsResync(FakeConvexServer.LimitsResyncText));
            Assert.False(LimitErrors.IsValuationsResync(FakeConvexServer.LimitsResyncText));
            Assert.True(LimitErrors.IsValuationsResync(FakeConvexServer.ValuationsResyncText));
        }

        [Fact]
        public void The_ticket_sets_each_limit_now_beside_after_the_trade()
        {
            LimitBookStatus Read(params JObject[] limits) =>
                LimitBookStatus.Parse(ServerJson.Parse(FakeConvexServer.LimitStatusBody("BOOK-A", 1, 1, "2026-11-09T10:00:00Z", limits).ToString()), false)!;
            var now = Read(
                FakeConvexServer.LimitEval("dv01", "within", "12000", upper: "50000", utilisation: "0.24"),
                FakeConvexServer.LimitEval("old", "within", "1"));
            var after = Read(
                FakeConvexServer.LimitEval("dv01", "breach", "61000", upper: "50000", utilisation: "1.22"),
                FakeConvexServer.LimitEval("face", "warning", "900000", upper: "1000000"));
            var rows = LimitTicket.Compare(now, after);
            Assert.Equal(new[] { "dv01", "face", "old" }, rows.Select(r => r.LimitId));
            Assert.True(rows[0].Worse);
            Assert.Equal("within", rows[0].Now!.Status);
            Assert.Null(rows[1].Now); // only after (the set changed between the reads)
            Assert.False(rows[1].Worse);
            Assert.Null(rows[2].After);
            Assert.False(rows[2].Worse);
            // The current status could not be read: nothing is called worse.
            Assert.All(LimitTicket.Compare(null, after), r => Assert.False(r.Worse));
            Assert.Equal("≤ 50,000", rows[0].Bounds);
            var both = new LimitItem { Lower = -50000, Upper = 50000, WarningFraction = 0.8 };
            Assert.Equal("[-50,000, 50,000] (warn 80%)", LimitTicket.Bounds(both));
            Assert.Equal("122.0%", LimitTicket.Percent(1.22));
            Assert.Equal("0.31", LimitTicket.Number(0.31));
        }

        [Fact]
        public void Topic_strings_and_quantities_are_checked()
        {
            Assert.Equal(new[] { "S1L", "BOOK-A", "dv01", "status" }, ServerCells.LimitTopicStrings(" BOOK-A ", "dv01", " Status "));
            Assert.Throws<ConvexException>(() => ServerCells.LimitTopicStrings("BOOK A", "dv01", "status"));
            Assert.Throws<ConvexException>(() => ServerCells.LimitTopicStrings("BOOK-A", "dv01", "colour"));
            Assert.Equal("-250000.5", LimitChangeRow.ParseQuantity(" -250,000.5 "));
            Assert.Equal("1000000", LimitChangeRow.ParseQuantity("1E6"));
            Assert.Throws<ConvexException>(() => LimitChangeRow.ParseQuantity("0"));
            Assert.Throws<ConvexException>(() => LimitChangeRow.ParseQuantity("ten"));
        }
    }
}
