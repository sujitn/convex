using System;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;

namespace Convex.Excel.Helpers
{
    // Single source of truth for the BondSpec / CurveSpec JSON the FFI accepts.
    // Used by both the cell UDFs (Functions.cs) and the ribbon forms so the
    // two paths can never drift.
    internal static class BondSpecs
    {
        public static JObject FixedRate(
            string id, double couponDecimal, string frequency,
            DateTime maturity, DateTime issue,
            string dayCount, string currency, double faceValue)
        {
            var spec = new JObject
            {
                ["type"] = "fixed_rate",
                ["coupon_rate"] = couponDecimal,
                ["frequency"] = frequency,
                ["maturity"] = Iso(maturity),
                ["issue"] = Iso(issue),
                ["day_count"] = dayCount,
                ["currency"] = currency,
                ["face_value"] = faceValue,
            };
            AttachId(spec, id);
            return spec;
        }

        public static JObject Callable(
            string id, double couponDecimal, string frequency,
            DateTime maturity, DateTime issue,
            JArray callSchedule, string callStyle, string dayCount)
        {
            var spec = new JObject
            {
                ["type"] = "callable",
                ["coupon_rate"] = couponDecimal,
                ["frequency"] = frequency,
                ["maturity"] = Iso(maturity),
                ["issue"] = Iso(issue),
                ["day_count"] = dayCount,
                ["call_schedule"] = callSchedule,
                ["call_style"] = callStyle,
            };
            AttachId(spec, id);
            return spec;
        }

        public static JObject Frn(
            string id, double spreadBps, DateTime maturity, DateTime issue,
            string rateIndex, string frequency, string dayCount,
            double? cap, double? floor)
        {
            var spec = new JObject
            {
                ["type"] = "floating_rate",
                ["spread_bps"] = spreadBps,
                ["maturity"] = Iso(maturity),
                ["issue"] = Iso(issue),
                ["rate_index"] = rateIndex,
                ["frequency"] = frequency,
                ["day_count"] = dayCount,
            };
            if (cap.HasValue) spec["cap"] = cap.Value;
            if (floor.HasValue) spec["floor"] = floor.Value;
            AttachId(spec, id);
            return spec;
        }

        public static JObject ZeroCoupon(
            string id, DateTime maturity, DateTime issue,
            string compounding, string dayCount)
        {
            var spec = new JObject
            {
                ["type"] = "zero_coupon",
                ["maturity"] = Iso(maturity),
                ["issue"] = Iso(issue),
                ["compounding"] = compounding,
                ["day_count"] = dayCount,
            };
            AttachId(spec, id);
            return spec;
        }

        public static JObject SinkingFund(
            string id, double couponDecimal, string frequency,
            DateTime maturity, DateTime issue, string dayCount,
            JArray schedule)
        {
            var spec = new JObject
            {
                ["type"] = "sinking_fund",
                ["coupon_rate"] = couponDecimal,
                ["frequency"] = frequency,
                ["maturity"] = Iso(maturity),
                ["issue"] = Iso(issue),
                ["day_count"] = dayCount,
                ["schedule"] = schedule,
            };
            AttachId(spec, id);
            return spec;
        }

        // Heuristic: 9 alphanumeric chars → CUSIP, 12 → ISIN, anything else → free name.
        private static void AttachId(JObject spec, string id)
        {
            var t = (id ?? "").Trim();
            if (t.Length == 0) return;
            if (t.Length == 9) spec["cusip"] = t;
            else if (t.Length == 12) spec["isin"] = t;
            else spec["name"] = t;
        }

        private static string Iso(DateTime dt) =>
            dt.ToString("yyyy-MM-dd", System.Globalization.CultureInfo.InvariantCulture);
    }

    internal static class CurveSpecs
    {
        public static JObject Discrete(
            string name, DateTime refDate,
            JArray tenors, JArray values,
            string valueKind, string interpolation,
            string dayCount, string compounding)
        {
            var spec = new JObject
            {
                ["type"] = "discrete",
                ["ref_date"] = Iso(refDate),
                ["tenors"] = tenors,
                ["values"] = values,
                ["value_kind"] = valueKind,
                ["interpolation"] = interpolation,
                ["day_count"] = dayCount,
                ["compounding"] = compounding,
            };
            if (!string.IsNullOrWhiteSpace(name)) spec["name"] = name;
            return spec;
        }

        public static JObject Bootstrap(
            string name, DateTime refDate, string method,
            JArray instruments, string interpolation, string dayCount)
        {
            var spec = new JObject
            {
                ["type"] = "bootstrap",
                ["ref_date"] = Iso(refDate),
                ["method"] = method,
                ["instruments"] = instruments,
                ["interpolation"] = interpolation,
                ["day_count"] = dayCount,
            };
            if (!string.IsNullOrWhiteSpace(name)) spec["name"] = name;
            return spec;
        }

        private static string Iso(DateTime dt) =>
            dt.ToString("yyyy-MM-dd", System.Globalization.CultureInfo.InvariantCulture);
    }
}
