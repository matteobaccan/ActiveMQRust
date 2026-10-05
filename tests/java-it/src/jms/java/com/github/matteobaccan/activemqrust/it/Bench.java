// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

package com.github.matteobaccan.activemqrust.it;

import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.Map;
import java.util.UUID;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicReference;
import javax.jms.Connection;
import javax.jms.DeliveryMode;
import javax.jms.Message;
import javax.jms.MessageConsumer;
import javax.jms.MessageProducer;
import javax.jms.Queue;
import javax.jms.Session;
import javax.jms.TextMessage;
import org.apache.activemq.ActiveMQConnectionFactory;
import org.apache.activemq.ActiveMQMessageProducer;

/**
 * Benchmark workloads: hold (produce all, hold, consume all), throughput (producers and
 * consumers in parallel) and latency (fixed rate, end-to-end latency).
 * Prints timestamped PHASE lines and one RESULT line.
 */
public final class Bench {

    private static final long SEED_RUN = 20261005L;
    private static final long SEED_WARMUP = 7L;
    private static final double MB = 1024.0 * 1024.0;

    private final String baseUrl;
    private final String user;
    private final String password;
    private final String scenario;
    private final int messages;
    private final int size;
    private final boolean async;
    private final int producers;
    private final int consumers;
    private final int rate;
    private final int warmup;
    private final int holdSeconds;
    private final long timeoutMs;

    public Bench(String url, String user, String password, Map<String, String> o) {
        this.baseUrl = url;
        this.user = user;
        this.password = password;
        this.scenario = o.getOrDefault("scenario", "throughput");
        this.messages = Integer.parseInt(o.getOrDefault("messages", "100000"));
        this.size = Integer.parseInt(o.getOrDefault("size", "1024"));
        this.async = !"sync".equals(o.getOrDefault("send", "async"));
        this.producers = Integer.parseInt(o.getOrDefault("producers", "1"));
        this.consumers = Integer.parseInt(o.getOrDefault("consumers", o.getOrDefault("producers", "1")));
        this.rate = Integer.parseInt(o.getOrDefault("rate", "1000"));
        this.warmup = Integer.parseInt(o.getOrDefault("warmup", "0"));
        this.holdSeconds = Integer.parseInt(o.getOrDefault("hold-seconds", "10"));
        this.timeoutMs = Long.parseLong(o.getOrDefault("timeout-seconds", "1800")) * 1000L;
    }

    private String url() {
        String opts = "jms.prefetchPolicy.queuePrefetch=1000&jms.useCompression=false&"
                + (async ? "jms.useAsyncSend=true" : "jms.alwaysSyncSend=true");
        return baseUrl + (baseUrl.contains("?") ? "&" : "?") + opts;
    }

    private static void phase(String name) {
        System.out.println("PHASE " + name + " " + System.currentTimeMillis());
        System.out.flush();
    }

    private static String q(String prefix) {
        return prefix + "." + UUID.randomUUID().toString().substring(0, 8);
    }

    private static String[] generate(long seed, int count, int size) {
        String[] docs = new String[count];
        for (int i = 0; i < count; i++) {
            docs[i] = XmlPayload.document(seed, i + 1, size);
        }
        return docs;
    }

    private static double perSec(long n, long ms) {
        return ms <= 0 ? 0 : n * 1000.0 / ms;
    }

    private String common() {
        return "messages=" + messages + " size=" + size + " send=" + (async ? "async" : "sync");
    }

    public int run() {
        try {
            switch (scenario) {
                case "hold":
                    return hold();
                case "throughput":
                    return throughput();
                case "latency":
                    return latency();
                default:
                    System.out.println("RESULT scenario=" + scenario + " status=failed reason=unknown-scenario");
                    return 1;
            }
        } catch (Throwable t) {
            System.out.println("RESULT scenario=" + scenario + " status=failed reason=" + t.toString().replace(' ', '_'));
            t.printStackTrace();
            return 1;
        }
    }

    /** Exchanges warm-up messages on a separate queue (not timed). */
    private void doWarmup(Session s) throws Exception {
        if (warmup <= 0) {
            return;
        }
        phase("warmup-start");
        String[] docs = generate(SEED_WARMUP, Math.min(warmup, 1000), size);
        Queue wq = s.createQueue(q("BENCH.WARMUP"));
        MessageProducer p = s.createProducer(wq);
        p.setDeliveryMode(DeliveryMode.NON_PERSISTENT);
        MessageConsumer c = s.createConsumer(wq);
        int received = 0;
        for (int i = 0; i < warmup; i++) {
            TextMessage m = s.createTextMessage(docs[i % docs.length]);
            m.setIntProperty("seq", i + 1);
            p.send(m);
            while (c.receiveNoWait() != null) {
                received++;
            }
        }
        while (received < warmup) {
            if (c.receive(30_000) == null) {
                throw new IllegalStateException("warm-up: received " + received + " of " + warmup);
            }
            received++;
        }
        c.close();
        p.close();
        phase("warmup-end");
    }

    private int hold() throws Exception {
        String[] docs = generate(SEED_RUN + size, messages, size);
        try (Connection conn = new ActiveMQConnectionFactory(url()).createConnection(user, password)) {
            conn.start();
            Session s = conn.createSession(false, Session.AUTO_ACKNOWLEDGE);
            doWarmup(s);
            Queue queue = s.createQueue(q("BENCH.HOLD"));
            MessageProducer p = s.createProducer(queue);
            p.setDeliveryMode(DeliveryMode.NON_PERSISTENT);
            ActiveMQMessageProducer last = (ActiveMQMessageProducer) s.createProducer(queue);
            last.setDeliveryMode(DeliveryMode.NON_PERSISTENT);
            last.setSendTimeout(120_000);

            phase("produce-start");
            long t0 = System.nanoTime();
            for (int i = 0; i < messages; i++) {
                TextMessage m = s.createTextMessage(docs[i]);
                m.setIntProperty("seq", i + 1);
                if (i == messages - 1 && async) {
                    last.send(m);
                } else {
                    p.send(m);
                }
            }
            long produceMs = (System.nanoTime() - t0) / 1_000_000;
            phase("produce-end");

            Thread.sleep(holdSeconds * 1000L);
            phase("hold-end");

            phase("consume-start");
            long t1 = System.nanoTime();
            MessageConsumer c = s.createConsumer(queue);
            int expectedSeq = 1;
            List<int[]> sampleSeqs = new ArrayList<>();
            List<String> sample = new ArrayList<>();
            String failure = null;
            for (int i = 0; i < messages; i++) {
                Message m = c.receive(60_000);
                if (m == null) {
                    failure = "missing-message-after-seq-" + (expectedSeq - 1);
                    break;
                }
                TextMessage t = (TextMessage) m;
                int seq = t.getIntProperty("seq");
                String text = t.getText();
                if (seq != expectedSeq || text.length() != size) {
                    failure = "bad-message-seq-" + seq;
                    break;
                }
                if (seq == 1 || seq == messages || seq % 100 == 0) {
                    sampleSeqs.add(new int[] {seq});
                    sample.add(text);
                }
                expectedSeq++;
            }
            long consumeMs = (System.nanoTime() - t1) / 1_000_000;
            phase("consume-end");
            if (failure == null) {
                for (int i = 0; i < sample.size(); i++) {
                    int seq = sampleSeqs.get(i)[0];
                    String doc = sample.get(i);
                    XmlPayload.verify(doc);
                    if (!doc.equals(docs[seq - 1])) {
                        failure = "content-mismatch-seq-" + seq;
                        break;
                    }
                    if (doc.getBytes(StandardCharsets.UTF_8).length != size) {
                        failure = "size-mismatch-seq-" + seq;
                        break;
                    }
                }
            }
            double mb = (double) messages * size / MB;
            System.out.printf("RESULT scenario=hold status=%s %s hold_seconds=%d produce_ms=%d consume_ms=%d "
                            + "produce_msgs_s=%.1f consume_msgs_s=%.1f produce_mb_s=%.2f consume_mb_s=%.2f%s%n",
                    failure == null ? "ok" : "failed", common(), holdSeconds, produceMs, consumeMs,
                    perSec(messages, produceMs), perSec(messages, consumeMs),
                    produceMs > 0 ? mb * 1000.0 / produceMs : 0, consumeMs > 0 ? mb * 1000.0 / consumeMs : 0,
                    failure == null ? "" : " reason=" + failure);
            return failure == null ? 0 : 1;
        }
    }

    private int throughput() throws Exception {
        int pairs = Math.max(producers, consumers);
        int perQueue = messages / pairs;
        String[] docs = generate(SEED_RUN + size, perQueue, size);
        List<Connection> conns = new ArrayList<>();
        try {
            ActiveMQConnectionFactory f = new ActiveMQConnectionFactory(url());
            String[] queues = new String[pairs];
            for (int i = 0; i < pairs; i++) {
                queues[i] = q("BENCH.TP" + i);
            }
            if (warmup > 0) {
                Connection w = f.createConnection(user, password);
                conns.add(w);
                w.start();
                doWarmup(w.createSession(false, Session.AUTO_ACKNOWLEDGE));
            }
            CountDownLatch ready = new CountDownLatch(pairs);
            CountDownLatch done = new CountDownLatch(pairs);
            AtomicReference<String> failure = new AtomicReference<>();
            long[] produceEnd = new long[pairs];
            List<Thread> threads = new ArrayList<>();
            for (int i = 0; i < pairs; i++) {
                Connection cc = f.createConnection(user, password);
                conns.add(cc);
                cc.start();
                Session cs = cc.createSession(false, Session.AUTO_ACKNOWLEDGE);
                MessageConsumer consumer = cs.createConsumer(cs.createQueue(queues[i]));
                final int idx = i;
                Thread t = new Thread(() -> {
                    try {
                        ready.countDown();
                        int expected = 1;
                        for (int n = 0; n < perQueue; n++) {
                            Message m = consumer.receive(60_000);
                            if (m == null) {
                                failure.compareAndSet(null, "missing-message-queue-" + idx + "-after-" + (expected - 1));
                                return;
                            }
                            int seq = m.getIntProperty("seq");
                            if (seq != expected) {
                                failure.compareAndSet(null, "out-of-order-queue-" + idx + "-seq-" + seq);
                                return;
                            }
                            expected++;
                        }
                    } catch (Exception e) {
                        failure.compareAndSet(null, e.toString().replace(' ', '_'));
                    } finally {
                        done.countDown();
                    }
                }, "consumer-" + i);
                t.start();
                threads.add(t);
            }
            ready.await();
            phase("produce-start");
            long t0 = System.nanoTime();
            List<Thread> prods = new ArrayList<>();
            for (int i = 0; i < pairs; i++) {
                Connection pc = f.createConnection(user, password);
                conns.add(pc);
                pc.start();
                Session ps = pc.createSession(false, Session.AUTO_ACKNOWLEDGE);
                MessageProducer producer = ps.createProducer(ps.createQueue(queues[i]));
                producer.setDeliveryMode(DeliveryMode.NON_PERSISTENT);
                final int idx = i;
                Thread t = new Thread(() -> {
                    try {
                        for (int n = 0; n < perQueue; n++) {
                            TextMessage m = ps.createTextMessage(docs[n]);
                            m.setIntProperty("seq", n + 1);
                            producer.send(m);
                        }
                        produceEnd[idx] = System.nanoTime();
                    } catch (Exception e) {
                        failure.compareAndSet(null, e.toString().replace(' ', '_'));
                    }
                }, "producer-" + i);
                t.start();
                prods.add(t);
            }
            for (Thread t : prods) {
                t.join();
            }
            long produceMs = (Arrays.stream(produceEnd).max().orElse(t0) - t0) / 1_000_000;
            phase("produce-end");
            boolean finished = done.await(timeoutMs, TimeUnit.MILLISECONDS);
            long elapsedMs = (System.nanoTime() - t0) / 1_000_000;
            phase("consume-end");
            long total = (long) perQueue * pairs;
            String fail = finished ? failure.get() : "timeout";
            double mb = (double) total * size / MB;
            System.out.printf("RESULT scenario=throughput status=%s %s producers=%d consumers=%d produce_ms=%d elapsed_ms=%d "
                            + "msgs_s=%.1f mb_s=%.2f%s%n",
                    fail == null ? "ok" : "failed", common(), pairs, pairs, produceMs, elapsedMs,
                    perSec(total, elapsedMs), elapsedMs > 0 ? mb * 1000.0 / elapsedMs : 0,
                    fail == null ? "" : " reason=" + fail);
            return fail == null ? 0 : 1;
        } finally {
            for (Connection c : conns) {
                try {
                    c.close();
                } catch (Exception ignored) {
                    // closing
                }
            }
        }
    }

    private int latency() throws Exception {
        String[] docs = generate(SEED_RUN + size, Math.min(messages, 10_000), size);
        try (Connection pc = new ActiveMQConnectionFactory(url()).createConnection(user, password);
             Connection cc = new ActiveMQConnectionFactory(url()).createConnection(user, password)) {
            pc.start();
            cc.start();
            Session ps = pc.createSession(false, Session.AUTO_ACKNOWLEDGE);
            Session cs = cc.createSession(false, Session.AUTO_ACKNOWLEDGE);
            doWarmup(ps);
            String name = q("BENCH.LAT");
            MessageConsumer consumer = cs.createConsumer(cs.createQueue(name));
            MessageProducer producer = ps.createProducer(ps.createQueue(name));
            producer.setDeliveryMode(DeliveryMode.NON_PERSISTENT);
            long[] lat = new long[messages];
            AtomicReference<String> failure = new AtomicReference<>();
            Thread t = new Thread(() -> {
                try {
                    for (int n = 0; n < messages; n++) {
                        Message m = consumer.receive(60_000);
                        if (m == null) {
                            failure.compareAndSet(null, "missing-message-after-" + n);
                            return;
                        }
                        lat[n] = (System.nanoTime() - m.getLongProperty("sendNanos")) / 1000;
                    }
                } catch (Exception e) {
                    failure.compareAndSet(null, e.toString().replace(' ', '_'));
                }
            });
            t.start();
            phase("produce-start");
            long intervalNs = 1_000_000_000L / Math.max(1, rate);
            long next = System.nanoTime();
            for (int n = 0; n < messages; n++) {
                long now;
                while ((now = System.nanoTime()) < next) {
                    Thread.onSpinWait();
                }
                TextMessage m = ps.createTextMessage(docs[n % docs.length]);
                m.setIntProperty("seq", n + 1);
                m.setLongProperty("sendNanos", System.nanoTime());
                producer.send(m);
                next += intervalNs;
            }
            phase("produce-end");
            t.join(timeoutMs);
            phase("consume-end");
            String fail = failure.get();
            long[] sorted = lat.clone();
            Arrays.sort(sorted);
            long p50 = sorted[sorted.length / 2];
            long p99 = sorted[Math.min(sorted.length - 1, (int) (sorted.length * 0.99))];
            long max = sorted[sorted.length - 1];
            System.out.printf("RESULT scenario=latency status=%s %s rate=%d p50_us=%d p99_us=%d max_us=%d%s%n",
                    fail == null ? "ok" : "failed", common(), rate, p50, p99, max,
                    fail == null ? "" : " reason=" + fail);
            return fail == null ? 0 : 1;
        }
    }
}
