using System;
using System.Collections.Generic;
using System.Globalization;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;
using ExcelDna.Integration;

namespace Convex.Excel
{
    // Coercion helpers that turn untyped Excel cell values into the
    // JSON shape the FFI expects. Everything here is single-purpose so
    // the UDF surface stays declarative.
    internal static class CxParse
    {
        public const string HandlePrefix = "#CX#";

        public static string FormatHandle(ulong h) => HandlePrefix + h.ToString(CultureInfo.InvariantCulture);

        // Accepts: "#CX#101", numeric handle, or named lookup by registry name.
        public static ulong AsHandle(object value, string fieldName = "handle")
        {
            switch (value)
            {
                case null:
                case ExcelMissing:
                case ExcelEmpty:
                    throw new ConvexException($"{fieldName} is missing");
                case double d:
                    return (ulong)d;
                case string s:
                    var trimmed = s.Trim();
                    if (trimmed.StartsWith(HandlePrefix, StringComparison.OrdinalIgnoreCase))
                        trimmed = trimmed.Substring(HandlePrefix.Length);
                    if (ulong.TryParse(trimmed, NumberStyles.Integer, CultureInfo.InvariantCulture, out var h))
                        return h;
                    throw new ConvexException($"{fieldName} {s}: not a recognized handle");
                default:
                    return Convert.ToUInt64(value, CultureInfo.InvariantCulture);
            }
        }

        public static ulong? AsHandleOrNull(object value)
        {
            if (value is null or ExcelMissing or ExcelEmpty) return null;
            return AsHandle(value);
        }

        // Mark cell semantics — accept either a textual mark (forwarded to
        // the Rust parser) or a parsed JSON object (rare; user pasted JSON).
        public static JToken AsMark(object value)
        {
            if (value is string s && !string.IsNullOrWhiteSpace(s))
            {
                var trimmed = s.Trim();
                if (trimmed.StartsWith("{"))
                    return JToken.Parse(trimmed) ?? throw new ConvexException("mark JSON parse failed");
                return new JValue(trimmed); // text — Rust side parses
            }
            if (value is double d)
                return new JValue(d.ToString(CultureInfo.InvariantCulture))!;
            throw new ConvexException("mark must be a textual mark or JSON object");
        }

        // ISO-8601 date suitable for serde::Deserialize for `convex_core::Date`.
        public static string AsIsoDate(DateTime dt) => dt.ToString("yyyy-MM-dd", CultureInfo.InvariantCulture);

        // Frequency: accepts "A", "SA", "SemiAnnual", "Q", "M", or numeric 1/2/4/12.
        public static string AsFrequency(object value, string defaultFreq = "SemiAnnual")
        {
            if (value is null or ExcelMissing or ExcelEmpty) return defaultFreq;
            switch (value)
            {
                case string s:
                    return s.Trim().ToUpperInvariant() switch
                    {
                        "A" or "ANN" or "ANNUAL" => "Annual",
                        "SA" or "SEMI" or "SEMIANNUAL" or "SEMI-ANNUAL" => "SemiAnnual",
                        "Q" or "QUARTERLY" => "Quarterly",
                        "M" or "MONTHLY" => "Monthly",
                        "Z" or "ZERO" => "Zero",
                        _ => throw new ConvexException($"unknown frequency {s}"),
                    };
                case double d when (int)d == 1: return "Annual";
                case double d when (int)d == 2: return "SemiAnnual";
                case double d when (int)d == 4: return "Quarterly";
                case double d when (int)d == 12: return "Monthly";
            }
            return defaultFreq;
        }

        // Day-count: pass-through for explicit string codes; numeric falls back to a small enum.
        public static string AsDayCount(object value, string defaultDc = "Thirty360US")
        {
            if (value is null or ExcelMissing or ExcelEmpty) return defaultDc;
            return value switch
            {
                string s => s.Trim() switch
                {
                    "0" => "Act360",
                    "1" => "Act365Fixed",
                    "2" => "ActActIsda",
                    "3" => "ActActIcma",
                    "4" => "Thirty360US",
                    "5" => "Thirty360E",
                    var t => t,
                },
                double d => ((int)d) switch
                {
                    0 => "Act360",
                    1 => "Act365Fixed",
                    2 => "ActActIsda",
                    3 => "ActActIcma",
                    4 => "Thirty360US",
                    5 => "Thirty360E",
                    _ => defaultDc,
                },
                _ => defaultDc,
            };
        }

        // Spread type (canonical names match `SpreadType` Rust enum).
        public static string AsSpreadType(string s)
        {
            return s.Trim().ToUpperInvariant() switch
            {
                "Z" or "ZSPREAD" or "Z-SPREAD" => "ZSpread",
                "G" or "GSPREAD" or "G-SPREAD" => "GSpread",
                "I" or "ISPREAD" or "I-SPREAD" => "ISpread",
                "OAS" => "OAS",
                "DM" or "DISCOUNT_MARGIN" or "DISCOUNTMARGIN" => "DiscountMargin",
                "ASW" or "ASW_PAR" or "ASW-PAR" => "AssetSwapPar",
                "ASW_PROC" or "ASW_PROCEEDS" => "AssetSwapProceeds",
                "CREDIT" => "Credit",
                _ => throw new ConvexException($"unknown spread type {s}"),
            };
        }

        // 1D or 2D ranges of cells -> double[]
        public static double[] AsDoubles(object range)
        {
            switch (range)
            {
                case double d: return new[] { d };
                case object[,] g:
                    var list = new List<double>(g.Length);
                    int rows = g.GetLength(0), cols = g.GetLength(1);
                    for (int r = 0; r < rows; r++)
                        for (int c = 0; c < cols; c++)
                            if (g[r, c] is double v) list.Add(v);
                            else if (g[r, c] is string s2 && double.TryParse(s2, NumberStyles.Any, CultureInfo.InvariantCulture, out var p))
                                list.Add(p);
                    return list.ToArray();
                case object[] arr:
                    var ll = new List<double>(arr.Length);
                    foreach (var o in arr)
                        if (o is double v) ll.Add(v);
                        else if (o is string s2 && double.TryParse(s2, NumberStyles.Any, CultureInfo.InvariantCulture, out var p))
                            ll.Add(p);
                    return ll.ToArray();
                default: return Array.Empty<double>();
            }
        }
    }
}
