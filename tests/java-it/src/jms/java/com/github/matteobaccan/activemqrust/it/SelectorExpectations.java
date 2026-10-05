// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

package com.github.matteobaccan.activemqrust.it;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * Expected results of the selector scenarios: for each selector, the messages it selects, in order.
 * The values were computed with ActiveMQ's own selector engine
 * ({@code org.apache.activemq.selector.SelectorParser}, activemq-client 5.18.x and 6.x give the same results)
 * on the messages the scenarios send.
 */
final class SelectorExpectations {

    private SelectorExpectations() {
    }

    /** The 12 messages of {@code Integration.selectors()}. */
    static final Map<String, List<String>> INTEGRATION = table(new String[][] {
        {"color = 'red'", "m0 m4 m8"},
        {"color <> 'red'", "m1 m2 m5 m6 m9 m10"},
        {"NOT (color = 'red')", "m1 m2 m5 m6 m9 m10"},
        {"size > 2", "m3 m4 m5 m9 m10 m11"},
        {"size BETWEEN 2 AND 4", "m2 m3 m4 m8 m9 m10"},
        {"size NOT BETWEEN 2 AND 4", "m0 m1 m5 m6 m7 m11"},
        {"color IN ('red','blue')", "m0 m1 m4 m5 m8 m9"},
        {"color NOT IN ('red','blue')", "m2 m6 m10"},
        {"name LIKE 'a%'", "m0 m3 m4 m5 m8 m9 m10"},
        {"name LIKE '_b%'", "m0 m1 m3 m5 m6 m8 m10 m11"},
        {"name NOT LIKE '%c'", "m2 m3 m4 m7 m8 m9"},
        {"missing IS NULL", "m0 m1 m2 m3 m4 m5 m6 m7 m8 m9 m10 m11"},
        {"missing IS NOT NULL", ""},
        {"missing = 'x'", ""},
        {"NOT (missing = 'x')", ""},
        {"size = '3'", ""},
        {"NOT (size = '3')", "m0 m1 m2 m3 m4 m5 m6 m7 m8 m9 m10 m11"},
        {"flag = TRUE", "m0 m3 m6 m9"},
        {"flag", "m0 m3 m6 m9"},
        {"size * 2 > 5", "m3 m4 m5 m9 m10 m11"},
        {"weight > 1.5", "m4 m5 m6 m7 m8 m9 m10 m11"},
        {"JMSPriority > 4", "m5 m6 m7 m8 m9"},
        {"JMSCorrelationID = 'c2'", "m2 m6 m10"},
        {"JMSType = 'kind1'", "m1 m3 m5 m7 m9 m11"},
        {"JMSDeliveryMode = 'PERSISTENT'", "m0 m2 m4 m6 m8 m10"},
        {"color = 'red' AND size > 1", "m4 m8"},
        {"color = 'red' OR size > 4", "m0 m4 m5 m8 m11"},
        {"(color = 'red' OR color = 'green') AND NOT flag", "m2 m4 m8 m10"},
        {"JMSXDeliveryCount = 1", "m0 m1 m2 m3 m4 m5 m6 m7 m8 m9 m10 m11"},
        {"size + 1 = 4", "m3 m9"},
    });

    /** The 12 messages of {@code SelectorParity}: type rules, precedence and arithmetic. */
    static final Map<String, List<String>> PARITY = table(new String[][] {
        {"NOT size = 3", ""},
        {"size = 1 and not color is null", "m7"},
        {"size = 1 and not missing is null", "m1 m7"},
        {"NOT flag", "m1 m2 m4 m5 m7 m8 m10 m11"},
        {"NOT size", "m0 m1 m2 m3 m4 m5 m6 m7 m8 m9 m10 m11"},
        {"NOT missing", ""},
        {"b < s", "m1 m2 m3 m4 m5 m6 m7 m8 m9 m10 m11"},
        {"s > b", ""},
        {"300 = s", ""},
        {"s = 300", "m3"},
        {"f = 0.5", "m2"},
        {"f > 1", "m5 m6 m7 m8 m9 m10 m11"},
        {"1 < f", "m5 m6 m7 m8 m9 m10 m11"},
        {"l > 30000000000", "m4 m5 m6 m7 m8 m9 m10 m11"},
        {"l = 10000000000", "m1"},
        {"l + 1 > 10000000000", "m1 m2 m3 m4 m5 m6 m7 m8 m9 m10 m11"},
        {"text = 1", ""},
        {"NOT (text = 1)", "m0 m1 m2 m3 m4 m5 m6 m7 m8 m9 m10 m11"},
        {"text = '1'", "m1 m4 m7 m10"},
        {"color + 'x' = 'redx'", "m0 m4 m8"},
        {"color + missing IS NULL", "m0 m1 m2 m3 m4 m5 m6 m7 m8 m9 m10 m11"},
        {"color + size = 'red0'", "m0"},
        {"size / 0 > 100", "m0 m1 m2 m3 m4 m5 m6 m7 m8 m9 m10 m11"},
        {"size % 2 = 1", "m1 m3 m5 m7 m9 m11"},
        {"7 / 2 = 3.5", "m0 m1 m2 m3 m4 m5 m6 m7 m8 m9 m10 m11"},
        {"size / 2 = 1.5", "m3 m9"},
        {"size BETWEEN missing AND 3", ""},
        {"size NOT BETWEEN missing AND 3", "m4 m5 m10 m11"},
        {"JMSPriority BETWEEN 3 AND 5", "m3 m4 m5"},
        {"JMSDeliveryMode <> 'PERSISTENT'", "m1 m3 m5 m7 m9 m11"},
        {"color > name", "m0 m1 m4 m5 m6 m8 m9 m10"},
        {"flag = (size = 0)", "m0 m1 m2 m4 m5 m6 m7 m8 m10 m11"},
        {"missing = other", "m0 m1 m2 m3 m4 m5 m6 m7 m8 m9 m10 m11"},
        {"color = missing", "m3 m7 m11"},
        {"JMSXGroupSeq = 0", "m0 m1 m2 m3 m4 m5 m6 m7 m8 m9 m10 m11"},
        {"JMSRedelivered = FALSE", "m0 m1 m2 m3 m4 m5 m6 m7 m8 m9 m10 m11"},
        {"JMSDestination LIKE 'queue://IT.SELP.%'", "m0 m1 m2 m3 m4 m5 m6 m7 m8 m9 m10 m11"},
        {"name LIKE '%b%'", "m0 m1 m3 m5 m6 m8 m10 m11"},
        {"name LIKE 'a_c'", "m0 m5 m10"},
        {"name IN ('ab','a','zz','y1','y2','y3','y4','y5','y6','y7')", "m3 m4 m8 m9"},
        {"-size < -3", "m4 m5 m10 m11"},
        {"size = 0x3", "m3 m9"},
        {"weight = 25e-1", "m5"},
    });

    private static Map<String, List<String>> table(String[][] rows) {
        Map<String, List<String>> m = new LinkedHashMap<>();
        for (String[] r : rows) {
            m.put(r[0], r[1].isEmpty() ? Collections.<String>emptyList() : Arrays.asList(r[1].split(" ")));
        }
        return Collections.unmodifiableMap(m);
    }

    /** Fails with every selector whose received messages differ from {@code expected}. */
    static void verify(Map<String, List<String>> expected, Map<String, List<String>> results) {
        List<String> diffs = new ArrayList<>();
        for (Map.Entry<String, List<String>> e : expected.entrySet()) {
            List<String> got = results.get(e.getKey());
            if (!e.getValue().equals(got)) {
                diffs.add(e.getKey() + ": expected " + e.getValue() + ", got " + got);
            }
        }
        for (String sel : results.keySet()) {
            if (!expected.containsKey(sel)) {
                diffs.add(sel + ": no expected result");
            }
        }
        if (!diffs.isEmpty()) {
            throw new AssertionError(diffs.size() + " selector(s) differ from ActiveMQ: " + String.join("; ", diffs));
        }
    }

    static void verify(Map<String, List<String>> results) {
        verify(INTEGRATION, results);
    }
}
