package com.convex;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import org.junit.jupiter.api.Test;

import java.io.BufferedReader;
import java.io.BufferedWriter;
import java.io.InputStreamReader;
import java.io.OutputStreamWriter;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.HashSet;
import java.util.zip.GZIPInputStream;
import java.util.zip.GZIPOutputStream;

import static org.junit.jupiter.api.Assertions.*;

/** Complete actual FFM transport census; original financial controls stay separate. */
class LibraryCensusTest {
    @Test void everyCurrentSharedRequestPreservesCompleteNativeEnvelope() throws Exception {
        Path index = Path.of(System.getProperty("convex.library.census", "target/library-host-census/index.json"));
        assertTrue(Files.isRegularFile(index), "Generate the complete fresh native host census first");
        ObjectMapper mapper = new ObjectMapper();
        JsonNode manifest = mapper.readTree(Files.readString(index));
        assertFalse(manifest.path("certified").asBoolean(true));
        assertTrue(manifest.path("complete_requests").asInt() > 0);
        Path outputs = Path.of(System.getProperty("convex.library.census.outputs", "target/library-host-actual"));
        assertFalse(Files.exists(outputs), "Refusing to overwrite an actual host capture");
        Files.createDirectories(outputs);
        HashSet<String> identities = new HashSet<>();
        int successes = 0;
        for (JsonNode corpus : manifest.path("corpora")) {
            String filename = corpus.path("file").asText();
            assertTrue(filename.matches("[0-9]{3}\\.jsonl\\.gz"));
            int count = 0;
            int positive = 0;
            try (BufferedReader input = new BufferedReader(new InputStreamReader(
                    new GZIPInputStream(Files.newInputStream(index.toAbsolutePath().getParent().resolve(filename))), StandardCharsets.UTF_8));
                 BufferedWriter output = new BufferedWriter(new OutputStreamWriter(
                    new GZIPOutputStream(Files.newOutputStream(outputs.resolve(filename))), StandardCharsets.UTF_8))) {
                String line;
                while ((line = input.readLine()) != null) {
                    JsonNode row = mapper.readTree(line);
                    String identity = row.path("id").asText();
                    assertTrue(identities.add(identity), "Missing or duplicated request identity");
                    // Keep exact native wire bytes: parser diagnostics include
                    // source columns, which change when a host rewrites JSON.
                    String actual = Convex.library(row.path("request_json").asText());
                    JsonNode envelope = mapper.readTree(actual);
                    // Preserve complete actual output even when strict comparison fails.
                    var capture = mapper.createObjectNode();
                    capture.put("id", identity);
                    capture.put("actual_envelope_json", actual);
                    output.write(capture.toString());
                    output.newLine();
                    assertEquals(row.path("expected_envelope"), envelope, identity);
                    boolean success = row.path("expected_error").isNull();
                    assertEquals(success ? "true" : "false", envelope.path("ok").asText(), identity);
                    if (success) positive++;
                    else assertEquals(row.path("expected_error"), envelope.path("error").path("code"), identity);
                    count++;
                }
            }
            assertEquals(corpus.path("cases").asInt(), count);
            assertEquals(corpus.path("successes").asInt(), positive);
            successes += positive;
        }
        assertEquals(manifest.path("complete_requests").asInt(), identities.size());
        var summary = mapper.createObjectNode();
        summary.put("status", "passed");
        summary.put("actual_java_ffm", true);
        summary.put("complete_requests", identities.size());
        summary.put("successes", successes);
        summary.put("typed_failures", identities.size() - successes);
        summary.put("exact_complete_native_envelopes", true);
        summary.put("certified", false);
        Files.writeString(outputs.resolve("summary.json"), mapper.writerWithDefaultPrettyPrinter().writeValueAsString(summary) + "\n");
    }
}
