// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

package com.github.matteobaccan.activemqrust.it;

import java.util.LinkedHashMap;
import java.util.Locale;
import java.util.Map;

/**
 * One {@code RESULT} line of the bench mode: {@code RESULT key=value key=value ...}.
 * Keys keep their insertion order, {@code scenario} and {@code status} come first, values never
 * contain whitespace and decimals always use a dot, whatever the default locale, so that the
 * comparison script can parse the line without knowing the client's locale.
 */
public final class BenchResult {

    public static final String PREFIX = "RESULT ";

    private final Map<String, String> fields = new LinkedHashMap<>();

    public BenchResult(String scenario) {
        fields.put("scenario", clean(scenario));
        fields.put("status", "ok");
    }

    /** Adds a text or integer value. */
    public BenchResult put(String key, Object value) {
        fields.put(key, clean(String.valueOf(value)));
        return this;
    }

    /** Adds a decimal value with the given number of fractional digits. */
    public BenchResult put(String key, double value, int decimals) {
        fields.put(key, String.format(Locale.ROOT, "%." + decimals + "f", value));
        return this;
    }

    /** Marks the run as failed; the first reason given is kept. */
    public BenchResult fail(String reason) {
        if (ok()) {
            fields.put("status", "failed");
            fields.put("reason", clean(reason == null ? "unknown" : reason));
        }
        return this;
    }

    public boolean ok() {
        return "ok".equals(fields.get("status"));
    }

    public String get(String key) {
        return fields.get(key);
    }

    /** Exit code of the bench: 0 when the run succeeded, 1 otherwise. */
    public int exitCode() {
        return ok() ? 0 : 1;
    }

    public String line() {
        StringBuilder sb = new StringBuilder(PREFIX.trim());
        String reason = fields.get("reason");
        for (Map.Entry<String, String> e : fields.entrySet()) {
            if (!e.getKey().equals("reason")) {
                sb.append(' ').append(e.getKey()).append('=').append(e.getValue());
            }
        }
        if (reason != null) {
            sb.append(" reason=").append(reason);
        }
        return sb.toString();
    }

    public void print() {
        System.out.println(line());
        System.out.flush();
    }

    /** Parses a {@code RESULT} line back into its fields; throws if the line is not one. */
    public static Map<String, String> parse(String line) {
        if (line == null || !line.startsWith(PREFIX)) {
            throw new IllegalArgumentException("not a RESULT line: " + line);
        }
        Map<String, String> m = new LinkedHashMap<>();
        for (String kv : line.substring(PREFIX.length()).trim().split(" +")) {
            int eq = kv.indexOf('=');
            if (eq <= 0) {
                throw new IllegalArgumentException("malformed field '" + kv + "' in: " + line);
            }
            m.put(kv.substring(0, eq), kv.substring(eq + 1));
        }
        return m;
    }

    /** Messages per second; 0 when no time has elapsed. */
    public static double perSecond(long count, long ms) {
        return ms <= 0 ? 0 : count * 1000.0 / ms;
    }

    /** MB per second, with 1 MB = 1,048,576 bytes of payload; 0 when no time has elapsed. */
    public static double mbPerSecond(long count, int size, long ms) {
        return ms <= 0 ? 0 : (double) count * size / (1024.0 * 1024.0) * 1000.0 / ms;
    }

    private static String clean(String s) {
        String v = s.trim().replaceAll("\\s+", "_");
        return v.isEmpty() ? "-" : v;
    }
}
