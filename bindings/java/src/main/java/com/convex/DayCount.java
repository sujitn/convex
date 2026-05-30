package com.convex;

/** Day-count convention. Wire values match {@code convex_core::daycounts::DayCountConvention}. */
public enum DayCount {
    ACT_360("Act360"),
    ACT_365_FIXED("Act365Fixed"),
    ACT_365_LEAP("Act365Leap"),
    ACT_ACT_ISDA("ActActIsda"),
    ACT_ACT_ICMA("ActActIcma"),
    ACT_ACT_AFB("ActActAfb"),
    THIRTY_360_US("Thirty360US"),
    THIRTY_360_E("Thirty360E"),
    THIRTY_360_E_ISDA("Thirty360EIsda"),
    THIRTY_360_GERMAN("Thirty360German");

    private final String wire;

    DayCount(String wire) {
        this.wire = wire;
    }

    public String wire() {
        return wire;
    }
}
