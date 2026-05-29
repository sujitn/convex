package com.convex;

import com.convex.internal.Json;
import com.fasterxml.jackson.databind.node.ObjectNode;

import java.math.BigDecimal;
import java.time.LocalDate;

/**
 * Builder for a zero-coupon bond, mirroring the Rust {@code ZeroCouponBond}.
 * See the module README for usage.
 */
public final class ZeroCouponBond {

    private ZeroCouponBond() {}

    public static Builder builder() {
        return new Builder();
    }

    public static final class Builder {
        private String cusip;
        private String isin;
        private String name;
        private LocalDate maturity;
        private LocalDate issue;
        private Compounding compounding = Compounding.SEMI_ANNUAL;
        private DayCount dayCount = DayCount.ACT_ACT_ICMA;
        private Currency currency = Currency.USD;
        private BigDecimal faceValue = new BigDecimal("100");

        public Builder cusip(String v) { this.cusip = v; return this; }
        public Builder isin(String v) { this.isin = v; return this; }
        public Builder name(String v) { this.name = v; return this; }
        public Builder maturity(LocalDate v) { this.maturity = v; return this; }
        public Builder issue(LocalDate v) { this.issue = v; return this; }
        public Builder compounding(Compounding v) { this.compounding = v; return this; }
        public Builder dayCount(DayCount v) { this.dayCount = v; return this; }
        public Builder currency(Currency v) { this.currency = v; return this; }
        public Builder faceValue(BigDecimal v) { this.faceValue = v; return this; }

        public Bond build() {
            Specs.require(issue, "issue");
            Specs.require(maturity, "maturity");

            ObjectNode spec = Json.object();
            spec.put("type", "zero_coupon");
            Specs.applyIdentifier(spec, cusip, isin, name);
            spec.put("issue", issue.toString());
            spec.put("maturity", maturity.toString());
            spec.put("compounding", compounding.wire());
            spec.put("day_count", dayCount.wire());
            spec.put("currency", currency.wire());
            spec.put("face_value", faceValue);
            return Bond.fromSpecJson(Json.write(spec));
        }
    }
}
