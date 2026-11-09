using System.Linq;

namespace Convex.Excel.Server
{
    /// The scalars a CX.LIMIT cell can show (DX13, DX21), and why it shows #N/A.
    internal static class LimitFields
    {
        public static readonly string[] All =
        {
            "status", "value", "utilisation", "lower", "upper", "warning_fraction", "member", "measure", "reason",
            "previous", "evaluated_at", "record_id", "quality", "book_version", "limit_set_version",
        };

        /// Canonical CX.LIMIT field, or #NAME? listing the fields.
        public static string Validate(string field)
        {
            var f = field.Trim().ToLowerInvariant();
            if (All.Contains(f)) return f;
            throw new ConvexException($"unknown CX.LIMIT field {f}: use {string.Join(", ", All)}", ErrorCodes.UnknownToken);
        }

        /// A book or limit id: the server's identifier grammar.
        public static string ValidateId(string id, string what)
        {
            var s = id.Trim();
            if (Identifiers.IsInstrumentId(s)) return s;
            throw new ConvexException(
                $"'{id}' is not a {what} id (1 to 128 letters, digits or _ . : / -, starting with a letter or digit)");
        }

        /// The value of a CX.LIMIT cell for `view`. Throws a ConvexException
        /// for #N/A (with the reason) when there is nothing to show.
        public static object Select(LimitView view, string field)
        {
            var item = Require(view);
            var name = $"limit {view.LimitId} of book {view.BookId}";
            switch (field)
            {
                case "status":
                    return item.Status;
                case "quality":
                    // Values stay through a disconnect, marked Stale (DX6).
                    return !view.Fresh ? "Stale" : item.Demo ? "Demo" : "Live";
                case "value":
                    return item.Value ?? throw Missing("NO_VALUE",
                        item.Status == "unknown" ? $"{name} is unknown: {item.Reason ?? "no reason given"}"
                        : $"{name} has no value (a limit on each member of a book that holds none)");
                case "utilisation":
                    return item.Utilisation ?? throw Missing("NO_UTILISATION",
                        item.Status == "unknown" ? $"{name} is unknown: {item.Reason ?? "no reason given"}"
                        : $"{name} has no utilisation: no bound faces away from zero, or there is no value");
                case "lower":
                    return item.Lower ?? throw Missing("NO_BOUND", $"{name} has no lower bound");
                case "upper":
                    return item.Upper ?? throw Missing("NO_BOUND", $"{name} has no upper bound");
                case "warning_fraction":
                    return item.WarningFraction ?? throw Missing("NO_BOUND", $"{name} has no warning fraction");
                case "member":
                    return item.Member ?? "";
                case "measure":
                    return item.Measure;
                case "reason":
                    return item.Reason ?? "";
                case "previous":
                    return item.Previous ?? "";
                case "evaluated_at":
                    return item.EvaluatedAtUtc.ToOADate();
                case "record_id":
                    return item.RecordId is { } id ? (double)id : throw Missing("NO_RECORD",
                        $"no recorded change of {name} has arrived since it was read; a status read carries no record id");
                case "book_version":
                    return (double)item.BookVersion;
                case "limit_set_version":
                    return (double)item.LimitSetVersion;
                default:
                    throw new ConvexException($"unknown CX.LIMIT field {field}", ErrorCodes.UnknownToken);
            }
        }

        /// The limit's newest evaluation, or #N/A saying why there is none.
        private static LimitItem Require(LimitView view)
        {
            if (view.Sub is Subscription.Refused or Subscription.OverCap)
                throw Missing(view.ProblemCode ?? "REFUSED", view.ProblemMessage ?? "the server refused book " + view.BookId);
            if (view.Item != null) return view.Item;
            if (view.ProblemCode == "NO_LIMIT_SET")
                throw Missing("NO_LIMIT_SET", view.ProblemMessage ?? $"the server has no limit set for book {view.BookId}");
            if (view.SetKnown)
                throw Missing("NO_LIMIT", $"version {view.SetVersion} of book {view.BookId}'s limit set has no limit {view.LimitId}");
            var server = view.Server.Length > 0 ? view.Server : "the server";
            throw view.Link switch
            {
                LinkState.NotConfigured => Missing("NOT_CONFIGURED",
                    "no Convex server is set: use Server on the Convex ribbon" +
                    (view.LinkProblem != null ? " (" + view.LinkProblem + ")" : "")),
                LinkState.SignedOut => Missing("SIGNED_OUT",
                    $"not signed in to {server}" + (view.LinkProblem != null ? ": " + view.LinkProblem : "")),
                LinkState.Connecting => Missing("CONNECTING", $"connecting to {server}"),
                LinkState.Reconnecting => Missing("DISCONNECTED",
                    $"not connected to {server}" + (view.LinkProblem != null ? ": " + view.LinkProblem : "")),
                _ => Missing("WAITING", $"reading the limit status of book {view.BookId}" +
                                        (view.ReadProblem != null ? " (last attempt: " + view.ReadProblem + ")" : "")),
            };
        }

        private static ConvexException Missing(string code, string message) => ConvexException.Unavailable(code, message);
    }
}
