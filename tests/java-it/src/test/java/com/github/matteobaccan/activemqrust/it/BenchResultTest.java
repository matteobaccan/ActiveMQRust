// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

package com.github.matteobaccan.activemqrust.it;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.util.List;
import java.util.Locale;
import java.util.Map;
import org.junit.jupiter.api.Test;

class BenchResultTest {

    @Test
    void lineRoundTripsInOrder() {
        String line = new BenchResult("hold").put("messages", 100000).put("size", 10240)
                .put("produce_ms", 9100L).put("produce_msgs_s", 10989.0109, 1).line();
        assertEquals("RESULT scenario=hold status=ok messages=100000 size=10240 produce_ms=9100 produce_msgs_s=10989.0",
                line);
        Map<String, String> m = BenchResult.parse(line);
        assertEquals(List.of("scenario", "status", "messages", "size", "produce_ms", "produce_msgs_s"),
                List.copyOf(m.keySet()));
        assertEquals("10989.0", m.get("produce_msgs_s"));
    }

    @Test
    void decimalsUseADotInEveryLocale() {
        Locale saved = Locale.getDefault();
        try {
            Locale.setDefault(Locale.ITALY);
            String line = new BenchResult("throughput").put("mb_s", 1234.5678, 2).line();
            assertEquals("1234.57", BenchResult.parse(line).get("mb_s"));
        } finally {
            Locale.setDefault(saved);
        }
    }

    @Test
    void failureKeepsTheFirstReasonAndPutsItLast() {
        BenchResult r = new BenchResult("latency").put("rate", 1000);
        assertTrue(r.ok());
        assertEquals(0, r.exitCode());
        r.fail("timeout after 30 s").fail("second reason").put("p99_us", 812);
        assertFalse(r.ok());
        assertEquals(1, r.exitCode());
        Map<String, String> m = BenchResult.parse(r.line());
        assertEquals("failed", m.get("status"));
        assertEquals("timeout_after_30_s", m.get("reason"));
        assertTrue(r.line().endsWith(" reason=timeout_after_30_s"));
        assertEquals("812", m.get("p99_us"));
    }

    @Test
    void valuesNeverContainSpaces() {
        String line = new BenchResult("x").put("detail", "java.io.IOException: a b\tc").line();
        assertEquals("java.io.IOException:_a_b_c", BenchResult.parse(line).get("detail"));
        assertEquals("-", BenchResult.parse(new BenchResult("x").put("empty", " ").line()).get("empty"));
    }

    @Test
    void parseRejectsOtherLines() {
        assertThrows(IllegalArgumentException.class, () -> BenchResult.parse("PHASE produce-start 1"));
        assertThrows(IllegalArgumentException.class, () -> BenchResult.parse("RESULT novalue"));
    }

    @Test
    void ratesUseBinaryMegabytes() {
        assertEquals(1000.0, BenchResult.perSecond(1000, 1000));
        assertEquals(0.0, BenchResult.perSecond(1000, 0));
        assertEquals(10.0, BenchResult.mbPerSecond(1024, 10240, 1000), 1e-9);
        assertEquals(0.0, BenchResult.mbPerSecond(1, 1, 0));
    }
}
