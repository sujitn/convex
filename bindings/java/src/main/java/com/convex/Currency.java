package com.convex;

/** ISO currency. Wire values match {@code convex_core::types::Currency} variant names. */
public enum Currency {
    USD, EUR, GBP, JPY, CHF, CAD, AUD;

    /** The JSON token the native layer expects (the variant name). */
    public String wire() {
        return name();
    }
}
