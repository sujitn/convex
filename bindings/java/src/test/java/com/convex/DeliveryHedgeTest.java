package com.convex;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import org.junit.jupiter.api.Test;
import java.nio.file.Files;
import java.nio.file.Path;
import static org.junit.jupiter.api.Assertions.*;

/** Actual Java/C transport of complete all-member delivery risk and hedge book. */
class DeliveryHedgeTest {
    @Test void completeDeliveryBasketAndHedgesSurviveJava() throws Exception {
        Path path = Path.of(System.getProperty("convex.library.delivery.hedge.parity", "target/delivery-hedge-parity.json"));
        assertTrue(Files.isRegularFile(path), "Generate fresh native parity with delivery_hedge --emit-parity");
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
