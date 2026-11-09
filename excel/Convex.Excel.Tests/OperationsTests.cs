using System;
using System.IO;
using System.Text.RegularExpressions;
using Convex.Excel;
using Xunit;

namespace Convex.Excel.Tests
{
    public class OperationsTests
    {
        [Fact]
        public void The_add_in_carries_the_workspace_version()
        {
            var dir = new DirectoryInfo(AppContext.BaseDirectory);
            while (dir != null && !File.Exists(Path.Combine(dir.FullName, "Cargo.toml"))) dir = dir.Parent;
            Assert.NotNull(dir);
            var cargo = File.ReadAllText(Path.Combine(dir!.FullName, "Cargo.toml"));
            var workspace = Regex.Match(cargo, "(?m)^version\\s*=\\s*\"([^\"]+)\"").Groups[1].Value;

            Assert.Equal(workspace, NativeLoader.AddInVersion);
        }
    }
}
