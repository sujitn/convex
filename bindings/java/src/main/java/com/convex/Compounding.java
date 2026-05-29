package com.convex;

/** Compounding basis. Wire values match {@code convex_core::types::Compounding}. */
public enum Compounding {
    ANNUAL("Annual"),
    SEMI_ANNUAL("SemiAnnual"),
    QUARTERLY("Quarterly"),
    MONTHLY("Monthly"),
    CONTINUOUS("Continuous"),
    SIMPLE("Simple"),
    DAILY("Daily");

    private final String wire;

    Compounding(String wire) {
        this.wire = wire;
    }

    public String wire() {
        return wire;
    }
}
