using System;
using System.IO;
using Convex.Excel;
using Newtonsoft.Json.Linq;
using Xunit;

namespace Convex.Excel.Tests
{
    public class LibraryTests
    {
        [Fact]
        public void RawJsonChunksPreserveDigitsAndUnicodeAndRejectNumericCells()
        {
            string text = new string('x', LibraryCells.CellCharacters - 1) + "\U0001f4b5" +
                "{\"cash\":\"99999999999999999.00000000001\",\"cash\":\"1\"}";
            var chunks = LibraryCells.Split(text);
            Assert.Equal(text, LibraryCells.Join(chunks));
            for (int row = 0; row < chunks.GetLength(0); row++)
                Assert.InRange(((string)chunks[row, 0]).Length, 1, LibraryCells.CellCharacters);
            Assert.Throws<ArgumentException>(() => LibraryCells.Join(new object[,] { { 1.0 } }));
            Assert.Throws<ArgumentException>(() => LibraryCells.Single(text));
            Assert.Throws<ArgumentException>(() => LibraryCells.ValidateInput("{}\0discarded"));
            Assert.Throws<ArgumentException>(() => LibraryCells.ValidateInput("\ud800"));
            Assert.Throws<ArgumentException>(() => LibraryCells.ValidateInput(new string('x', LibraryCells.InputBytes + 1)));
        }

        [Fact]
        public void ActualPInvokePreservesSharedCorpusResultsAndErrors()
        {
            var document = JObject.Parse(File.ReadAllText(Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "library-parity.json")));
            foreach (JObject example in document["cases"]!)
            {
                string request = example["request"]!.ToString(Newtonsoft.Json.Formatting.None);
                string native = Cx.Library(request);
                string ranged = ResponseCells.Join(LibraryCells.Split(native));
                Assert.Equal(native, ranged);
                var result = JObject.Parse(native);
                Assert.Equal(example["expected_error"]!.Type == JTokenType.Null ? "true" : "false", (string?)result["ok"]);
                if (example["expected_error"]!.Type != JTokenType.Null)
                    Assert.Equal((string?)example["expected_error"], (string?)result["error"]!["code"]);
                Assert.True(JToken.DeepEquals(example["expected_envelope"], result), (string?)example["id"]);
            }
        }
    }
}
