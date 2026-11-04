using System;
using ExcelDna.Integration;

namespace Convex.Excel
{
    // Caller-cell keys. Pretty key "Sheet1!R5C2" needs the xlSheetNm callback
    // (main calc thread only — fine for builders); stable key "1A2B3C!R5C2"
    // is derived from the ExcelReference alone, safe from IsThreadSafe UDFs
    // on worker threads, and is what the error store and CX.LASTERROR share.
    internal static class CxCaller
    {
        public static ExcelReference? TryGetCaller()
        {
            try { return XlCall.Excel(XlCall.xlfCaller) as ExcelReference; }
            catch { return null; } // only valid during cell calculation
        }

        public static string? TryGetPrettyKey()
        {
            try
            {
                if (XlCall.Excel(XlCall.xlfCaller) is not ExcelReference r) return null;
                var sheet = XlCall.Excel(XlCall.xlSheetNm, r) as string ?? "";
                return $"{sheet}!R{r.RowFirst}C{r.ColumnFirst}";
            }
            catch
            {
                return null;
            }
        }

        public static string StableKey(ExcelReference r) =>
            $"{r.SheetId.ToInt64():X}!R{r.RowFirst}C{r.ColumnFirst}";

        // "Sheet1!B5" when xlSheetNm is callable; stable key otherwise.
        public static string Describe(ExcelReference r)
        {
            var cell = $"{ColumnName(r.ColumnFirst)}{r.RowFirst + 1}";
            try
            {
                if (XlCall.Excel(XlCall.xlSheetNm, r) is string sheet && sheet.Length > 0)
                    return $"{sheet}!{cell}";
            }
            catch
            {
                // xlSheetNm not allowed from this thread/context.
            }
            return StableKey(r);
        }

        private static string ColumnName(int zeroBased)
        {
            var name = "";
            int n = zeroBased;
            while (n >= 0)
            {
                name = (char)('A' + n % 26) + name;
                n = n / 26 - 1;
            }
            return name;
        }
    }
}
