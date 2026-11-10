using System;
using System.Collections.Generic;
using System.Linq;
using System.Text.RegularExpressions;

namespace Convex.Excel.Installer
{
    /// Excel loads the add-ins named by the values OPEN, OPEN1, OPEN2, ... under
    /// HKCU\Software\Microsoft\Office\16.0\Excel\Options. Each edit here is
    /// pure, values in and values out, so it is tested without a registry.
    /// Linked into Convex.Excel.Tests.
    internal static class OpenRegistration
    {
        private static readonly Regex OpenName =
            new("^OPEN([1-9][0-9]*)?$", RegexOptions.IgnoreCase | RegexOptions.CultureInvariant);

        public static bool IsOpenValue(string name) => OpenName.IsMatch(name);

        /// The values with our add-in at the first free index, after dropping
        /// anything already under `root`. A version's uninstall step ships in
        /// that version's MSI, so a later install must not rely on it.
        public static Dictionary<string, string> Register(
            IReadOnlyDictionary<string, string> values, string xllPath, string root)
        {
            var kept = Unregister(values, root);
            int index = 0;
            while (kept.ContainsKey(NameAt(index))) index++;
            kept[NameAt(index)] = "/R \"" + xllPath + "\"";
            return kept;
        }

        /// The values without anything under `root`. Later add-ins move down
        /// over the gap we leave, in case Excel stops at the first missing
        /// index. A gap that was already there stays, so what Excel loads of
        /// the other add-ins does not change.
        public static Dictionary<string, string> Unregister(
            IReadOnlyDictionary<string, string> values, string root)
        {
            var kept = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
            int removed = 0;
            foreach (var entry in values.Where(v => IsOpenValue(v.Key)).OrderBy(v => IndexOf(v.Key)))
            {
                if (entry.Value.IndexOf(root, StringComparison.OrdinalIgnoreCase) >= 0) removed++;
                else kept[NameAt(IndexOf(entry.Key) - removed)] = entry.Value;
            }
            return kept;
        }

        private static int IndexOf(string name)
        {
            var digits = OpenName.Match(name).Groups[1].Value;
            return digits.Length == 0 ? 0 : int.Parse(digits, System.Globalization.CultureInfo.InvariantCulture);
        }

        private static string NameAt(int index) => index == 0 ? "OPEN" : "OPEN" + index;
    }
}
