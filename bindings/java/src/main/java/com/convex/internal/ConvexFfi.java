package com.convex.internal;

import com.convex.ConvexException;

import java.lang.foreign.Arena;
import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.Linker;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.SymbolLookup;
import java.lang.foreign.ValueLayout;
import java.lang.invoke.MethodHandle;

/**
 * Panama FFM binding over the {@code convex-ffi} C ABI: one {@link MethodHandle}
 * per symbol, all UTF-8 {@code char*} in / {@code char*} out. Returned strings
 * are Rust-owned and freed here via {@code convex_string_free}. Safe to call
 * from any thread.
 */
public final class ConvexFfi {

    private ConvexFfi() {}

    private static final Linker LINKER = Linker.nativeLinker();
    private static final SymbolLookup LOOKUP;

    // char* fn(const char*)
    private static final MethodHandle PRICE;
    private static final MethodHandle RISK;
    private static final MethodHandle SPREAD;
    private static final MethodHandle CASHFLOWS;
    private static final MethodHandle CURVE_QUERY;
    private static final MethodHandle MAKE_WHOLE;
    private static final MethodHandle RISK_PROFILE;
    private static final MethodHandle HEDGE;
    private static final MethodHandle COMPARE;
    private static final MethodHandle MARK_PARSE;
    private static final MethodHandle DESCRIBE;     // char* fn(u64)

    // u64 fn(const char*)
    private static final MethodHandle BOND_FROM_JSON;
    private static final MethodHandle CURVE_FROM_JSON;

    private static final MethodHandle RELEASE;       // void fn(u64)
    private static final MethodHandle STRING_FREE;    // void fn(char*)
    private static final MethodHandle LAST_ERROR;     // const char* fn()
    private static final MethodHandle VERSION;        // const char* fn()
    private static final MethodHandle OBJECT_COUNT;   // i32 fn()
    private static final MethodHandle CLEAR_ALL;      // void fn()

    static {
        NativeLoader.ensureLoaded();
        LOOKUP = SymbolLookup.loaderLookup();

        FunctionDescriptor strToStr = FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.ADDRESS);
        FunctionDescriptor strToHandle = FunctionDescriptor.of(ValueLayout.JAVA_LONG, ValueLayout.ADDRESS);
        FunctionDescriptor handleToStr = FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.JAVA_LONG);
        FunctionDescriptor voidToStr = FunctionDescriptor.of(ValueLayout.ADDRESS);
        FunctionDescriptor voidToInt = FunctionDescriptor.of(ValueLayout.JAVA_INT);
        FunctionDescriptor handleToVoid = FunctionDescriptor.ofVoid(ValueLayout.JAVA_LONG);
        FunctionDescriptor ptrToVoid = FunctionDescriptor.ofVoid(ValueLayout.ADDRESS);
        FunctionDescriptor voidToVoid = FunctionDescriptor.ofVoid();

        PRICE = handle("convex_price", strToStr);
        RISK = handle("convex_risk", strToStr);
        SPREAD = handle("convex_spread", strToStr);
        CASHFLOWS = handle("convex_cashflows", strToStr);
        CURVE_QUERY = handle("convex_curve_query", strToStr);
        MAKE_WHOLE = handle("convex_make_whole", strToStr);
        RISK_PROFILE = handle("convex_risk_profile", strToStr);
        HEDGE = handle("convex_hedge", strToStr);
        COMPARE = handle("convex_compare", strToStr);
        MARK_PARSE = handle("convex_mark_parse", strToStr);
        DESCRIBE = handle("convex_describe", handleToStr);

        BOND_FROM_JSON = handle("convex_bond_from_json", strToHandle);
        CURVE_FROM_JSON = handle("convex_curve_from_json", strToHandle);

        RELEASE = handle("convex_release", handleToVoid);
        STRING_FREE = handle("convex_string_free", ptrToVoid);
        LAST_ERROR = handle("convex_last_error", voidToStr);
        VERSION = handle("convex_version", voidToStr);
        OBJECT_COUNT = handle("convex_object_count", voidToInt);
        CLEAR_ALL = handle("convex_clear_all", voidToVoid);
    }

    private static MethodHandle handle(String symbol, FunctionDescriptor descriptor) {
        MemorySegment addr = LOOKUP.find(symbol)
                .orElseThrow(() -> new ConvexException("missing native symbol: " + symbol));
        return LINKER.downcallHandle(addr, descriptor);
    }

    // ---- public surface ----------------------------------------------------

    /** Force class loading / native init eagerly (used by tests). */
    public static String version() {
        return readStaticCString(invokePtr(VERSION));
    }

    public static int objectCount() {
        try {
            return (int) OBJECT_COUNT.invokeExact();
        } catch (Throwable t) {
            throw rethrow(t);
        }
    }

    public static void clearAll() {
        try {
            CLEAR_ALL.invokeExact();
        } catch (Throwable t) {
            throw rethrow(t);
        }
    }

    public static void release(long handle) {
        try {
            RELEASE.invokeExact(handle);
        } catch (Throwable t) {
            throw rethrow(t);
        }
    }

    /** Build a bond from a {@code BondSpec} JSON; throws on a {@code 0} handle. */
    public static long buildBond(String specJson) {
        return build(BOND_FROM_JSON, specJson, "bond");
    }

    /** Build a curve from a {@code CurveSpec} JSON; throws on a {@code 0} handle. */
    public static long buildCurve(String specJson) {
        return build(CURVE_FROM_JSON, specJson, "curve");
    }

    public static String price(String requestJson)        { return rpc(PRICE, requestJson); }
    public static String risk(String requestJson)         { return rpc(RISK, requestJson); }
    public static String spread(String requestJson)       { return rpc(SPREAD, requestJson); }
    public static String cashflows(String requestJson)    { return rpc(CASHFLOWS, requestJson); }
    public static String curveQuery(String requestJson)   { return rpc(CURVE_QUERY, requestJson); }
    public static String makeWhole(String requestJson)    { return rpc(MAKE_WHOLE, requestJson); }
    public static String riskProfile(String requestJson)  { return rpc(RISK_PROFILE, requestJson); }
    public static String hedge(String requestJson)        { return rpc(HEDGE, requestJson); }
    public static String compare(String requestJson)      { return rpc(COMPARE, requestJson); }
    public static String markParse(String text)           { return rpc(MARK_PARSE, text); }

    public static String describe(long handle) {
        try {
            MemorySegment ptr = (MemorySegment) DESCRIBE.invokeExact(handle);
            return ownedToString(ptr);
        } catch (Throwable t) {
            throw rethrow(t);
        }
    }

    // ---- internals ----------------------------------------------------------

    private static long build(MethodHandle ctor, String specJson, String kind) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment in = arena.allocateFrom(specJson);
            long handle = (long) ctor.invokeExact(in);
            if (handle == 0L) {
                throw new ConvexException("invalid_input", "failed to build " + kind + ": " + lastError(), null);
            }
            return handle;
        } catch (ConvexException e) {
            throw e;
        } catch (Throwable t) {
            throw rethrow(t);
        }
    }

    /**
     * Invoke a {@code char* fn(const char*)} RPC: marshal the request, read the
     * returned owned C string, free it, and return the JSON text. Envelope
     * decoding (and error throwing) is the caller's job (see {@code Json}).
     */
    private static String rpc(MethodHandle fn, String requestJson) {
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment in = arena.allocateFrom(requestJson);
            MemorySegment out = (MemorySegment) fn.invokeExact(in);
            return ownedToString(out);
        } catch (Throwable t) {
            throw rethrow(t);
        }
    }

    /** Read a Rust-owned C string and free it. */
    private static String ownedToString(MemorySegment ptr) {
        if (ptr.address() == 0L) {
            throw new ConvexException("native function returned NULL");
        }
        try {
            // Returned pointers are zero-length; widen so the string is readable.
            String s = ptr.reinterpret(Long.MAX_VALUE).getString(0);
            STRING_FREE.invokeExact(ptr);
            return s;
        } catch (Throwable t) {
            throw rethrow(t);
        }
    }

    /** Read a static (borrowed, never-freed) C string such as version/last_error. */
    private static String readStaticCString(MemorySegment ptr) {
        if (ptr.address() == 0L) {
            return "";
        }
        return ptr.reinterpret(Long.MAX_VALUE).getString(0);
    }

    private static MemorySegment invokePtr(MethodHandle fn) {
        try {
            return (MemorySegment) fn.invokeExact();
        } catch (Throwable t) {
            throw rethrow(t);
        }
    }

    private static String lastError() {
        return readStaticCString(invokePtr(LAST_ERROR));
    }

    private static RuntimeException rethrow(Throwable t) {
        if (t instanceof ConvexException ce) {
            return ce;
        }
        return new ConvexException("error", "native call failed: " + t, null);
    }
}
