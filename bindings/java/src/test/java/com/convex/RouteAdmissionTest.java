package com.convex;

import com.convex.internal.ConvexFfi;
import com.convex.internal.Json;
import org.junit.jupiter.api.Test;
import java.util.Arrays;
import static org.junit.jupiter.api.Assertions.*;

/** Removed government zero-curve inputs stay absent across Java and actual C FFM. */
class RouteAdmissionTest {
    @Test void spreadBuilderHasNoGovernmentCurveProxy() {
        assertFalse(Arrays.stream(ConvexAnalytics.SpreadParams.class.getMethods())
                .anyMatch(method -> method.getName().equals("govtCurve")));
        var params = new ConvexAnalytics.SpreadParams().volatility(0.01).currentIndex(0.035);
        assertFalse(params.toJson().has("govt_curve"));
        assertEquals(0.01, params.toJson().get("volatility").asDouble());
    }

    @Test void removedSpreadFieldFailsThroughActualNativeFfm() {
        String request = """
                {"bond":123,"curve":456,"settlement":"2025-04-15","mark":"99.5C",
                 "spread_type":"GSpread","params":{"govt_curve":null}}
                """;
        var error = assertThrows(ConvexException.class,
                () -> Json.unwrap(ConvexFfi.spread(request)));
        assertEquals("invalid_input", error.code());
        assertTrue(error.getMessage().contains("unknown field `govt_curve`"));
    }
}
