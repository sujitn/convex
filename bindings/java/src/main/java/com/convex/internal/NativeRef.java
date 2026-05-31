package com.convex.internal;

import java.lang.ref.Cleaner;

/** Shared {@link Cleaner} for native registry handles (bonds, curves). */
public final class NativeRef {

    private NativeRef() {}

    private static final Cleaner CLEANER = Cleaner.create();

    /**
     * Register {@code owner} for cleanup that releases {@code handle}. The
     * returned {@link Cleaner.Cleanable} lets the owner release eagerly from
     * {@code close()}; the cleaning action captures only the handle, never the
     * owner, so it can't keep it alive.
     */
    public static Cleaner.Cleanable releaseOnClean(Object owner, long handle) {
        return CLEANER.register(owner, () -> ConvexFfi.release(handle));
    }
}
