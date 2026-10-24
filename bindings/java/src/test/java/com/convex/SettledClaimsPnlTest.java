package com.convex;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import org.junit.jupiter.api.Test;
import java.nio.file.Files;
import java.nio.file.Path;
import static org.junit.jupiter.api.Assertions.*;

/** Actual Java/C transport of complete original successive custody, actual disposals and retained invoice recipients. */
class SettledClaimsPnlTest {
    @Test void settledClaimsAndOriginalClosureSurviveJava() throws Exception {
        Path path = Path.of(System.getProperty("convex.library.settled.claims.pnl.parity", "target/settled-claims-pnl-parity.json"));
        assertTrue(Files.isRegularFile(path), "Generate fresh native parity with settled_claims_pnl --emit-parity");
        var mapper = new ObjectMapper();
        JsonNode corpus = mapper.readTree(Files.readString(path));
        assertFalse(corpus.get("certified").asBoolean());
        for (JsonNode example : corpus.get("cases")) {
            String request = example.get("request_json").asText();
            assertEquals(example.get("request"), mapper.readTree(request));
            JsonNode result = mapper.readTree(Convex.library(request));
            assertEquals(example.get("expected_envelope"), result, example.get("id").asText());
            assertEquals(example.get("expected_error").isNull() ? "true" : "false", result.get("ok").asText());
        }
    }
}
