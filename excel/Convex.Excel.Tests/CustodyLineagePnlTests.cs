using System;
using System.IO;
using Convex.Excel;
using Newtonsoft.Json.Linq;
using Xunit;

namespace Convex.Excel.Tests
{
    public class CustodyLineagePnlTests
    {
        [Fact]
        public void ActualPInvokePreservesSuccessiveCustodyAndActualDisposal()
        {
            var document = JObject.Parse(File.ReadAllText(Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "custody-lineage-pnl-parity.json")));
            Assert.False((bool)document["certified"]!);
            foreach (JObject example in document["cases"]!)
            {
                string wire = (string)example["request_json"]!;
                Assert.True(JToken.DeepEquals(example["request"], JToken.Parse(wire)));
                string input = LibraryCells.Join(LibraryCells.Split(wire));
                Assert.Equal(wire, input);
                string native = Cx.Library(input);
                Assert.Equal(native, ResponseCells.Join(LibraryCells.Split(native)));
                var result = JObject.Parse(native);
                Assert.Equal(example["expected_error"]!.Type == JTokenType.Null ? "true" : "false", (string?)result["ok"]);
                Assert.True(JToken.DeepEquals(example["expected_envelope"], result), (string?)example["id"]);
            }
        }
    }
}
