package com.convex;

/** Day-count convention. Wire values match {@code convex_core::daycounts::DayCountConvention}. */
public enum DayCount {
    THIRTY_360_US("Thirty360US"),
    THIRTY_360_E("Thirty360E"),
    THIRTY_360_ICMA("Thirty360Icma"),
    ACT_360("Act360"),
    ACT_365_FIXED("Act365Fixed"),
    ACT_ACT_ICMA("ActActIcma"),
    ACT_ACT_ISDA("ActActIsda"),
    SIMPLE("Simple"),
    DAILY("Daily");

    private final String wire;

    DayCount(String wire) {
        this.wire = wire;
    }

    public String wire() {
        return wire;
    }
}
