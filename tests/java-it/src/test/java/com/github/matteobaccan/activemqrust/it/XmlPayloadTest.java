// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

package com.github.matteobaccan.activemqrust.it;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.charset.StandardCharsets;
import java.util.List;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.params.ParameterizedTest;
import org.junit.jupiter.params.provider.ValueSource;

class XmlPayloadTest {

    private static final Pattern TRAILER = Pattern.compile("</payload>( *)</message>$");

    @ParameterizedTest
    @ValueSource(ints = {1024, 10240, 12288, 51200})
    void everyDocumentHasTheExactSizeAndIsWellFormed(int size) throws Exception {
        for (int seq = 1; seq <= 10_000; seq++) {
            String doc = XmlPayload.document(Bench.SEED_RUN + size, seq, size);
            assertEquals(size, doc.getBytes(StandardCharsets.UTF_8).length, "seq " + seq);
            assertEquals(size, doc.length(), "ASCII only, seq " + seq);
            XmlPayload.verify(doc);
            assertTrue(doc.startsWith("<message><id>" + seq + "</id><field01>"), "seq " + seq);
            Matcher m = TRAILER.matcher(doc);
            assertTrue(m.find(), "seq " + seq);
            assertTrue(m.group(1).length() <= 3, "at most 3 padding spaces, seq " + seq);
        }
    }

    @Test
    void sameSeedGivesIdenticalSets() {
        assertArrayEquals(Bench.generate(42, 2_000, 10240), Bench.generate(42, 2_000, 10240));
    }

    @Test
    void differentSeedsAndSequencesDiffer() {
        assertNotEquals(XmlPayload.document(1, 1, 1024), XmlPayload.document(2, 1, 1024));
        assertNotEquals(XmlPayload.document(1, 1, 1024), XmlPayload.document(1, 2, 1024));
        assertNotEquals(Bench.generate(Bench.SEED_RUN, 1, 1024)[0], Bench.generate(Bench.SEED_WARMUP, 1, 1024)[0]);
    }

    @Test
    void verifyRejectsBrokenDocuments() {
        String doc = XmlPayload.document(1, 1, 1024);
        assertThrows(Exception.class, () -> XmlPayload.verify(doc.replace("<field07>", "<fieldX>")));
        assertThrows(Exception.class,
                () -> XmlPayload.verify(doc.replaceFirst("<field17>(true|false)", "<field17>maybe")));
        assertThrows(Exception.class, () -> XmlPayload.verify(doc.replace("</message>", "")));
        assertThrows(Exception.class,
                () -> XmlPayload.verify(doc.replace("encoding=\"base64\">", "encoding=\"base64\">!")));
    }

    @Test
    void tooSmallSizeIsRejected() {
        assertThrows(IllegalArgumentException.class, () -> XmlPayload.document(1, 1, 200));
    }

    @Test
    void sampleIsFirstLastAndEveryHundredth() {
        assertTrue(XmlPayload.sampled(1, 3600));
        assertTrue(XmlPayload.sampled(100, 3600));
        assertTrue(XmlPayload.sampled(3600, 3600));
        assertTrue(XmlPayload.sampled(3599, 3599));
        assertFalse(XmlPayload.sampled(2, 3600));
        assertFalse(XmlPayload.sampled(3599, 3600));
    }

    @Test
    void deflateRatioOfBase64DocumentsIsBelowOne() {
        List<String> docs = List.of(Bench.generate(Bench.SEED_RUN, 20, 51200));
        double r = XmlPayload.deflateRatio(docs);
        assertTrue(r > 0.6 && r < 0.9, "ratio " + r);
        assertEquals(1.0, XmlPayload.deflateRatio(List.of()));
        assertTrue(XmlPayload.deflateRatio(List.of(" ".repeat(10_000))) < 0.05);
    }
}
