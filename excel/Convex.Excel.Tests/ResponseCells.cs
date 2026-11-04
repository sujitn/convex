using System.Text;
using Convex.Excel;
using Xunit;

namespace Convex.Excel.Tests
{
    // Reassemble CX.LIBRARY.RANGE output. Responses may reach the 16 MiB native
    // bound, so this must not apply the 2 MiB request bound of LibraryCells.Join.
    internal static class ResponseCells
    {
        internal static string Join(object[,] cells)
        {
            var text = new StringBuilder();
            foreach (object cell in cells)
            {
                string part = Assert.IsType<string>(cell);
                Assert.InRange(part.Length, 1, LibraryCells.CellCharacters);
                text.Append(part);
            }
            return text.ToString();
        }
    }
}
