using System;
using System.Globalization;
using System.Threading;
using Convex.Excel;
using ExcelDna.Integration;
using Xunit;

namespace Convex.Excel.Tests
{
    public class StrictDoublesTests
    {
        [Fact]
        public void Parses_2d_grid_of_numbers_and_numeric_strings()
        {
            var grid = new object[,] { { 1.0, "2.5" }, { 3.0, 4.0 } };
            Assert.Equal(new[] { 1.0, 2.5, 3.0, 4.0 }, CxParse.AsDoublesStrict(grid, "t"));
        }

        [Fact]
        public void Trailing_blanks_are_trimmed()
        {
            var grid = new object[,] { { 1.0 }, { 2.0 }, { ExcelEmpty.Value }, { ExcelEmpty.Value } };
            Assert.Equal(new[] { 1.0, 2.0 }, CxParse.AsDoublesStrict(grid, "t"));
        }

        [Fact]
        public void Interior_blank_throws_with_position()
        {
            var grid = new object[,] { { 1.0 }, { ExcelEmpty.Value }, { 3.0 } };
            var ex = Assert.Throws<ConvexException>(() => CxParse.AsDoublesStrict(grid, "rates"));
            Assert.Contains("rates", ex.Message);
            Assert.Contains("row 2", ex.Message);
        }

        [Fact]
        public void Unparseable_cell_throws_instead_of_being_skipped()
        {
            var grid = new object[,] { { 1.0 }, { "oops" }, { 3.0 } };
            var ex = Assert.Throws<ConvexException>(() => CxParse.AsDoublesStrict(grid, "rates"));
            Assert.Contains("oops", ex.Message);
            Assert.Contains("row 2", ex.Message);
        }

        [Fact]
        public void Scalar_double_yields_single_element()
        {
            Assert.Equal(new[] { 5.0 }, CxParse.AsDoublesStrict(5.0, "t"));
        }

        [Fact]
        public void Blank_range_yields_empty()
        {
            Assert.Empty(CxParse.AsDoublesStrict(ExcelMissing.Value, "t"));
        }
    }

    public class StrictDatesTests : IDisposable
    {
        private readonly CultureInfo _saved = Thread.CurrentThread.CurrentCulture;

        public void Dispose() => Thread.CurrentThread.CurrentCulture = _saved;

        [Fact]
        public void Oadate_and_datetime_cells_pass_through()
        {
            var date = new DateTime(2030, 6, 15);
            var grid = new object[,] { { date.ToOADate() }, { date } };
            Assert.Equal(new[] { date, date }, CxParse.AsDatesStrict(grid, "d"));
        }

        [Fact]
        public void Iso_strings_parse_regardless_of_machine_culture()
        {
            Thread.CurrentThread.CurrentCulture = new CultureInfo("en-GB"); // dd/MM
            var got = CxParse.AsDatesStrict(new object[,] { { "2025-04-03" } }, "d");
            Assert.Equal(new DateTime(2025, 4, 3), got[0]);
        }

        [Fact]
        public void Slash_dates_use_invariant_not_machine_culture()
        {
            // Under en-GB the old code read 03/04/2025 as 3 April; invariant
            // reads it as 4 March. Culture must not change the result.
            Thread.CurrentThread.CurrentCulture = new CultureInfo("en-GB");
            var got = CxParse.AsDatesStrict(new object[,] { { "03/04/2025" } }, "d");
            Assert.Equal(new DateTime(2025, 3, 4), got[0]);
        }

        [Fact]
        public void Unparseable_date_throws()
        {
            var ex = Assert.Throws<ConvexException>(
                () => CxParse.AsDatesStrict(new object[,] { { "not-a-date" } }, "call dates"));
            Assert.Contains("call dates", ex.Message);
        }
    }

    public class StrictStringsTests
    {
        [Fact]
        public void Strings_are_trimmed_and_extracted()
        {
            var grid = new object[,] { { " swap " }, { "ois" } };
            Assert.Equal(new[] { "swap", "ois" }, CxParse.AsStringsStrict(grid, "kinds"));
        }

        [Fact]
        public void Interior_blank_throws()
        {
            var grid = new object[,] { { "swap" }, { "" }, { "ois" } };
            Assert.Throws<ConvexException>(() => CxParse.AsStringsStrict(grid, "kinds"));
        }
    }

    public class TokenParsingTests
    {
        [Theory]
        [InlineData("SA", "SemiAnnual")]
        [InlineData("a", "Annual")]
        [InlineData("Q", "Quarterly")]
        [InlineData("monthly", "Monthly")]
        public void Frequency_tokens_map(string token, string expected) =>
            Assert.Equal(expected, CxParse.AsFrequency(token));

        [Theory]
        [InlineData(1.0, "Annual")]
        [InlineData(2.0, "SemiAnnual")]
        [InlineData(4.0, "Quarterly")]
        [InlineData(12.0, "Monthly")]
        public void Frequency_numerics_map(double n, string expected) =>
            Assert.Equal(expected, CxParse.AsFrequency(n));

        [Fact]
        public void Unknown_numeric_frequency_throws_unknown_token()
        {
            var ex = Assert.Throws<ConvexException>(() => CxParse.AsFrequency(3.0));
            Assert.Equal(ErrorCodes.UnknownToken, ex.Code);
        }

        [Fact]
        public void Unknown_string_frequency_throws_unknown_token()
        {
            var ex = Assert.Throws<ConvexException>(() => CxParse.AsFrequency("X"));
            Assert.Equal(ErrorCodes.UnknownToken, ex.Code);
        }

        [Theory]
        [InlineData("Z", "ZSpread")]
        [InlineData("asw", "AssetSwapPar")]
        [InlineData("OAS", "OAS")]
        [InlineData("dm", "DiscountMargin")]
        public void Spread_types_map(string token, string expected) =>
            Assert.Equal(expected, CxParse.AsSpreadType(token));

        [Fact]
        public void Unknown_spread_type_throws_unknown_token()
        {
            var ex = Assert.Throws<ConvexException>(() => CxParse.AsSpreadType("W"));
            Assert.Equal(ErrorCodes.UnknownToken, ex.Code);
        }

        [Fact]
        public void Handle_string_and_numeric_forms_parse()
        {
            Assert.Equal(101UL, CxParse.AsHandle("#CX#101"));
            Assert.Equal(101UL, CxParse.AsHandle(101.0));
        }

        [Fact]
        public void Fractional_handle_throws()
        {
            Assert.Throws<ConvexException>(() => CxParse.AsHandle(101.5));
        }

        [Fact]
        public void Handle_refs_pass_numbers_and_handle_strings_as_numbers()
        {
            Assert.Equal(101UL, CxParse.AsHandleRef("#CX#101").ToObject<ulong>());
            Assert.Equal(101UL, CxParse.AsHandleRef(101.0).ToObject<ulong>());
            Assert.Equal(101UL, CxParse.AsHandleRef("101").ToObject<ulong>());
        }

        [Fact]
        public void Handle_refs_pass_tickers_through_as_strings()
        {
            Assert.Equal("912828YK0", CxParse.AsHandleRef(" 912828YK0 ").ToObject<string>());
            Assert.Equal("USD.SOFR", CxParse.AsHandleRef("USD.SOFR").ToObject<string>());
        }

        [Fact]
        public void Handle_ref_or_null_treats_blanks_as_null()
        {
            Assert.Null(CxParse.AsHandleRefOrNull(ExcelMissing.Value));
            Assert.Null(CxParse.AsHandleRefOrNull(""));
            Assert.NotNull(CxParse.AsHandleRefOrNull("USD.SOFR"));
        }
    }

    public class ErrorMapperTests
    {
        [Theory]
        [InlineData(ErrorCodes.InvalidInput, ExcelError.ExcelErrorValue)]
        [InlineData(ErrorCodes.InvalidHandle, ExcelError.ExcelErrorRef)]
        [InlineData(ErrorCodes.Analytics, ExcelError.ExcelErrorNum)]
        [InlineData(ErrorCodes.UnknownToken, ExcelError.ExcelErrorName)]
        [InlineData(ErrorCodes.Panic, ExcelError.ExcelErrorValue)]
        [InlineData("serialize", ExcelError.ExcelErrorValue)]
        public void Convex_exception_codes_map_to_excel_errors(string code, ExcelError expected) =>
            Assert.Equal(expected, ErrorMapper.ToExcelError(new ConvexException("m", code)));

        [Fact]
        public void Native_load_failures_map_to_na()
        {
            Assert.Equal(ExcelError.ExcelErrorNA, ErrorMapper.ToExcelError(new DllNotFoundException()));
            Assert.Equal(ExcelError.ExcelErrorNA, ErrorMapper.ToExcelError(new EntryPointNotFoundException()));
            Assert.Equal(ExcelError.ExcelErrorNA, ErrorMapper.ToExcelError(new BadImageFormatException()));
        }

        [Fact]
        public void Unclassified_exceptions_map_to_value()
        {
            Assert.Equal(ExcelError.ExcelErrorValue, ErrorMapper.ToExcelError(new InvalidOperationException()));
        }
    }
}
