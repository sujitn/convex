package com.convex;

import com.convex.internal.Json;
import com.fasterxml.jackson.databind.node.ArrayNode;
import com.fasterxml.jackson.databind.node.ObjectNode;

import java.math.BigDecimal;
import java.time.LocalDate;
import java.util.ArrayList;
import java.util.List;

/**
 * Builder for a callable bond, mirroring the Rust {@code CallableBond}.
 *
 * <p>Add call entries with {@link Builder#call(LocalDate, double)}. For a
 * make-whole bond, set {@link Builder#callStyle(CallStyle)} to
 * {@link CallStyle#MAKE_WHOLE} and supply {@link Builder#makeWholeSpreadBps(double)}.
 */
public final class CallableBond {

    private CallableBond() {}

    public static Builder builder() {
        return new Builder();
    }

    /** A single call (or put) entry: a date, a price per 100, and an optional window end. */
    public record CallEntry(LocalDate date, double pricePer100, LocalDate endDate) {
        public CallEntry(LocalDate date, double pricePer100) {
            this(date, pricePer100, null);
        }
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
        private CallStyle callStyle = CallStyle.AMERICAN;
        private Double makeWholeSpreadBps;
        private final List<CallEntry> calls = new ArrayList<>();
        private final List<CallEntry> puts = new ArrayList<>();

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
        public Builder callStyle(CallStyle v) { this.callStyle = v; return this; }
        public Builder makeWholeSpreadBps(double v) { this.makeWholeSpreadBps = v; return this; }

        public Builder call(LocalDate date, double pricePer100) {
            calls.add(new CallEntry(date, pricePer100));
            return this;
        }

        public Builder call(CallEntry entry) {
            calls.add(entry);
            return this;
        }

        public Builder put(LocalDate date, double pricePer100) {
            puts.add(new CallEntry(date, pricePer100));
            return this;
        }

        public Bond build() {
            if (calls.isEmpty()) {
                throw new IllegalArgumentException("callable bond requires at least one call entry");
            }
            ObjectNode spec = Specs.fixedRate(
                    "callable", cusip, isin, name, couponRate, frequency,
                    issue, maturity, dayCount, currency, faceValue);
            spec.put("call_style", callStyle.wire());
            if (makeWholeSpreadBps != null) {
                spec.put("make_whole_spread_bps", makeWholeSpreadBps);
            }
            spec.set("call_schedule", entries(calls));
            if (!puts.isEmpty()) {
                spec.set("put_schedule", entries(puts));
            }
            return Bond.fromSpecJson(Json.write(spec));
        }

        private static ArrayNode entries(List<CallEntry> list) {
            ArrayNode arr = Json.mapper().createArrayNode();
            for (CallEntry e : list) {
                ObjectNode n = Json.object();
                n.put("date", e.date().toString());
                n.put("price", e.pricePer100());
                if (e.endDate() != null) {
                    n.put("end_date", e.endDate().toString());
                }
                arr.add(n);
            }
            return arr;
        }
    }
}
