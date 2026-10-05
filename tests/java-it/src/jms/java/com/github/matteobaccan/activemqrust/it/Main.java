// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

package com.github.matteobaccan.activemqrust.it;

import java.util.HashMap;
import java.util.Map;

/**
 * Entry point.
 * <pre>
 *   java -jar mqrust-acceptance.jar [accept] --url tcp://127.0.0.1:61616 --user admin --password admin [--only 1|2|3]
 *   java -jar mqrust-acceptance.jar expiry-options --url ... (broker started with the [expiry] options in ExpiryOptions)
 *   java -jar mqrust-acceptance.jar bench --url ... --user ... --password ... --scenario hold|throughput|latency [options]
 * </pre>
 */
public final class Main {

    private Main() {
    }

    static Map<String, String> parse(String[] args, int from) {
        Map<String, String> m = new HashMap<>();
        for (int i = from; i < args.length; i++) {
            String a = args[i];
            if (!a.startsWith("--")) {
                throw new IllegalArgumentException("unexpected argument: " + a);
            }
            String key = a.substring(2);
            String value = (i + 1 < args.length && !args[i + 1].startsWith("--")) ? args[++i] : "true";
            m.put(key, value);
        }
        return m;
    }

    public static void main(String[] args) throws Exception {
        String mode = args.length > 0 && !args[0].startsWith("--") ? args[0] : "accept";
        int from = args.length > 0 && !args[0].startsWith("--") ? 1 : 0;
        Map<String, String> opts = parse(args, from);
        String url = opts.getOrDefault("url", "tcp://127.0.0.1:61616");
        String user = opts.getOrDefault("user", "admin");
        String password = opts.getOrDefault("password", "admin");
        int code;
        switch (mode) {
            case "accept":
                code = new Acceptance(url, user, password).run(opts.get("only"));
                break;
            case "integration":
                code = new Integration(url, user, password, opts.containsKey("long")).runAll(opts.get("only"));
                break;
            case "compression":
                code = new CompressionChecks(url, user, password, opts).run();
                break;
            case "compression-golden":
                code = new CompressionChecks(url, user, password, opts).golden();
                break;
            case "expiry-options":
                code = new ExpiryOptions(url, user, password).run();
                break;
            case "bench":
                code = new Bench(url, user, password, opts).run();
                break;
            default:
                System.err.println("unknown mode: " + mode + " (use accept or bench)");
                code = 2;
        }
        System.exit(code);
    }
}
