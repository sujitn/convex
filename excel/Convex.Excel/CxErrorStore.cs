using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Linq;
using ExcelDna.Integration;

namespace Convex.Excel
{
    internal sealed class CxErrorDetail
    {
        public string Code { get; }
        public string Message { get; }
        public string Address { get; }
        public DateTime Utc { get; }

        public CxErrorDetail(string code, string message, string address)
        {
            Code = code;
            Message = message;
            Address = address;
            Utc = DateTime.UtcNow;
        }
    }

    // Cell-keyed detail behind the Excel errors UDFs return. Written only on
    // the error path (success recalcs must not pay the caller lookup), so a
    // fixed cell keeps its stale entry — the viewer shows timestamps.
    internal static class CxErrorStore
    {
        private const int Cap = 20_000; // wholesale clear at the cap

        private static readonly ConcurrentDictionary<string, CxErrorDetail> _byCell = new();

        public static void Record(Exception ex)
        {
            var code = ex is ConvexException cx ? cx.Code : CodeFor(ex);
            var caller = CxCaller.TryGetCaller();
            var key = caller == null ? "(non-cell)" : CxCaller.StableKey(caller);
            var address = caller == null ? "(non-cell)" : CxCaller.Describe(caller);
            if (_byCell.Count >= Cap) _byCell.Clear();
            _byCell[key] = new CxErrorDetail(code, ex.Message, address);
        }

        public static CxErrorDetail? Lookup(string stableKey) =>
            _byCell.TryGetValue(stableKey, out var d) ? d : null;

        public static List<CxErrorDetail> Snapshot() =>
            _byCell.Values.OrderByDescending(d => d.Utc).ToList();

        public static void Clear() => _byCell.Clear();

        private static string CodeFor(Exception ex) => ex switch
        {
            DllNotFoundException => "native_load",
            EntryPointNotFoundException => "native_load",
            BadImageFormatException => "native_load",
            _ => ErrorCodes.InvalidInput,
        };
    }

    // Exception → native Excel error. Pure so the mapping table is unit-
    // testable without a running Excel.
    //
    //   #VALUE!  malformed input (bad JSON, unparseable cell, misaligned range)
    //   #NAME?   unknown keyword (frequency / day count / spread type / field)
    //   #REF!    handle or ticker not found, wrong object kind
    //   #NUM!    solver failure, non-finite result, business-rule violation
    //   #N/A     native library not loaded (see CX.DIAG)
    internal static class ErrorMapper
    {
        public static object ToExcelError(Exception ex) => ex switch
        {
            ConvexException cx => cx.Code switch
            {
                ErrorCodes.InvalidHandle => ExcelError.ExcelErrorRef,
                ErrorCodes.Analytics => ExcelError.ExcelErrorNum,
                ErrorCodes.UnknownToken => ExcelError.ExcelErrorName,
                _ => ExcelError.ExcelErrorValue,
            },
            DllNotFoundException => ExcelError.ExcelErrorNA,
            EntryPointNotFoundException => ExcelError.ExcelErrorNA,
            BadImageFormatException => ExcelError.ExcelErrorNA,
            _ => ExcelError.ExcelErrorValue,
        };
    }
}
