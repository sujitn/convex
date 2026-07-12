using System;
using System.Windows.Forms;

namespace Convex.Excel.Helpers
{
    // Picker-combo helpers over the shared "#CX#101  ·  kind  ·  name" label
    // format. Reload preserves selection by handle token, not index (indices
    // shift whenever objects are added or released).
    internal static class ComboReload
    {
        private static readonly string[] Separator = { "  ·  " };
        public static void Reload(ComboBox combo, object[] items)
        {
            var prevToken = TokenOf(combo.SelectedItem);
            combo.BeginUpdate();
            combo.Items.Clear();
            combo.Items.AddRange(items);
            int idx = -1;
            if (prevToken != null)
                for (int i = 0; i < combo.Items.Count; i++)
                    if (TokenOf(combo.Items[i]) == prevToken) { idx = i; break; }
            if (combo.Items.Count > 0)
                combo.SelectedIndex = idx >= 0 ? idx : 0;
            combo.EndUpdate();
        }

        // Leading token of a picker label ("#CX#101  ·  fixed_rate  ·  T 5s35").
        public static string? TokenOf(object? item)
        {
            var t = item?.ToString();
            if (string.IsNullOrEmpty(t)) return null;
            int sep = t!.IndexOf(' ');
            return sep < 0 ? t : t.Substring(0, sep);
        }

        public static ulong HandleOf(ComboBox combo, string field)
        {
            var token = TokenOf(combo.SelectedItem)
                ?? throw new ConvexException($"select a {field}");
            return CxParse.AsHandle(token, field);
        }

        // Name segment of the label, or null without a human identifier.
        public static string? NameOf(object? item)
        {
            var t = item?.ToString();
            if (string.IsNullOrEmpty(t)) return null;
            var parts = t!.Split(Separator, StringSplitOptions.None);
            return parts.Length >= 3 ? parts[parts.Length - 1].Trim() : null;
        }
    }
}
