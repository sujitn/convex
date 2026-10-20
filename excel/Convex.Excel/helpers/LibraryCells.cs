using System;
using System.Text;
using ExcelDna.Integration;

namespace Convex.Excel
{
    // Preserve raw JSON text through cells; never parse cash into Excel numbers.
    internal static class LibraryCells
    {
        internal const int CellCharacters = 32760;
        internal const int InputBytes = 2 * 1024 * 1024;

        internal static string Join(object[,] cells)
        {
            if (cells.Length > 4096) throw new ArgumentException("At most 4096 JSON text cells.");
            var text = new StringBuilder();
            foreach (object cell in cells)
            {
                if (cell is null or ExcelEmpty or ExcelMissing) continue;
                if (cell is not string part) throw new ArgumentException("Library JSON cells must contain text.");
                if (part.Length > CellCharacters) throw new ArgumentException("JSON text exceeds one cell; use more rows.");
                text.Append(part);
                if (text.Length > InputBytes) throw new ArgumentException("Library input exceeds 2 MiB.");
            }
            string result = text.ToString();
            ValidateInput(result);
            return result;
        }

        internal static void ValidateInput(string request)
        {
            if (request is null || request.IndexOf('\0') >= 0)
                throw new ArgumentException("Library JSON must contain text without embedded NUL.");
            if (request.Length > InputBytes) throw new ArgumentException("Library input exceeds 2 MiB UTF-8.");
            int bytes;
            try { bytes = new UTF8Encoding(false, true).GetByteCount(request); }
            catch (EncoderFallbackException) { throw new ArgumentException("Library JSON contains an unpaired UTF-16 surrogate."); }
            if (bytes > InputBytes) throw new ArgumentException("Library input exceeds 2 MiB UTF-8.");
        }

        internal static string Single(string response)
        {
            if (response.Length > CellCharacters)
                throw new ArgumentException("Library output needs multiple cells; use CX.LIBRARY.RANGE.");
            return response;
        }

        internal static object[,] Split(string response)
        {
            // A cell boundary may split JSON tokens, but cannot split a Unicode pair.
            var parts = new System.Collections.Generic.List<string>();
            for (int offset = 0; offset < response.Length;)
            {
                int size = Math.Min(CellCharacters, response.Length - offset);
                if (offset + size < response.Length && char.IsHighSurrogate(response[offset + size - 1])) size--;
                parts.Add(response.Substring(offset, size));
                offset += size;
            }
            var cells = new object[parts.Count, 1];
            for (int i = 0; i < parts.Count; i++) cells[i, 0] = parts[i];
            return cells;
        }
    }
}
