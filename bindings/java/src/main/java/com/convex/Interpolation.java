package com.convex;

/** Curve interpolation. Wire values match {@code convex_analytics::dto::InterpolationMethodCode} (snake_case). */
public enum Interpolation {
    LINEAR("linear"),
    LOG_LINEAR("log_linear"),
    CUBIC_SPLINE("cubic_spline"),
    MONOTONE_CONVEX("monotone_convex");

    private final String wire;

    Interpolation(String wire) {
        this.wire = wire;
    }

    public String wire() {
        return wire;
    }
}
