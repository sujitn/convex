using System;

namespace Convex.Excel
{
    /// <summary>
    /// Helper class for formatting and parsing handles with #CX# prefix.
    /// Handles are displayed as #CX#100, #CX#101, etc.
    /// </summary>
    public static class HandleHelper
    {
        private const string HandlePrefix = "#CX#";

        /// <summary>
        /// Formats a handle for display in Excel (e.g., #CX#100).
        /// </summary>
        public static string Format(ulong handle)
        {
            return HandlePrefix + handle.ToString();
        }

        /// <summary>
        /// Parses a handle from Excel input.
        /// Accepts: #CX#100, #100, "100", or numeric 100.
        /// </summary>
        public static ulong Parse(object reference)
        {
            if (reference == null)
                return NativeMethods.INVALID_HANDLE;

            // Handle numeric input (double from Excel)
            if (reference is double d)
                return (ulong)d;

            // Handle string input
            if (reference is string s)
            {
                // Remove #CX# prefix if present
                if (s.StartsWith(HandlePrefix, StringComparison.OrdinalIgnoreCase))
                    s = s.Substring(HandlePrefix.Length);
                // Also handle legacy # prefix
                else if (s.StartsWith("#"))
                    s = s.Substring(1);

                // Try to parse as number
                if (ulong.TryParse(s, out ulong handle))
                    return handle;

                // Otherwise treat as name lookup
                return ConvexWrapper.Lookup(s);
            }

            return NativeMethods.INVALID_HANDLE;
        }

        /// <summary>
        /// Checks if a value looks like a handle (starts with #CX# or #).
        /// </summary>
        public static bool IsHandle(object reference)
        {
            if (reference is string s)
                return s.StartsWith("#");
            if (reference is double)
                return true;
            return false;
        }
    }
}
