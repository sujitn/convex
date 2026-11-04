package com.convex;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import org.junit.jupiter.api.Test;
import java.nio.file.Files;
import java.nio.file.Path;
import static org.junit.jupiter.api.Assertions.*;

/** Actual Java/C transport parity for full supplied Treasury bill quotations. */
class TreasuryBillTest {
    @Test void fullTreasuryQuotesAndErrorsSurviveJava() throws Exception {
        Path path = Path.of(System.getProperty("convex.treasury.parity", "target/treasury-bill-parity.json"));
        assertTrue(Files.isRegularFile(path), "Generate fresh native parity with treasury_bill --emit-parity");
        var mapper = new ObjectMapper();
        JsonNode corpus = mapper.readTree(Files.readString(path));
        assertFalse(corpus.get("certified").asBoolean());
        Path actual = Path.of("target/treasury-bill-actual.jsonl");
        assertFalse(Files.exists(actual), "Use a fresh actual host capture directory");
        try (var writer = Files.newBufferedWriter(actual)) {
            for (JsonNode example : corpus.get("cases")) {
                JsonNode result = mapper.readTree(Convex.library(mapper.writeValueAsString(example.get("request"))));
                var row = mapper.createObjectNode();
                row.set("id", example.get("id"));
                row.set("request", example.get("request"));
                row.set("actual_envelope", result);
                writer.write(mapper.writeValueAsString(row));
                writer.newLine();
                writer.flush();
                assertEquals(example.get("expected_envelope"), result, example.get("id").asText());
                assertEquals(example.get("expected_error").isNull() ? "true" : "false", result.get("ok").asText());
            }
        }
    }
}
