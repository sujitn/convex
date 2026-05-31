package com.convex;

import com.convex.internal.ConvexFfi;
import com.convex.internal.Json;

import java.math.BigDecimal;

/**
 * A trader mark — a price, yield, or spread quote.
 *
 * <p>Marks cross the FFI as the textual shorthand the Rust {@code Mark} parser
 * accepts (e.g. {@code "99.5C"}, {@code "4.65%"}, {@code "+125bps@USD.SOFR"}),
 * carried as a {@code MarkInput::Text}. Use the factories for the common cases
 * or {@link #parse(String)} to validate free-form text against the native
 * parser up front.
 */
public final class Mark {

    private final String text;

    private Mark(String text) {
        this.text = text;
    }

    /** Clean price per 100 (e.g. 99.5 → {@code "99.5C"}). */
    public static Mark cleanPrice(BigDecimal pricePer100) {
        return new Mark(pricePer100.toPlainString() + "C");
    }

    /** Dirty price per 100 (e.g. 100.75 → {@code "100.75D"}). */
    public static Mark dirtyPrice(BigDecimal pricePer100) {
        return new Mark(pricePer100.toPlainString() + "D");
    }

    /** Yield in percent (e.g. 4.65 → {@code "4.65%"}). */
    public static Mark yieldPercent(BigDecimal percent) {
        return new Mark(percent.toPlainString() + "%");
    }

    /** Any textual mark, taken verbatim. */
    public static Mark of(String text) {
        return new Mark(text);
    }

    /**
     * Validate {@code text} against the native mark parser, throwing
     * {@link ConvexException} if it is not a recognised mark.
     */
    public static Mark parse(String text) {
        Json.unwrap(ConvexFfi.markParse(text)); // throws on invalid_input
        return new Mark(text);
    }

    /** The wire text sent as {@code MarkInput::Text}. */
    public String wire() {
        return text;
    }

    @Override
    public String toString() {
        return text;
    }
}
