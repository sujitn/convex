package com.convex;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import org.junit.jupiter.api.Test;
import java.nio.file.Files;
import java.nio.file.Path;
import static org.junit.jupiter.api.Assertions.*;

/** Actual Java/C transport parity for complete workout diagnostics. */
class IcmaGovernmentTest {
    @Test void fullIcmaGovernmentAndErrorsSurviveJava() throws Exception {
        Path path = Path.of(System.getProperty("convex.library.icma.government.parity", "target/icma-government-parity.json"));
        assertTrue(Files.isRegularFile(path), "Generate fresh native parity with icma_government --emit-parity");
        var mapper = new ObjectMapper();
        JsonNode corpus = mapper.readTree(Files.readString(path));
        assertFalse(corpus.get("certified").asBoolean());
        for (JsonNode example : corpus.get("cases")) {
            JsonNode result = mapper.readTree(Convex.library(mapper.writeValueAsString(example.get("request"))));
            assertEquals(example.get("expected_envelope"), result, example.get("id").asText());
            assertEquals(example.get("expected_error").isNull() ? "true" : "false", result.get("ok").asText());
        }
    }
}
