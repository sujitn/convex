using System;
using System.Windows.Forms;

namespace Convex.Excel.Helpers
{
    // Shared form chrome.
    internal static class FormUi
    {
        public static Button NewButton(string text, EventHandler onClick)
        {
            var b = new Button { Text = text, AutoSize = true, Padding = new Padding(8, 2, 8, 2) };
            b.Click += onClick;
            return b;
        }
    }
}
