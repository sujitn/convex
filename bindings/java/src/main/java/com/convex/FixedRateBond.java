package com.convex;

import com.convex.internal.Json;
import com.fasterxml.jackson.databind.node.ObjectNode;

import java.math.BigDecimal;
import java.time.LocalDate;

/**
 * Builder for a fixed-rate (bullet) bond, mirroring {@code FixedRateBond::builder()}
 * on the Rust side. Coupon is a decimal (0.05 = 5%); see the module README for
 * a usage example.
 */
public final class FixedRateBond {

    private FixedRateBond() {}

    public static Builder builder() {
        return new Builder();
    }

    public static final class Builder {
        private String cusip;
        private String isin;
        private String name;
        private BigDecimal couponRate;
        private Frequency frequency = Frequency.SEMI_ANNUAL;
        private LocalDate maturity;
        private LocalDate issue;
        private DayCount dayCount = DayCount.THIRTY_360_US;
        private Currency currency = Currency.USD;
        private BigDecimal faceValue = new BigDecimal("100");

        public Builder cusip(String v) { this.cusip = v; return this; }
        public Builder isin(String v) { this.isin = v; return this; }
        public Builder name(String v) { this.name = v; return this; }
        public Builder couponRate(BigDecimal v) { this.couponRate = v; return this; }
        public Builder frequency(Frequency v) { this.frequency = v; return this; }
        public Builder maturity(LocalDate v) { this.maturity = v; return this; }
        public Builder issue(LocalDate v) { this.issue = v; return this; }
        public Builder dayCount(DayCount v) { this.dayCount = v; return this; }
        public Builder currency(Currency v) { this.currency = v; return this; }
        public Builder faceValue(BigDecimal v) { this.faceValue = v; return this; }

        public Bond build() {
            ObjectNode spec = Specs.fixedRate(
                    "fixed_rate", cusip, isin, name, couponRate, frequency,
                    issue, maturity, dayCount, currency, faceValue);
            return Bond.fromSpecJson(Json.write(spec));
        }
    }
}
