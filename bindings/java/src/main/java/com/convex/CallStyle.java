package com.convex;

/** Embedded-call exercise style. Wire values match {@code convex_analytics::dto::CallStyle} (snake_case). */
public enum CallStyle {
    AMERICAN("american"),
    EUROPEAN("european"),
    BERMUDAN("bermudan"),
    MAKE_WHOLE("make_whole");

    private final String wire;

    CallStyle(String wire) {
        this.wire = wire;
    }

    public String wire() {
        return wire;
    }
}
