package com.convex.internal;

import com.convex.ConvexException;
import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.node.ObjectNode;

/**
 * JSON marshalling and envelope decoding for the FFI boundary.
 *
 * <p>The native RPCs always answer with an envelope:
 * {@code {"ok":"true","result":…}} or
 * {@code {"ok":"false","error":{code,message,field?}}} (note {@code ok} is the
 * string {@code "true"}/{@code "false"} — it is the serde tag, not a boolean).
 * {@link #unwrap(String)} returns the {@code result} node or throws a typed
 * {@link ConvexException}.
 */
public final class Json {

    private Json() {}

    private static final ObjectMapper MAPPER = new ObjectMapper();

    public static ObjectNode object() {
        return MAPPER.createObjectNode();
    }

    public static ObjectMapper mapper() {
        return MAPPER;
    }

    /** Parse arbitrary JSON text into a tree (used for round-tripping values). */
    public static JsonNode parse(String json) {
        try {
            return MAPPER.readTree(json);
        } catch (Exception e) {
            throw new ConvexException("parse", "invalid JSON from native layer: " + e.getMessage(), null);
        }
    }

    public static String write(JsonNode node) {
        try {
            return MAPPER.writeValueAsString(node);
        } catch (Exception e) {
            throw new ConvexException("serialize", e.getMessage(), null);
        }
    }

    /**
     * Decode an envelope, returning the {@code result} node on success and
     * throwing {@link ConvexException} on {@code "ok":"false"}.
     */
    public static JsonNode unwrap(String envelopeJson) {
        JsonNode env = parse(envelopeJson);
        JsonNode ok = env.get("ok");
        if (ok != null && "true".equals(ok.asText())) {
            JsonNode result = env.get("result");
            return result == null ? MAPPER.nullNode() : result;
        }
        JsonNode error = env.get("error");
        if (error != null) {
            String code = text(error, "code", "error");
            String message = text(error, "message", "unknown error");
            String field = error.hasNonNull("field") ? error.get("field").asText() : null;
            throw new ConvexException(code, message, field);
        }
        throw new ConvexException("malformed envelope: " + envelopeJson);
    }

    private static String text(JsonNode node, String key, String fallback) {
        JsonNode v = node.get(key);
        return v == null || v.isNull() ? fallback : v.asText();
    }

    // ---- small typed readers (null-safe) ----

    public static double dbl(JsonNode node, String field) {
        JsonNode v = node.get(field);
        return v == null || v.isNull() ? Double.NaN : v.asDouble();
    }

    public static java.util.OptionalDouble optDbl(JsonNode node, String field) {
        JsonNode v = node.get(field);
        return v == null || v.isNull() ? java.util.OptionalDouble.empty()
                : java.util.OptionalDouble.of(v.asDouble());
    }
}
