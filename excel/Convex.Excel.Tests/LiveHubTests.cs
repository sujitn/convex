using System;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;
using Convex.Excel;
using Convex.Excel.Live;
using ExcelDna.Integration;
using Newtonsoft.Json.Linq;
using Xunit;

namespace Convex.Excel.Tests
{
    public class LocalSpecTests
    {
        private static JObject Price(JToken bond, JToken? curve = null)
        {
            var req = new JObject { ["bond"] = bond, ["settlement"] = "2025-04-15", ["mark"] = "99.5C" };
            if (curve != null) req["curve"] = curve;
            return req;
        }

        [Fact]
        public void Topic_strings_round_trip_and_name_the_references_read()
        {
            var spec = LocalSpec.Create("price", "clean", Price("912828YK0", 101UL));
            var strings = spec.ToTopicStrings();
            Assert.Equal(new[] { "L1", "price", "clean" }, strings.Take(3));

            var back = LocalSpec.FromTopicStrings(strings);
            Assert.Equal(spec.RequestJson, back.RequestJson);
            Assert.Equal(new[] { "912828YK0" }, back.Names);
            Assert.Equal(new[] { 101UL }, back.Handles);
        }

        [Fact]
        public void Spread_reads_the_forward_and_government_curves_in_params()
        {
            var req = new JObject
            {
                ["bond"] = 100UL, ["curve"] = "USD.SOFR", ["settlement"] = "2025-04-15", ["mark"] = "99.5C",
                ["spread_type"] = "G",
                ["params"] = new JObject { ["forward_curve"] = "USD.FWD", ["govt_curve"] = 205UL },
            };
            var spec = LocalSpec.Create("spread", "bps", req);
            Assert.Equal(new[] { "USD.SOFR", "USD.FWD" }, spec.Names);
            Assert.Equal(new[] { 100UL, 205UL }, spec.Handles);
        }

        [Fact]
        public void Long_requests_split_into_255_character_strings_without_breaking_a_surrogate_pair()
        {
            // 254 ASCII characters into the JSON, then an emoji (a surrogate pair).
            var name = new string('x', 254 - "{\"bond\":\"".Length) + "\U0001F600" + new string('y', 600);
            var spec = LocalSpec.Create("price", "clean", new JObject { ["bond"] = name });
            var strings = spec.ToTopicStrings();
            Assert.All(strings, s => Assert.True(s.Length <= 255));
            Assert.All(strings.Skip(3), s => Assert.False(char.IsHighSurrogate(s[s.Length - 1])));
            Assert.Equal(spec.RequestJson, string.Concat(strings.Skip(3)));
            Assert.Equal(new[] { name }, LocalSpec.FromTopicStrings(strings).Names);
        }

        [Fact]
        public void A_request_too_long_for_rtd_is_refused()
        {
            var spec = LocalSpec.Create("price", "clean", new JObject { ["bond"] = new string('x', 70_000) });
            Assert.Throws<ConvexException>(() => spec.ToTopicStrings());
        }

        [Fact]
        public void Verbs_without_a_live_form_and_foreign_topics_are_refused()
        {
            Assert.Throws<ConvexException>(() => LocalSpec.Create("cashflows", "x", new JObject()));
            Assert.Throws<ConvexException>(() => LocalSpec.FromTopicStrings(new[] { "S1", "price", "clean", "{}" }));
        }
    }

    public class LiveFieldsTests
    {
        [Theory]
        [InlineData("price", "Clean", "clean")]
        [InlineData("risk", "modified_duration", "mod_dur")]
        [InlineData("risk", "SPREAD_DURATION", "spread_dur")]
        [InlineData("spread", "bps", "bps")]
        [InlineData("yas", "z_spread", "z_spread")]
        public void Aliases_canonicalise_so_equal_cells_share_a_topic(string verb, string field, string canonical) =>
            Assert.Equal(canonical, LiveFields.Validate(verb, field));

        [Theory]
        [InlineData("price", "grid", "CX.PRICE")]
        [InlineData("risk", "grid", "CX.RISK")]
        [InlineData("risk", "krd", "CX.RISK")]
        [InlineData("spread", "grid", "CX.SPREAD")]
        [InlineData("yas", "grid", "CX.YAS")]
        public void Grids_are_value_errors_that_name_the_static_function(string verb, string field, string function)
        {
            var ex = Assert.Throws<ConvexException>(() => LiveFields.Validate(verb, field));
            Assert.Equal(ErrorCodes.InvalidInput, ex.Code);
            Assert.Contains(function, ex.Message);
        }

        [Theory]
        [InlineData("price", "z_spread")] // GX16: dropped from CX.PRICE
        [InlineData("risk", "nope")]
        [InlineData("yas", "nope")]
        public void Unknown_fields_are_name_errors(string verb, string field) =>
            Assert.Equal(ErrorCodes.UnknownToken,
                Assert.Throws<ConvexException>(() => LiveFields.Validate(verb, field)).Code);

        [Fact]
        public void Yas_fields_read_the_response()
        {
            var result = new JObject { ["ytm_pct"] = 4.25, ["dv01_per_100"] = 0.081 };
            Assert.Equal(4.25, LiveFields.Select("yas", "ytm", result));
            Assert.Equal(0.081, LiveFields.Select("yas", "dv01", result));
            Assert.Throws<ConvexException>(() => LiveFields.Select("yas", "clean", result));
        }
    }

    public class LiveHubTests
    {
        private sealed class Sink : ITopicSink
        {
            public readonly List<object> Values = new();
            public void Push(object value) => Values.Add(value);
        }

        private sealed class Fake
        {
            public ulong Generation = 10;
            public ChangeSet Next = new(10, true, Array.Empty<string>(), Array.Empty<ulong>());
            public readonly Dictionary<string, Func<object>> Values = new();
            public readonly List<string> Computed = new();
            public readonly List<Action> Scheduled = new();

            public LiveHub Hub() => new(
                spec =>
                {
                    Computed.Add(spec.RequestJson);
                    return Values[spec.RequestJson]();
                },
                () => Generation,
                cursor => Next,
                Scheduled.Add,
                parallelism: 1,
                startPoll: false);

            public void Changed(ulong cursor, bool complete = true, string[]? names = null, ulong[]? handles = null) =>
                Next = new ChangeSet(cursor, complete, names ?? Array.Empty<string>(), handles ?? Array.Empty<ulong>());
        }

        private static string[] Topic(JToken bond, JToken curve, string mark = "99.5C") =>
            LocalSpec.Create("price", "clean", new JObject
            {
                ["bond"] = bond, ["curve"] = curve, ["settlement"] = "2025-04-15", ["mark"] = mark,
            }).ToTopicStrings();

        private static string Json(string[] topic) => LocalSpec.FromTopicStrings(topic).RequestJson;

        [Fact]
        public void Connect_computes_once_on_the_calling_thread_and_returns_the_value()
        {
            var f = new Fake();
            var hub = f.Hub();
            var t = Topic("BOND", "USD.SOFR");
            f.Values[Json(t)] = () => 99.5;
            var sink = new Sink();
            Assert.Equal(99.5, hub.Connect(1, t, sink));
            Assert.Single(f.Computed);
            Assert.Empty(sink.Values);
            Assert.Equal(1, hub.TopicCount);
        }

        [Fact]
        public void A_change_recomputes_only_the_topics_that_read_it()
        {
            var f = new Fake();
            var hub = f.Hub();
            var onSofr = Topic("BOND", "USD.SOFR");
            var onTsy = Topic("BOND", "USD.TSY");
            var onHandle = Topic(101UL, 300UL);
            double sofr = 99.0;
            f.Values[Json(onSofr)] = () => sofr;
            f.Values[Json(onTsy)] = () => 98.0;
            f.Values[Json(onHandle)] = () => 97.0;
            var sofrSink = new Sink();
            var tsySink = new Sink();
            var handleSink = new Sink();
            hub.Connect(1, onSofr, sofrSink);
            hub.Connect(2, onTsy, tsySink);
            hub.Connect(3, onHandle, handleSink);
            f.Computed.Clear();

            sofr = 99.25;
            f.Changed(11, names: new[] { "USD.SOFR" });
            hub.Drain();
            Assert.Equal(new[] { Json(onSofr) }, f.Computed);
            Assert.Equal(new object[] { 99.25 }, sofrSink.Values);
            Assert.Empty(tsySink.Values);

            f.Computed.Clear();
            f.Changed(12, handles: new[] { 300UL });
            hub.Drain();
            Assert.Equal(new[] { Json(onHandle) }, f.Computed);
            Assert.Empty(handleSink.Values); // recomputed, same value: cut off
        }

        [Fact]
        public void An_incomplete_change_log_recomputes_every_topic()
        {
            var f = new Fake();
            var hub = f.Hub();
            var a = Topic("A", "C1");
            var b = Topic("B", "C2");
            f.Values[Json(a)] = () => 1.0;
            f.Values[Json(b)] = () => 2.0;
            hub.Connect(1, a, new Sink());
            hub.Connect(2, b, new Sink());
            f.Computed.Clear();
            f.Changed(50, complete: false);
            hub.Drain();
            Assert.Equal(2, f.Computed.Count);
        }

        [Fact]
        public void Value_cutoff_pushes_only_changes_and_treats_nan_as_equal()
        {
            var f = new Fake();
            var hub = f.Hub();
            var t = Topic("A", "C");
            object value = double.NaN;
            f.Values[Json(t)] = () => value;
            var sink = new Sink();
            hub.Connect(1, t, sink);
            f.Changed(11, names: new[] { "C" });
            hub.Drain();
            Assert.Empty(sink.Values);
            value = ExcelError.ExcelErrorRef;
            f.Changed(12, names: new[] { "C" });
            hub.Drain();
            Assert.Equal(new object[] { ExcelError.ExcelErrorRef }, sink.Values);
        }

        [Fact]
        public void Errors_become_excel_errors_with_detail_for_lasterror()
        {
            var f = new Fake();
            var hub = f.Hub();
            var t = Topic("MISSING", "C");
            f.Values[Json(t)] = () => throw new ConvexException("no bond 'MISSING'", ErrorCodes.InvalidHandle);
            Assert.Equal(ExcelError.ExcelErrorRef, hub.Connect(1, t, new Sink()));
            Assert.True(hub.TryGetError(LocalSpec.KeyOf(t), out var error));
            Assert.Contains("MISSING", error.Message);

            var sink = new Sink();
            hub.Disconnect(1);
            hub.Connect(1, t, sink);
            f.Values[Json(t)] = () => throw new ConvexException("no curve 'C'", ErrorCodes.InvalidHandle);
            f.Changed(11, names: new[] { "C" });
            hub.Drain();
            Assert.Equal(new object[] { ExcelError.ExcelErrorRef }, sink.Values); // same value, new cause
            Assert.True(hub.TryGetError(LocalSpec.KeyOf(t), out error));
            Assert.Contains("'C'", error.Message);

            f.Values[Json(t)] = () => 100.0;
            f.Changed(12, names: new[] { "C" });
            hub.Drain();
            Assert.False(hub.TryGetError(LocalSpec.KeyOf(t), out _));
        }

        [Fact]
        public void The_ffi_call_cap_keeps_the_last_value_and_retries_on_the_next_drain()
        {
            var f = new Fake();
            var hub = f.Hub();
            var t = Topic("A", "C");
            bool busy = false;
            double v = 1.0;
            f.Values[Json(t)] = () => busy ? throw new ConvexException("busy", ErrorCodes.ResourceLimit) : (object)v;
            var sink = new Sink();
            hub.Connect(1, t, sink);

            busy = true;
            v = 2.0;
            f.Changed(11, names: new[] { "C" });
            hub.Drain();
            Assert.Empty(sink.Values);

            busy = false;
            f.Changed(11); // nothing new in the log
            hub.Drain();
            Assert.Equal(new object[] { 2.0 }, sink.Values);
        }

        [Fact]
        public void Disconnected_topics_are_forgotten()
        {
            var f = new Fake();
            var hub = f.Hub();
            var t = Topic("A", "C");
            f.Values[Json(t)] = () => 1.0;
            hub.Connect(1, t, new Sink());
            hub.Disconnect(1);
            hub.Disconnect(1); // idempotent
            f.Computed.Clear();
            f.Changed(11, complete: false);
            hub.Drain();
            Assert.Empty(f.Computed);
            Assert.Equal(0, hub.TopicCount);
        }

        [Fact]
        public void Malformed_topics_are_refused_with_detail()
        {
            var f = new Fake();
            var hub = f.Hub();
            var bad = new[] { "L1", "price", "clean", "{not json" };
            Assert.IsType<ExcelError>(hub.Connect(1, bad, new Sink()));
            Assert.Equal(0, hub.TopicCount);
            Assert.True(hub.TryGetError(LocalSpec.KeyOf(bad), out _));
        }

        [Fact]
        public void The_topic_ceiling_is_enforced_with_a_typed_error()
        {
            var f = new Fake();
            var hub = f.Hub();
            for (int i = 0; i < LiveHub.MaxTopics; i++)
            {
                var t = Topic("B" + i, "C");
                f.Values[Json(t)] = () => 1.0;
                hub.Connect(i, t, new Sink());
            }
            var over = Topic("OVER", "C");
            // The message must not depend on the user's culture (de-DE writes 20.000).
            var culture = CultureInfo.CurrentCulture;
            CultureInfo.CurrentCulture = new CultureInfo("de-DE");
            try
            {
                Assert.Equal(ExcelError.ExcelErrorValue, hub.Connect(-1, over, new Sink()));
            }
            finally
            {
                CultureInfo.CurrentCulture = culture;
            }
            Assert.True(hub.TryGetError(LocalSpec.KeyOf(over), out var e));
            Assert.Contains("20,000", e.Message);
            Assert.Equal(LiveHub.MaxTopics, hub.TopicCount);
        }

        [Fact]
        public void Drain_requests_are_single_flight()
        {
            var f = new Fake();
            var hub = f.Hub();
            hub.RequestDrain();
            Assert.Empty(f.Scheduled); // no topics, nothing to do

            var t = Topic("A", "C");
            f.Values[Json(t)] = () => 1.0;
            hub.Connect(1, t, new Sink());
            hub.RequestDrain();
            hub.RequestDrain();
            hub.RequestDrain();
            Assert.Single(f.Scheduled);
            f.Computed.Clear();
            f.Changed(11, names: new[] { "C" });
            f.Scheduled[0](); // drains twice: once, then once more for the requests during it
            Assert.Equal(2, hub.Stats().Drains);
            hub.RequestDrain();
            Assert.Equal(2, f.Scheduled.Count); // idle again, so a new request schedules
        }
    }

    public class RtdValueTests
    {
        [Theory]
        [InlineData(ExcelError.ExcelErrorRef, unchecked((int)0x800A07E7))]   // CVErr(xlErrRef = 2023)
        [InlineData(ExcelError.ExcelErrorNA, unchecked((int)0x800A07FA))]    // CVErr(xlErrNA = 2042)
        [InlineData(ExcelError.ExcelErrorValue, unchecked((int)0x800A07DF))] // CVErr(xlErrValue = 2015)
        public void Connect_time_errors_reach_excel_as_com_errors(ExcelError error, int hresult) =>
            Assert.Equal(hresult,
                Assert.IsType<System.Runtime.InteropServices.ErrorWrapper>(ConvexRtdServer.ToComValue(error)).ErrorCode);

        [Fact]
        public void Numbers_pass_through() => Assert.Equal(99.5, ConvexRtdServer.ToComValue(99.5));
    }

    [Collection("native-registry")]
    public class NativeChangeLogTests
    {
        [Fact]
        public void Rebuilding_a_named_curve_reports_its_name_through_the_change_log()
        {
            var name = "X1.CHANGELOG." + Guid.NewGuid().ToString("N").Substring(0, 8);
            JObject Curve(double rate) => new()
            {
                ["type"] = "discrete", ["name"] = name, ["ref_date"] = "2025-01-15",
                ["tenors"] = new JArray(1.0, 5.0), ["values"] = new JArray(rate, rate),
                ["value_kind"] = "zero_rate", ["interpolation"] = "linear",
                ["day_count"] = "Act365Fixed", ["compounding"] = "Continuous",
                ["registry_key"] = "test!" + name,
            };
            ulong h1 = Cx.BuildCurve(Curve(0.04));
            ulong cursor = Cx.Generation();
            ulong h2 = Cx.BuildCurve(Curve(0.05));
            var changes = Cx.ChangesSince(cursor);
            Assert.True(changes.Complete);
            Assert.Contains(name, changes.Names);
            Assert.Contains(h1, changes.Handles);

            ulong idle = Cx.Generation();
            Assert.Equal(h2, Cx.BuildCurve(Curve(0.05))); // idempotent: nothing recorded
            Assert.DoesNotContain(name, Cx.ChangesSince(idle).Names);
            Cx.Release(h2);
        }
    }
}
