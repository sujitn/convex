using System;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;

namespace Convex.Excel.Server
{
    /// One limit before and after a proposed trade.
    internal sealed class LimitTicketRow
    {
        public string LimitId { get; set; } = "";
        public string Measure { get; set; } = "";
        public string Bounds { get; set; } = "";
        public LimitItem? Now { get; set; }
        public LimitItem? After { get; set; }

        /// The trade makes the limit's status more severe.
        public bool Worse => LimitTicket.Severity(After?.Status) > LimitTicket.Severity(Now?.Status);
    }

    /// The pre-trade ticket's comparison: the book's status now beside what
    /// the check says it would be with the proposed changes (server D88, D90).
    internal static class LimitTicket
    {
        /// within < warning < unknown < breach, as the server orders them.
        public static int Severity(string? status) => status switch
        {
            "within" => 0,
            "warning" => 1,
            "unknown" => 2,
            "breach" => 3,
            _ => -1,
        };

        /// One row per limit, in the check's order (then any limit only the
        /// earlier read had, if the set changed between the two).
        public static List<LimitTicketRow> Compare(LimitBookStatus? now, LimitBookStatus after)
        {
            var before = now?.Limits.ToDictionary(l => l.LimitId, StringComparer.Ordinal) ??
                         new Dictionary<string, LimitItem>(StringComparer.Ordinal);
            var rows = after.Limits.Select(a => Row(before.TryGetValue(a.LimitId, out var b) ? b : null, a)).ToList();
            var seen = new HashSet<string>(after.Limits.Select(l => l.LimitId), StringComparer.Ordinal);
            rows.AddRange(before.Values.Where(b => !seen.Contains(b.LimitId)).Select(b => Row(b, null)));
            return rows;
        }

        private static LimitTicketRow Row(LimitItem? now, LimitItem? after)
        {
            var any = after ?? now!;
            return new LimitTicketRow
            {
                LimitId = any.LimitId,
                Measure = any.Measure,
                Bounds = Bounds(any),
                Now = now,
                After = after,
            };
        }

        /// `[-50,000, 50,000]`, `≤ 2,000,000`, `≥ -1,000`, with the warning
        /// fraction when there is one.
        public static string Bounds(LimitItem item)
        {
            string text = (item.Lower, item.Upper) switch
            {
                ({ } lo, { } hi) => $"[{Number(lo)}, {Number(hi)}]",
                (null, { } hi) => "≤ " + Number(hi),
                ({ } lo, null) => "≥ " + Number(lo),
                _ => "",
            };
            return item.WarningFraction is { } w
                ? text + " (warn " + w.ToString("0.##%", CultureInfo.InvariantCulture) + ")"
                : text;
        }

        public static string Number(double? value) => value switch
        {
            null => "",
            { } v when Math.Abs(v) >= 1000 => v.ToString("#,##0", CultureInfo.InvariantCulture),
            { } v => v.ToString("0.######", CultureInfo.InvariantCulture),
        };

        public static string Percent(double? value) =>
            value is { } v ? v.ToString("0.0%", CultureInfo.InvariantCulture) : "";
    }
}
