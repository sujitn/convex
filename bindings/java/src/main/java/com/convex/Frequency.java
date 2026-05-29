package com.convex;

/** Coupon / compounding frequency. Wire values match {@code convex_core::types::Frequency}. */
public enum Frequency {
    ANNUAL("Annual"),
    SEMI_ANNUAL("SemiAnnual"),
    QUARTERLY("Quarterly"),
    MONTHLY("Monthly"),
    ZERO("Zero");

    private final String wire;

    Frequency(String wire) {
        this.wire = wire;
    }

    /** The JSON token the native layer expects. */
    public String wire() {
        return wire;
    }
}
