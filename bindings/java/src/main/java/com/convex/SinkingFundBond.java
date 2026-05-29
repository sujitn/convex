package com.convex;

import com.convex.internal.Json;
import com.fasterxml.jackson.databind.node.ArrayNode;
import com.fasterxml.jackson.databind.node.ObjectNode;

import java.math.BigDecimal;
import java.time.LocalDate;
import java.util.ArrayList;
import java.util.List;

/**
 * Builder for a sinking-fund bond, mirroring the Rust {@code SinkingFundBond} —
 * a fixed-coupon bond with a scheduled amortisation (sink) of principal. See
 * the module README for usage.
 */
public final class SinkingFundBond {

    private SinkingFundBond() {}

    public static Builder builder() {
        return new Builder();
    }

    /** One sink payment: a date, the % of original face retired, and the sink price (% of par). */
    public record Payment(LocalDate date, double amountPctOfFace, double pricePctOfPar) {
        /** Defaults the sink price to par (100). */
        public Payment(LocalDate date, double amountPctOfFace) {
            this(date, amountPctOfFace, 100.0);
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
        private final List<Payment> schedule = new ArrayList<>();

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

        /** Retire {@code amountPctOfFace}% of original face on {@code date} at par. */
        public Builder sink(LocalDate date, double amountPctOfFace) {
            schedule.add(new Payment(date, amountPctOfFace));
            return this;
        }

        public Builder sink(Payment payment) {
            schedule.add(payment);
            return this;
        }

        public Bond build() {
            if (schedule.isEmpty()) {
                throw new IllegalArgumentException("sinking-fund bond requires at least one sink payment");
            }
            ObjectNode spec = Specs.fixedRate(
                    "sinking_fund", cusip, isin, name, couponRate, frequency,
                    issue, maturity, dayCount, currency, faceValue);
            ArrayNode sched = spec.putArray("schedule");
            for (Payment p : schedule) {
                ObjectNode n = Json.object();
                n.put("date", p.date().toString());
                n.put("amount", p.amountPctOfFace());
                n.put("price", p.pricePctOfPar());
                sched.add(n);
            }
            return Bond.fromSpecJson(Json.write(spec));
        }
    }
}
