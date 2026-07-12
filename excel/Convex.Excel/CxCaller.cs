using System;
using ExcelDna.Integration;

namespace Convex.Excel
{
    // Caller-cell identification, shared by the registry eviction key and the
    // error-detail store. Two key shapes exist because of Excel's threading
    // rules:
    //
    //  • Pretty key  "Sheet1!R5C2"    — needs the xlSheetNm callback, which is
    //    only allowed on the main calc thread. Used as the Rust registry
    //    eviction slot (builders are not thread-safe, so always available).
    //  • Stable key  "1A2B3C!R5C2"    — derived from the ExcelReference alone
    //    (sheet id in hex), safe from IsThreadSafe UDFs on worker threads.
    //    Used to key the error store; CX.LASTERROR rebuilds the same key from
    //    its reference argument, so lookups match without any callback.
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

        // Best-effort human-readable address ("Sheet1!B5"). Falls back to the
        // stable key when xlSheetNm isn't callable (worker threads).
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
