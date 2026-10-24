package com.convex;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import org.junit.jupiter.api.Test;
import java.nio.file.Files;
import java.nio.file.Path;
import static org.junit.jupiter.api.Assertions.*;

/** Actual Panama/C ABI parity; freshly generated native output is not a financial oracle. */
class LibraryTest {
    @Test void completeNativeRequestsResultsAndErrorsSurviveJava() throws Exception {
        String path = System.getProperty("convex.library.parity", "target/library-parity.json");
        assertTrue(Files.isRegularFile(Path.of(path)), "Generate fresh native parity with library_workflow --emit-parity");
        var mapper = new ObjectMapper();
        JsonNode corpus = mapper.readTree(Files.readString(Path.of(path)));
        for (JsonNode example : corpus.get("cases")) {
            String input = mapper.writeValueAsString(example.get("request"));
            JsonNode result = mapper.readTree(Convex.library(input));
            assertEquals(example.get("expected_envelope"), result, example.get("id").asText());
            assertEquals(example.get("expected_error").isNull() ? "true" : "false", result.get("ok").asText());
        }
        JsonNode invalid = mapper.readTree(Convex.library("{\"schema_version\":1,\"schema_version\":1}"));
        assertEquals("false", invalid.get("ok").asText());
        assertEquals("invalid_input", invalid.get("error").get("code").asText());
        assertThrows(IllegalArgumentException.class, () -> Convex.library("{}\0discarded"));
        assertThrows(IllegalArgumentException.class, () -> Convex.library("\ud800"));
        assertThrows(IllegalArgumentException.class, () -> Convex.library(null));
    }
}
