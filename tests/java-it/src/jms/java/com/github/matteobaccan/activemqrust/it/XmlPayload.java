// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

package com.github.matteobaccan.activemqrust.it;

import java.io.StringReader;
import java.nio.charset.StandardCharsets;
import java.time.Instant;
import java.time.ZoneOffset;
import java.time.format.DateTimeFormatter;
import java.util.Base64;
import java.util.SplittableRandom;
import java.util.zip.Deflater;
import javax.xml.stream.XMLInputFactory;
import javax.xml.stream.XMLStreamConstants;
import javax.xml.stream.XMLStreamReader;

/**
 * Deterministic XML benchmark documents: 20 random fields plus a base64 buffer that pads the
 * document to an exact size. The same (seed, seq, size) always produces the same text.
 */
public final class XmlPayload {

    private static final String ALNUM = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    private static final DateTimeFormatter TS =
            DateTimeFormatter.ofPattern("yyyy-MM-dd'T'HH:mm:ss.SSS'Z'").withZone(ZoneOffset.UTC);
    private static final long BASE_EPOCH_MS = 1_700_000_000_000L;
    private static final String TAIL_OPEN = "<payload encoding=\"base64\">";
    private static final String TAIL_CLOSE = "</payload>";
    private static final String END = "</message>";

    private XmlPayload() {
    }

    /** Builds the document for message {@code seq}; throws if the exact size cannot be reached. */
    public static String document(long seed, int seq, int size) {
        SplittableRandom r = new SplittableRandom(seed * 1_000_003L + seq);
        StringBuilder sb = new StringBuilder(size + 16);
        sb.append("<message><id>").append(seq).append("</id>");
        for (int i = 1; i <= 20; i++) {
            String name = (i < 10 ? "field0" : "field") + i;
            sb.append('<').append(name).append('>').append(value(i, r)).append("</").append(name).append('>');
        }
        sb.append(TAIL_OPEN);
        int fixed = sb.length() + TAIL_CLOSE.length() + END.length();
        int avail = size - fixed;
        if (avail < 0) {
            throw new IllegalArgumentException("size " + size + " is too small for the XML fields");
        }
        int b64 = avail - (avail % 4);
        byte[] raw = new byte[b64 / 4 * 3];
        for (int i = 0; i < raw.length; i++) {
            raw[i] = (byte) r.nextInt(256);
        }
        sb.append(Base64.getEncoder().encodeToString(raw));
        sb.append(TAIL_CLOSE);
        for (int i = 0; i < avail - b64; i++) {
            sb.append(' ');
        }
        sb.append(END);
        String doc = sb.toString();
        if (doc.getBytes(StandardCharsets.UTF_8).length != size) {
            throw new IllegalStateException("generated document for seq " + seq + " has length "
                    + doc.getBytes(StandardCharsets.UTF_8).length + " instead of " + size);
        }
        return doc;
    }

    private static String value(int field, SplittableRandom r) {
        if (field <= 4) {
            int len = 8 + r.nextInt(17);
            StringBuilder s = new StringBuilder(len);
            for (int i = 0; i < len; i++) {
                s.append(ALNUM.charAt(r.nextInt(ALNUM.length())));
            }
            return s.toString();
        } else if (field <= 8) {
            return Integer.toString(r.nextInt());
        } else if (field <= 12) {
            int digits = 2 + r.nextInt(5);
            StringBuilder s = new StringBuilder();
            s.append(r.nextLong(-1_000_000L, 1_000_000L)).append('.');
            for (int i = 0; i < digits; i++) {
                s.append((char) ('0' + r.nextInt(10)));
            }
            return s.toString();
        } else if (field <= 16) {
            return TS.format(Instant.ofEpochMilli(BASE_EPOCH_MS + r.nextLong(0, 400L * 24 * 3600 * 1000)));
        } else {
            return Boolean.toString(r.nextBoolean());
        }
    }

    /** True for the messages kept for verification after timing: the first, the last and every 100th. */
    public static boolean sampled(int seq, int last) {
        return seq == 1 || seq == last || seq % 100 == 0;
    }

    /**
     * Ratio between the size of the documents compressed with {@code Deflater} level 1 and their
     * original UTF-8 size (0.75 means 25% saved); 1.0 for an empty set.
     */
    public static double deflateRatio(Iterable<String> docs) {
        long original = 0;
        long compressed = 0;
        byte[] out = new byte[64 * 1024];
        Deflater d = new Deflater(1);
        try {
            for (String doc : docs) {
                byte[] in = doc.getBytes(StandardCharsets.UTF_8);
                original += in.length;
                d.reset();
                d.setInput(in);
                d.finish();
                while (!d.finished()) {
                    compressed += d.deflate(out);
                }
            }
        } finally {
            d.end();
        }
        return original == 0 ? 1.0 : (double) compressed / original;
    }

    /** Parses a document and checks its structure, field kinds and base64 payload. */
    public static void verify(String doc) throws Exception {
        XMLStreamReader x = XMLInputFactory.newFactory().createXMLStreamReader(new StringReader(doc));
        expectStart(x, "message");
        expectStart(x, "id");
        Integer.parseInt(x.getElementText());
        for (int i = 1; i <= 20; i++) {
            String name = (i < 10 ? "field0" : "field") + i;
            expectStart(x, name);
            String v = x.getElementText();
            if (i <= 4 && !v.matches("[A-Za-z0-9]{8,24}")) {
                throw new IllegalStateException(name + " is not an alphanumeric string: " + v);
            } else if (i > 4 && i <= 8) {
                Integer.parseInt(v);
            } else if (i > 8 && i <= 12 && !v.matches("-?[0-9]+\\.[0-9]{2,6}")) {
                throw new IllegalStateException(name + " is not a decimal: " + v);
            } else if (i > 12 && i <= 16) {
                Instant.parse(v);
            } else if (i > 16 && !v.equals("true") && !v.equals("false")) {
                throw new IllegalStateException(name + " is not a boolean: " + v);
            }
        }
        expectStart(x, "payload");
        if (!"base64".equals(x.getAttributeValue(null, "encoding"))) {
            throw new IllegalStateException("payload encoding is not base64");
        }
        Base64.getDecoder().decode(x.getElementText());
        // Nothing but </message> may follow; reading to the end also detects a truncated document.
        while (x.hasNext()) {
            if (x.next() == XMLStreamConstants.START_ELEMENT) {
                throw new IllegalStateException("unexpected <" + x.getLocalName() + "> after <payload>");
            }
        }
    }

    private static void expectStart(XMLStreamReader x, String name) throws Exception {
        while (x.hasNext()) {
            int ev = x.next();
            if (ev == XMLStreamConstants.START_ELEMENT) {
                if (!x.getLocalName().equals(name)) {
                    throw new IllegalStateException("expected <" + name + "> but found <" + x.getLocalName() + ">");
                }
                return;
            }
        }
        throw new IllegalStateException("missing <" + name + ">");
    }
}
