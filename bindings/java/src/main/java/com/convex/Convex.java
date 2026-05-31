package com.convex;

import com.convex.internal.ConvexFfi;

/**
 * Library entry point: version and registry diagnostics.
 *
 * <p>Touching any method here (or any builder) triggers one-time native library
 * loading. Bonds and curves are reference-counted native objects — prefer
 * try-with-resources; {@link #objectCount()} is handy in tests to assert none
 * leaked.
 */
public final class Convex {

    private Convex() {}

    /** The native library version (matches the Rust workspace version). */
    public static String version() {
        return ConvexFfi.version();
    }

    /** Number of live native objects (bonds + curves) currently registered. */
    public static int objectCount() {
        return ConvexFfi.objectCount();
    }

    /** Release every registered native object. Mainly for test teardown. */
    public static void clearAll() {
        ConvexFfi.clearAll();
    }
}
