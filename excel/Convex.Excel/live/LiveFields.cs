using System.Collections.Generic;
using Newtonsoft.Json.Linq;

namespace Convex.Excel.Live
{
    // Scalar fields a local live cell can show (DX3: RTD carries scalars only).
    // Aliases canonicalise first, so cells that ask for the same number in two
    // spellings share one topic. Grids stay on the static functions.
    internal static class LiveFields
    {
        private static readonly Dictionary<string, string> Price = new()
        {
            ["clean"] = "clean", ["dirty"] = "dirty", ["accrued"] = "accrued", ["ytm"] = "ytm",
        };

        private static readonly Dictionary<string, string> Risk = new()
        {
            ["mod_dur"] = "mod_dur", ["modified_duration"] = "mod_dur",
            ["mac_dur"] = "mac_dur", ["macaulay_duration"] = "mac_dur",
            ["convexity"] = "convexity", ["dv01"] = "dv01",
            ["spread_dur"] = "spread_dur", ["spread_duration"] = "spread_dur",
        };

        private static readonly Dictionary<string, string> Spread = new()
        {
            ["bps"] = "bps",
        };

        // CX.YAS grid row → response field.
        private static readonly Dictionary<string, string> Yas = new()
        {
            ["clean"] = "clean_price", ["dirty"] = "dirty_price", ["accrued"] = "accrued",
            ["accrued_days"] = "accrued_days", ["ytm"] = "ytm_pct",
            ["current_yield"] = "current_yield_pct", ["simple_yield"] = "simple_yield_pct",
            ["z_spread"] = "z_spread_bps", ["mod_dur"] = "modified_duration",
            ["mac_dur"] = "macaulay_duration", ["convexity"] = "convexity",
            ["dv01"] = "dv01_per_100", ["principal"] = "principal_amount",
            ["accrued_amount"] = "accrued_amount", ["settlement_amount"] = "settlement_amount",
        };

        /// Canonical live field for `verb`, or a ConvexException: #VALUE! naming
        /// the static function for a grid, #NAME? for an unknown field.
        public static string Validate(string verb, string field)
        {
            var f = field.Trim().ToLowerInvariant();
            switch (verb)
            {
                case "price":
                    if (f == "grid") throw NotLive(f, "CX.PRICE");
                    return Lookup(Price, f, "CX.PRICE.LIVE", "clean, dirty, accrued or ytm");
                case "risk":
                    if (f is "grid" or "krd" or "key_rates") throw NotLive(f, "CX.RISK");
                    return Lookup(Risk, f, "CX.RISK.LIVE",
                        "mod_dur, mac_dur, convexity, dv01 or spread_dur");
                case "spread":
                    if (f == "grid") throw NotLive(f, "CX.SPREAD");
                    return Lookup(Spread, f, "CX.SPREAD.LIVE", "bps");
                case "yas":
                    if (f == "grid") throw NotLive(f, "CX.YAS");
                    if (Yas.ContainsKey(f)) return f;
                    throw new ConvexException(
                        $"unknown CX.YAS.LIVE field {f}: use one of {string.Join(", ", Yas.Keys)}",
                        ErrorCodes.UnknownToken);
                default:
                    throw new ConvexException($"'{verb}' has no live form");
            }
        }

        /// The scalar for a canonical field from an RPC result.
        public static object Select(string verb, string field, JToken result) => verb switch
        {
            "price" => Functions.SelectPriceField(result, field),
            "risk" => Functions.SelectRiskField(result, field),
            "spread" => Functions.SelectSpreadField(result, field),
            "yas" => (double?)result[Yas[field]] ?? throw new ConvexException($"missing {Yas[field]}"),
            _ => throw new ConvexException($"'{verb}' has no live form"),
        };

        private static string Lookup(Dictionary<string, string> fields, string f, string function, string allowed) =>
            fields.TryGetValue(f, out var canonical)
                ? canonical
                : throw new ConvexException($"unknown {function} field {f}: use {allowed}",
                    ErrorCodes.UnknownToken);

        private static ConvexException NotLive(string field, string staticFunction) =>
            new($"'{field}' is not a live field (live cells return one number): use {staticFunction}");
    }
}
