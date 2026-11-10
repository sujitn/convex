using System;
using System.Collections.Generic;
using System.Linq;
using Convex.Excel.Installer;
using Xunit;

namespace Convex.Excel.Tests
{
    public class OpenRegistrationTests
    {
        private const string Root = @"C:\Users\me\AppData\Local\Convex\Excel\";
        private const string Xll = Root + @"0.15.0\Convex.Excel64.xll";
        private const string Ours = "/R \"" + Xll + "\"";
        private const string Older = "/R \"" + Root + "0.14.0\\Convex.Excel64.xll\"";
        private const string OtherA = "/R \"C:\\Tools\\a.xll\"";
        private const string OtherB = "\"C:\\Tools\\b.xlam\"";
        private const string OtherC = "/R \"C:\\Tools\\c.xll\"";

        private static Dictionary<string, string> Values(params string[] nameThenValue)
        {
            var values = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
            for (int i = 0; i < nameThenValue.Length; i += 2) values[nameThenValue[i]] = nameThenValue[i + 1];
            return values;
        }

        private static void Same(Dictionary<string, string> expected, Dictionary<string, string> actual) =>
            Assert.Equal(expected.OrderBy(v => v.Key), actual.OrderBy(v => v.Key));

        [Fact]
        public void An_empty_list_gets_OPEN_in_Excels_format() =>
            Same(Values("OPEN", Ours), OpenRegistration.Register(Values(), Xll, Root));

        [Fact]
        public void Other_add_ins_keep_their_names_and_ours_takes_the_next_index() =>
            Same(Values("OPEN", OtherA, "OPEN1", OtherB, "OPEN2", Ours),
                OpenRegistration.Register(Values("OPEN", OtherA, "OPEN1", OtherB), Xll, Root));

        [Fact]
        public void An_older_version_is_dropped_and_later_add_ins_move_down_before_ours_is_added() =>
            Same(Values("OPEN", OtherA, "OPEN1", OtherB, "OPEN2", Ours),
                OpenRegistration.Register(Values("OPEN", OtherA, "OPEN1", Older, "OPEN2", OtherB), Xll, Root));

        [Fact]
        public void Several_stale_values_of_ours_leave_exactly_one() =>
            Same(Values("OPEN", OtherA, "OPEN1", Ours),
                OpenRegistration.Register(Values("OPEN", Older, "OPEN1", OtherA, "OPEN2", Ours), Xll, Root));

        [Fact]
        public void Installing_twice_changes_nothing()
        {
            var once = OpenRegistration.Register(Values("OPEN", OtherA), Xll, Root);
            Same(once, OpenRegistration.Register(once, Xll, Root));
        }

        [Fact]
        public void Our_first_free_index_can_be_a_gap_that_was_already_there() =>
            Same(Values("OPEN", OtherA, "OPEN1", Ours, "OPEN2", OtherC),
                OpenRegistration.Register(Values("OPEN", OtherA, "OPEN2", OtherC), Xll, Root));

        [Fact]
        public void Uninstall_drops_ours_and_closes_the_gap() =>
            Same(Values("OPEN", OtherA, "OPEN1", OtherB),
                OpenRegistration.Unregister(Values("OPEN", OtherA, "OPEN1", Ours, "OPEN2", OtherB), Root));

        [Fact]
        public void Uninstall_of_the_only_value_leaves_nothing() =>
            Assert.Empty(OpenRegistration.Unregister(Values("OPEN", Ours), Root));

        [Fact]
        public void Uninstall_keeps_a_gap_that_was_already_there() =>
            Same(Values("OPEN", OtherA, "OPEN2", OtherC),
                OpenRegistration.Unregister(Values("OPEN", OtherA, "OPEN1", Ours, "OPEN3", OtherC), Root));

        [Fact]
        public void The_folder_is_matched_whatever_its_case_and_quoting() =>
            Assert.Empty(OpenRegistration.Unregister(Values(
                "OPEN", "\"" + Root.ToUpperInvariant() + "0.13.0\\X.XLL\"",
                "OPEN1", Root + "0.12.0\\Convex.Excel64.xll"), Root));

        [Fact]
        public void A_like_named_folder_elsewhere_is_not_ours()
        {
            var values = Values(
                "OPEN", "/R \"C:\\Tools\\Convex\\Excel\\Convex.Excel64.xll\"",
                "OPEN1", "/R \"C:\\Users\\me\\AppData\\Local\\Convex\\Excel2\\x.xll\"");
            Same(values, OpenRegistration.Unregister(values, Root));
        }

        [Theory]
        [InlineData("OPEN", true)]
        [InlineData("open", true)]
        [InlineData("OPEN1", true)]
        [InlineData("OPEN12", true)]
        [InlineData("OPEN0", false)]
        [InlineData("OPEN01", false)]
        [InlineData("OPENX", false)]
        [InlineData("REOPEN", false)]
        [InlineData("Options", false)]
        public void Only_the_auto_load_names_count(string name, bool counts) =>
            Assert.Equal(counts, OpenRegistration.IsOpenValue(name));

        [Fact]
        public void A_value_that_is_not_an_auto_load_name_never_comes_back() =>
            Same(Values("OPEN", OtherA), OpenRegistration.Unregister(Values("OPEN", OtherA, "OPENX", OtherB), Root));
    }
}
