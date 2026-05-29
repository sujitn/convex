package com.convex;

import com.convex.internal.Json;
import com.fasterxml.jackson.databind.node.ObjectNode;

import java.math.BigDecimal;
import java.time.LocalDate;
import java.util.Objects;

/**
 * Package-private helpers that assemble the JSON {@code BondSpec} / {@code CurveSpec}
 * shapes consumed by {@code crates/convex-analytics::dto}. Centralised so the
 * builders stay declarative and required-field validation lives in one place.
 */
final class Specs {

    private Specs() {}

    /** Shared fixed-coupon spec (used by fixed_rate, callable, sinking_fund). */
    static ObjectNode fixedRate(String type, String cusip, String isin, String name,
                                BigDecimal couponRate, Frequency frequency,
                                LocalDate issue, LocalDate maturity,
                                DayCount dayCount, Currency currency, BigDecimal faceValue) {
        require(couponRate, "couponRate");
        require(issue, "issue");
        require(maturity, "maturity");

        ObjectNode n = Json.object();
        n.put("type", type);
        applyIdentifier(n, cusip, isin, name);
        n.put("coupon_rate", couponRate);
        n.put("frequency", frequency.wire());
        n.put("issue", issue.toString());
        n.put("maturity", maturity.toString());
        n.put("day_count", dayCount.wire());
        n.put("currency", currency.wire());
        n.put("face_value", faceValue);
        return n;
    }

    static void applyIdentifier(ObjectNode n, String cusip, String isin, String name) {
        if (cusip != null) {
            n.put("cusip", cusip);
        } else if (isin != null) {
            n.put("isin", isin);
        } else if (name != null) {
            n.put("name", name);
        }
    }

    static <T> T require(T value, String field) {
        return Objects.requireNonNull(value, () -> "missing required field: " + field);
    }
}
