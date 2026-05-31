package com.convex;

/** Floating-rate index. Wire values match {@code convex_analytics::dto::RateIndexCode} (snake_case). */
public enum RateIndex {
    SOFR("sofr"),
    SONIA("sonia"),
    ESTR("estr"),
    TONAR("tonar"),
    SARON("saron"),
    CORRA("corra"),
    EURIBOR_3M("euribor3m"),
    EURIBOR_6M("euribor6m"),
    TIBOR_3M("tibor3m");

    private final String wire;

    RateIndex(String wire) {
        this.wire = wire;
    }

    public String wire() {
        return wire;
    }
}
