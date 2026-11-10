using System;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.Linq;
using System.Runtime.ExceptionServices;
using System.Text.RegularExpressions;
using System.Threading;
using System.Windows.Forms;
using Convex.Excel.Forms;
using Xunit;

namespace Convex.Excel.Tests
{
    public class FormTests
    {
        private static readonly Dictionary<string, Func<Form>> Make = new()
        {
            ["Bond builder"] = () => new BondBuilderForm(),
            ["Curve builder"] = () => new CurveBuilderForm(),
            ["Curve viewer"] = () => new CurveViewerForm(),
            ["Pricing ticket"] = () => new PricingTicketForm(),
            ["Spread ticket"] = () => new SpreadTicketForm(),
            ["Scenario"] = () => new ScenarioForm(),
            ["Limit check"] = () => new LimitTicketForm(),
            ["Objects"] = () => new ObjectBrowserForm(),
            ["Schemas"] = () => new SchemaBrowserForm(),
            ["Settings"] = () => new SettingsForm(),
            ["Server"] = () => new ServerSettingsForm(),
            ["Error log"] = () => new ErrorLogForm(),
        };

        public static IEnumerable<object[]> Forms() => Make.Keys.Select(name => new object[] { name });

        // The forms set their sizes in pixels while their text grows with the
        // Windows display scale, so a label column of fixed width wraps and clips.
        [Theory]
        [MemberData(nameof(Forms))]
        public void No_label_is_cropped_when_the_text_is_half_as_large_again(string name) => OnSta(() =>
        {
            using var form = Make[name]();
            form.StartPosition = FormStartPosition.Manual;
            form.Location = new Point(-5000, -5000);
            form.ShowInTaskbar = false;
            form.Font = new Font(form.Font.FontFamily, form.Font.Size * 1.5f);
            form.Show();
            Application.DoEvents();

            var cropped = new HashSet<string>();
            void Check()
            {
                // Settings reads the RTD throttle from Excel; here there is none, so it shows the failure.
                foreach (var label in All(form).OfType<Label>().Where(l => l.Visible && l.Text.Length > 0 && !l.Text.StartsWith("unavailable:")))
                    if (label.Width < label.GetPreferredSize(Size.Empty).Width || label.Right > label.Parent.ClientSize.Width)
                        cropped.Add(label.Text);
            }

            Check();
            foreach (var tabs in All(form).OfType<TabControl>().ToList())
                for (int i = 0; i < tabs.TabCount; i++)
                {
                    tabs.SelectedIndex = i;
                    Application.DoEvents();
                    Check();
                }

            form.Close();
            Assert.Empty(cropped);
        });

        // xlfCaller is valid only during a cell calculation. Called from a form
        // it did not return, and Excel hung or crashed. Without Excel it just
        // throws, so no test run here can see the difference: this reads the code.
        [Fact]
        public void A_form_never_asks_Excel_for_the_calling_cell()
        {
            var dir = new DirectoryInfo(AppContext.BaseDirectory);
            while (dir != null && !File.Exists(Path.Combine(dir.FullName, "Cargo.toml"))) dir = dir.Parent;
            Assert.NotNull(dir);
            var addIn = Path.Combine(dir!.FullName, "excel", "Convex.Excel");

            var cx = File.ReadAllText(Path.Combine(addIn, "Cx.cs"));
            foreach (var build in new[] { "BuildBond", "BuildCurve" })
            {
                var body = Regex.Match(cx, "public static ulong " + build + @"\(JObject spec\)\s*\{.*?\r?\n        \}", RegexOptions.Singleline).Value;
                Assert.Contains("_from_json", body);
                Assert.DoesNotContain("ApplyCallerKey", body);
            }

            foreach (var file in Directory.GetFiles(Path.Combine(addIn, "forms"), "*.cs"))
            {
                var text = File.ReadAllText(file);
                foreach (var banned in new[] { "XlCall.", "CxCaller.", "InCell(" })
                    Assert.False(text.Contains(banned), Path.GetFileName(file) + " uses " + banned);
            }
        }

        private static IEnumerable<Control> All(Control root) =>
            root.Controls.Cast<Control>().SelectMany(c => new[] { c }.Concat(All(c)));

        private static void OnSta(Action body)
        {
            ExceptionDispatchInfo? failure = null;
            var thread = new Thread(() =>
            {
                try { body(); }
                catch (Exception ex) { failure = ExceptionDispatchInfo.Capture(ex); }
            });
            thread.SetApartmentState(ApartmentState.STA);
            thread.Start();
            thread.Join();
            failure?.Throw();
        }
    }
}
