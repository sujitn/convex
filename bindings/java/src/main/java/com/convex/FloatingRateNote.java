package com.convex;

import com.convex.internal.Json;
import com.fasterxml.jackson.databind.node.ObjectNode;

import java.math.BigDecimal;
import java.time.LocalDate;

/**
 * Builder for a floating-rate note, mirroring the Rust {@code FloatingRateNote}.
 * Spread is in basis points over the index; see the module README for usage.
 */
public final class FloatingRateNote {

    private FloatingRateNote() {}

    public static Builder builder() {
        return new Builder();
    }

    public static final class Builder {
        private String cusip;
        private String isin;
        private String name;
        private BigDecimal spreadBps;
        private RateIndex rateIndex = RateIndex.SOFR;
        private LocalDate maturity;
        private LocalDate issue;
        private Frequency frequency = Frequency.QUARTERLY;
        private DayCount dayCount = DayCount.ACT_360;
        private Currency currency = Currency.USD;
        private BigDecimal faceValue = new BigDecimal("100");
        private BigDecimal cap;
        private BigDecimal floor;

        public Builder cusip(String v) { this.cusip = v; return this; }
        public Builder isin(String v) { this.isin = v; return this; }
        public Builder name(String v) { this.name = v; return this; }
        /** Spread over the index, in basis points. */
        public Builder spreadBps(BigDecimal v) { this.spreadBps = v; return this; }
        public Builder rateIndex(RateIndex v) { this.rateIndex = v; return this; }
        public Builder maturity(LocalDate v) { this.maturity = v; return this; }
        public Builder issue(LocalDate v) { this.issue = v; return this; }
        public Builder frequency(Frequency v) { this.frequency = v; return this; }
        public Builder dayCount(DayCount v) { this.dayCount = v; return this; }
        public Builder currency(Currency v) { this.currency = v; return this; }
        public Builder faceValue(BigDecimal v) { this.faceValue = v; return this; }
        /** Optional coupon cap (decimal, e.g. 0.06 for 6%). */
        public Builder cap(BigDecimal v) { this.cap = v; return this; }
        /** Optional coupon floor (decimal). */
        public Builder floor(BigDecimal v) { this.floor = v; return this; }

        public Bond build() {
            Specs.require(spreadBps, "spreadBps");
            Specs.require(issue, "issue");
            Specs.require(maturity, "maturity");
            Specs.require(rateIndex, "rateIndex");
            Specs.require(frequency, "frequency");
            Specs.require(dayCount, "dayCount");
            Specs.require(currency, "currency");
            Specs.require(faceValue, "faceValue");
            if (cap != null && floor != null && cap.compareTo(floor) < 0) {
                throw new IllegalArgumentException("FRN cap (" + cap + ") must be >= floor (" + floor + ")");
            }

            ObjectNode spec = Json.object();
            spec.put("type", "floating_rate");
            Specs.applyIdentifier(spec, cusip, isin, name);
            spec.put("spread_bps", spreadBps);
            spec.put("rate_index", rateIndex.wire());
            spec.put("issue", issue.toString());
            spec.put("maturity", maturity.toString());
            spec.put("frequency", frequency.wire());
            spec.put("day_count", dayCount.wire());
            spec.put("currency", currency.wire());
            spec.put("face_value", faceValue);
            if (cap != null) {
                spec.put("cap", cap);
            }
            if (floor != null) {
                spec.put("floor", floor);
            }
            return Bond.fromSpecJson(Json.write(spec));
        }
    }
}
