package com.convex;

/**
 * Raised when the native library reports a failure.
 *
 * <p>Mirrors the two Rust error paths behind {@code crates/convex-ffi}:
 * constructor failures (a {@code 0} handle plus a thread-local message) and
 * RPC error envelopes ({@code {"ok":"false","error":{code,message,field?}}}).
 * The {@link #code()} is a stable machine string ({@code invalid_input},
 * {@code invalid_handle}, {@code analytics}, ...); {@link #field()} points at
 * the offending request field when the native layer knows it.
 */
public final class ConvexException extends RuntimeException {

    private final String code;
    private final String field;

    public ConvexException(String code, String message, String field) {
        super(message);
        this.code = code;
        this.field = field;
    }

    public ConvexException(String message) {
        this("error", message, null);
    }

    /** Stable error code, never {@code null}. */
    public String code() {
        return code;
    }

    /** Offending request field, or {@code null} if unknown. */
    public String field() {
        return field;
    }

    @Override
    public String getMessage() {
        StringBuilder sb = new StringBuilder().append('[').append(code).append("] ").append(super.getMessage());
        if (field != null) {
            sb.append(" (field: ").append(field).append(')');
        }
        return sb.toString();
    }
}
