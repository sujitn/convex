using System;
using ExcelDna.Integration;

namespace Convex.Excel.Helpers
{
    // Worksheet I/O: write a 2D grid into the active selection, stamp the
    // formula that produced it, and read selected cells back as objects.
    //
    // Forms route through here so they never bypass Excel's calc graph.
    internal static class SheetHelpers
    {
        // Write `grid` starting at the active cell; resize the selection to fit.
        // Returns the address that was written (e.g. "Sheet1!A1:B5").
        public static string WriteGridAtSelection(object[,] grid)
        {
            var app = ExcelDnaUtil.Application;
            dynamic selection = ((dynamic)app).Selection;
            int rows = grid.GetLength(0), cols = grid.GetLength(1);
            dynamic anchor = selection.Cells[1, 1];
            dynamic range = anchor.Resize(rows, cols);
            range.Value2 = grid;
            return range.Address;
        }

        // Stamp a single formula at a target cell. `formula` should start with `=`.
        public static string WriteFormulaAtSelection(string formula)
        {
            var app = ExcelDnaUtil.Application;
            dynamic selection = ((dynamic)app).Selection;
            dynamic anchor = selection.Cells[1, 1];
            anchor.Formula2 = formula;
            return anchor.Address;
        }

        // Read the active selection as either a scalar or 2D array of objects.
        public static object ReadSelection()
        {
            var app = ExcelDnaUtil.Application;
            dynamic selection = ((dynamic)app).Selection;
            return selection.Value2;
        }

        // Show a small status message in the Excel status bar.
        public static void Status(string message)
        {
            try
            {
                var app = ExcelDnaUtil.Application;
                ((dynamic)app).StatusBar = message;
            }
            catch
            {
                // Status bar is best-effort; don't surface failures.
            }
        }

        public static void ClearStatus()
        {
            try { ((dynamic)ExcelDnaUtil.Application).StatusBar = false; } catch { }
        }
    }
}
