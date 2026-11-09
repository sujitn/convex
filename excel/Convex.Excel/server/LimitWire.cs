using System;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;
using System.Text.RegularExpressions;
using Newtonsoft.Json.Linq;

namespace Convex.Excel.Server
{
    /// One limit's evaluation, read from a status item (`GET .../limits/status`)
    /// or a `limit_status` message. Fields the server adds later (such as an
    /// acknowledgement, server plan P15c) are ignored.
    internal sealed class LimitItem
    {
        public string LimitId { get; set; } = "";
        /// within | warning | unknown | breach
        public string Status { get; set; } = "";
        /// The measure, with its member: `dv01`, `face BOND-1`, `key_rate_dv01 5Y`.
        public string Measure { get; set; } = "";
        public double? Value { get; set; }
        public double? Utilisation { get; set; }
        public double? Lower { get; set; }
        public double? Upper { get; set; }
        public double? WarningFraction { get; set; }
        /// For a limit on each member, the member whose value set the status.
        public string? Member { get; set; }
        /// Why the status is unknown, as text: its code and what it names.
        public string? Reason { get; set; }
        /// The last recorded status (`unevaluated` after a server start); only
        /// a recorded change carries it.
        public string? Previous { get; set; }
        /// The audit record of a recorded change; a status read has none.
        public ulong? RecordId { get; set; }
        public DateTime EvaluatedAtUtc { get; set; }
        public ulong BookVersion { get; set; }
        public ulong LimitSetVersion { get; set; }
        /// The server labelled it demonstration data.
        public bool Demo { get; set; }

        /// A limit_status message's change, or null when it is not one.
        public static (string BookId, LimitItem Item)? ParseChange(JObject message)
        {
            var book = ServerJson.Text(message["book_id"]);
            if (book == null || !TryVersions(message, out var bookVersion, out var setVersion)) return null;
            var item = Parse(message, ServerJson.Timestamp(message["evaluated_at"]), bookVersion, setVersion);
            if (item == null) return null;
            item.Previous = ServerJson.Text(message["previous"]);
            item.RecordId = message["record_id"] is JValue { Type: JTokenType.Integer } id
                ? Convert.ToUInt64(id.Value, CultureInfo.InvariantCulture)
                : null;
            item.Demo = ServerJson.Text(message["data_quality"]) == "demo";
            return (book, item);
        }

        /// One limit of a status read or of a change; null when malformed.
        public static LimitItem? Parse(JObject o, DateTime? evaluatedAt, ulong bookVersion, ulong setVersion)
        {
            var id = ServerJson.Text(o["limit_id"]);
            var status = ServerJson.Text(o["status"]);
            if (id == null || status == null || evaluatedAt == null || o["measure"] is not JObject measure) return null;
            return new LimitItem
            {
                LimitId = id,
                Status = status,
                Measure = MeasureText(measure),
                Value = ServerJson.Number(o["value"]),
                Utilisation = ServerJson.Number(o["utilisation"]),
                Lower = ServerJson.Number(o["lower"]),
                Upper = ServerJson.Number(o["upper"]),
                WarningFraction = ServerJson.Number(o["warning_fraction"]),
                Member = ServerJson.Text(o["member"]),
                Reason = o["reason"] is JObject reason ? ReasonText(reason) : null,
                EvaluatedAtUtc = evaluatedAt.Value,
                BookVersion = bookVersion,
                LimitSetVersion = setVersion,
            };
        }

        internal static bool TryVersions(JObject o, out ulong bookVersion, out ulong setVersion)
        {
            bookVersion = setVersion = 0;
            if (o["book_version"] is not JValue { Type: JTokenType.Integer } b ||
                o["limit_set_version"] is not JValue { Type: JTokenType.Integer } s)
                return false;
            bookVersion = Convert.ToUInt64(b.Value, CultureInfo.InvariantCulture);
            setVersion = Convert.ToUInt64(s.Value, CultureInfo.InvariantCulture);
            return true;
        }

        /// `kind`, then what it bounds: `face BOND-1`, `face each`,
        /// `key_rate_dv01 5Y`, `group_share issuer ISSUER-1`.
        internal static string MeasureText(JObject measure)
        {
            var parts = new List<string> { ServerJson.Text(measure["kind"]) ?? "?" };
            foreach (var p in measure.Properties())
            {
                if (p.Name == "kind") continue;
                var text = MemberText(p.Value);
                if (text != null) parts.Add(text);
            }
            return string.Join(" ", parts);
        }

        /// A member is `"each"` or `{"one": id}`; a tenor or grouping is text.
        private static string? MemberText(JToken token) => token switch
        {
            JValue { Type: JTokenType.String } v => (string)v!,
            JObject o when ServerJson.Text(o["one"]) is { } one => one,
            _ => null,
        };

        /// The reason's code, then what it names: `missing_result: BOND-1,
        /// BOND-2 (3 in all)`, `quote_refused: BOND-1: stale quote`.
        internal static string ReasonText(JObject reason)
        {
            var text = ServerJson.Text(reason["code"]) ?? "unknown";
            var details = new List<string>();
            foreach (var p in reason.Properties())
            {
                switch (p.Name)
                {
                    case "code":
                        continue;
                    case "instruments" when p.Value is JObject list:
                        var first = (list["first"] as JArray ?? new JArray()).Select(t => ServerJson.Text(t)).Where(t => t != null).ToList();
                        var count = ServerJson.Number(list["count"]) ?? first.Count;
                        details.Add(string.Join(", ", first) +
                                    (count > first.Count ? $" ({count.ToString(CultureInfo.InvariantCulture)} in all)" : ""));
                        break;
                    default:
                        if (ServerJson.Text(p.Value) is { } s) details.Add(s);
                        break;
                }
            }
            return details.Count == 0 ? text : text + ": " + string.Join(": ", details);
        }
    }

    /// A status read: each limit of the book's current set, evaluated now.
    internal sealed class LimitBookStatus
    {
        public string BookId { get; set; } = "";
        public ulong BookVersion { get; set; }
        public ulong LimitSetVersion { get; set; }
        public DateTime EvaluatedAtUtc { get; set; }
        public List<LimitItem> Limits { get; } = new();

        /// Null when the body is not a status read.
        public static LimitBookStatus? Parse(JObject o, bool demo)
        {
            var book = ServerJson.Text(o["book_id"]);
            var at = ServerJson.Timestamp(o["evaluated_at"]);
            if (book == null || at == null || o["limits"] is not JArray limits ||
                !LimitItem.TryVersions(o, out var bookVersion, out var setVersion))
                return null;
            var status = new LimitBookStatus
            {
                BookId = book,
                BookVersion = bookVersion,
                LimitSetVersion = setVersion,
                EvaluatedAtUtc = at.Value,
            };
            foreach (var item in limits.OfType<JObject>())
            {
                var limit = LimitItem.Parse(item, at, bookVersion, setVersion);
                if (limit == null) return null;
                limit.Demo = demo;
                status.Limits.Add(limit);
            }
            return status;
        }
    }

    /// The limits errors the server tells apart from the valuations ones only
    /// by their text (Excel plan DX19). A server test pins each opening
    /// (`crates/convex-server/src/websocket.rs`,
    /// `the_limit_stream_errors_keep_the_text_the_excel_add_in_reads`).
    internal static class LimitErrors
    {
        /// What `subscribe_limits` gets from a server with no limit monitor.
        public const string NotMonitored = "this server does not monitor limits";

        private static readonly Regex Forbidden = new(
            @"^limit status of book (?<book>\S+) needs read access",
            RegexOptions.CultureInvariant);

        /// A FORBIDDEN that answers no request: a subscribed book changed and
        /// the session may not read it. A book id has no spaces, so the parse
        /// is exact.
        public static bool TryParseForbiddenBook(string message, out string bookId)
        {
            var m = Forbidden.Match(message);
            bookId = m.Groups["book"].Value;
            return m.Success;
        }

        /// RESYNC_REQUIRED for the limit stream: read each book's status again.
        public static bool IsLimitsResync(string message) =>
            message.StartsWith("Read /api/v1/books/", StringComparison.Ordinal);

        /// RESYNC_REQUIRED for the valuation stream: subscribe again.
        public static bool IsValuationsResync(string message) =>
            message.StartsWith("Subscribe again", StringComparison.Ordinal);
    }
}
