package com.convex;

import com.convex.internal.ConvexFfi;
import com.convex.internal.Json;
import com.convex.internal.NativeRef;

import java.lang.ref.Cleaner;

/**
 * A bond instrument living in the native registry, referenced by an opaque
 * handle.
 *
 * <p>Build one with a typed builder ({@link FixedRateBond}, {@link CallableBond},
 * …). It is {@link AutoCloseable} — use try-with-resources to release the native
 * object promptly; a {@link Cleaner} is only a GC-time backstop.
 */
public final class Bond implements AutoCloseable {

    private final long handle;
    private final Cleaner.Cleanable cleanable;

    private Bond(long handle) {
        this.handle = handle;
        this.cleanable = NativeRef.releaseOnClean(this, handle);
    }

    /** Build from a raw {@code BondSpec} JSON. Prefer the typed builders. */
    static Bond fromSpecJson(String specJson) {
        return new Bond(ConvexFfi.buildBond(specJson));
    }

    /** Package-private: the registry handle, for request marshalling. */
    long handle() {
        return handle;
    }

    /** A JSON description of the registered bond (coupon, maturity, …). */
    public com.fasterxml.jackson.databind.JsonNode describe() {
        return Json.unwrap(ConvexFfi.describe(handle));
    }

    @Override
    public void close() {
        cleanable.clean(); // idempotent; releases the native object
    }
}
