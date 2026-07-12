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

        // Request reference: numeric handle, "#CX#101" string, or a
        // ticker/name the engine resolves against its alias table.
        public static JToken AsHandleRef(object value, string fieldName = "handle")
        {
            switch (value)
            {
                case null:
                case ExcelMissing:
                case ExcelEmpty:
                    throw new ConvexException($"{fieldName} is missing");
                case double d:
                    if (d < 0 || d > ulong.MaxValue || Math.Floor(d) != d)
                        throw new ConvexException($"{fieldName} {d}: not a non-negative integer handle");
                    return new JValue((ulong)d);
                case string s:
                    var t = s.Trim();
                    if (t.Length == 0)
                        throw new ConvexException($"{fieldName} is empty");
                    if (t.StartsWith(HandlePrefix, StringComparison.OrdinalIgnoreCase) &&
                        ulong.TryParse(t.Substring(HandlePrefix.Length), NumberStyles.Integer,
                            CultureInfo.InvariantCulture, out var h))
                        return new JValue(h);
                    if (ulong.TryParse(t, NumberStyles.Integer, CultureInfo.InvariantCulture, out var n))
                        return new JValue(n);
                    return new JValue(t); // ticker/name — resolved engine-side
                default:
                    return new JValue(Convert.ToUInt64(value, CultureInfo.InvariantCulture));
            }
        }

        public static JToken? AsHandleRefOrNull(object value)
        {
            if (value is null or ExcelMissing or ExcelEmpty) return null;
            if (value is string s && string.IsNullOrWhiteSpace(s)) return null;
            return AsHandleRef(value);
        }

        // Strictly numeric handle — registry ops (release/describe) address a
        // specific instance, never a name.
        public static ulong AsHandle(object value, string fieldName = "handle")
        {
            switch (value)
            {
                case null:
                case ExcelMissing:
                case ExcelEmpty:
                    throw new ConvexException($"{fieldName} is missing");
                case double d:
                    // Math.Floor(d) != d also rules out NaN/Inf (NaN != NaN).
                    if (d < 0 || d > ulong.MaxValue || Math.Floor(d) != d)
                        throw new ConvexException($"{fieldName} {d}: not a non-negative integer handle");
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
                // Fixed notation: default ToString can emit "1E-06", which
                // the mark grammar rejects.
                return new JValue(d.ToString("0.################", CultureInfo.InvariantCulture))!;
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
                        _ => throw new ConvexException($"unknown frequency {s}", ErrorCodes.UnknownToken),
                    };
                case double d when (int)d == 1: return "Annual";
                case double d when (int)d == 2: return "SemiAnnual";
                case double d when (int)d == 4: return "Quarterly";
                case double d when (int)d == 12: return "Monthly";
                case double d:
                    throw new ConvexException(
                        $"unknown frequency {d} (use 1, 2, 4 or 12)", ErrorCodes.UnknownToken);
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
                _ => throw new ConvexException($"unknown spread type {s}", ErrorCodes.UnknownToken),
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

        // ===================================================================
        // Strict range extraction — required wherever two ranges must stay
        // parallel: trailing blanks trim, but an embedded blank or
        // unparseable cell throws with its position (the lenient AsDoubles
        // above silently drops cells, which misaligns parallel ranges).
        // ===================================================================

        public static double[] AsDoublesStrict(object range, string fieldName) =>
            ExtractStrict(range, fieldName, "a number", cell =>
            {
                if (cell is double v) return (true, v);
                if (cell is string s &&
                    double.TryParse(s.Trim(), NumberStyles.Any, CultureInfo.InvariantCulture, out var p))
                    return (true, p);
                return (false, 0.0);
            });

        public static DateTime[] AsDatesStrict(object range, string fieldName) =>
            ExtractStrict(range, fieldName, "a date", cell =>
            {
                switch (cell)
                {
                    case double oa: return (true, DateTime.FromOADate(oa));
                    case DateTime dt: return (true, dt);
                    case string s:
                        var t = s.Trim();
                        // Never the machine locale: dd/MM vs MM/dd swaps dates.
                        if (DateTime.TryParseExact(t, "yyyy-MM-dd", CultureInfo.InvariantCulture,
                                DateTimeStyles.None, out var iso))
                            return (true, iso);
                        if (DateTime.TryParse(t, CultureInfo.InvariantCulture,
                                DateTimeStyles.None, out var inv))
                            return (true, inv);
                        return (false, default);
                    default: return (false, default);
                }
            });

        public static string[] AsStringsStrict(object range, string fieldName) =>
            ExtractStrict(range, fieldName, "text", cell =>
            {
                var s = cell as string ?? cell.ToString();
                return string.IsNullOrWhiteSpace(s) ? (false, "") : (true, s!.Trim());
            });

        private static T[] ExtractStrict<T>(
            object range, string fieldName, string expected,
            Func<object, (bool ok, T value)> convert)
        {
            if (IsBlankCell(range)) return Array.Empty<T>();

            if (range is object[,] grid)
            {
                int rows = grid.GetLength(0), cols = grid.GetLength(1);
                var list = new List<T>(rows * cols);
                bool seenBlank = false;
                int blankRow = 0, blankCol = 0;
                for (int r = 0; r < rows; r++)
                    for (int c = 0; c < cols; c++)
                    {
                        var cell = grid[r, c];
                        if (IsBlankCell(cell))
                        {
                            if (!seenBlank) { seenBlank = true; blankRow = r; blankCol = c; }
                            continue;
                        }
                        if (seenBlank)
                            throw new ConvexException(
                                $"{fieldName}: empty cell at row {blankRow + 1}, column {blankCol + 1} " +
                                "inside the range — parallel ranges must be contiguous");
                        list.Add(ConvertStrict(cell, fieldName, expected, convert,
                            $" at row {r + 1}, column {c + 1}"));
                    }
                return list.ToArray();
            }

            if (range is object[] arr)
            {
                var list = new List<T>(arr.Length);
                bool seenBlank = false;
                int blankAt = 0;
                for (int i = 0; i < arr.Length; i++)
                {
                    if (IsBlankCell(arr[i]))
                    {
                        if (!seenBlank) { seenBlank = true; blankAt = i; }
                        continue;
                    }
                    if (seenBlank)
                        throw new ConvexException(
                            $"{fieldName}: empty cell at position {blankAt + 1} inside the range " +
                            "— parallel ranges must be contiguous");
                    list.Add(ConvertStrict(arr[i], fieldName, expected, convert, $" at position {i + 1}"));
                }
                return list.ToArray();
            }

            return new[] { ConvertStrict(range, fieldName, expected, convert, "") };
        }

        private static T ConvertStrict<T>(
            object cell, string fieldName, string expected,
            Func<object, (bool ok, T value)> convert, string where)
        {
            var (ok, value) = convert(cell);
            if (!ok)
                throw new ConvexException($"{fieldName}: '{cell}'{where} is not {expected}");
            return value;
        }

        private static bool IsBlankCell(object? cell) =>
            cell is null or ExcelMissing or ExcelEmpty ||
            (cell is string s && s.Trim().Length == 0);
    }
}
