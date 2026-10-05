// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

package com.github.matteobaccan.activemqrust.it;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.List;
import java.util.Map;
import java.util.UUID;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.concurrent.atomic.AtomicLong;
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
 * Benchmark workloads:
 * <ul>
 *   <li>{@code hold}: produce all, hold, consume all, with separately timed phases;</li>
 *   <li>{@code throughput}: producers and consumers running at the same time on one queue;</li>
 *   <li>{@code scale}: producers and consumers spread over several queues (default 10 / 10 / 10);</li>
 *   <li>{@code latency}: fixed send rate, end-to-end latency percentiles.</li>
 * </ul>
 * Prints timestamped PHASE lines and one RESULT line; the exit code is 1 when the run failed.
 */
public final class Bench {

    static final long SEED_RUN = 20261005L;
    static final long SEED_WARMUP = 7L;
    /** Distinct documents cycled through by the warm-up phase. */
    static final int WARMUP_DOCUMENTS = 1000;
    /** A consumer that receives nothing for this long declares the remaining messages missing. */
    private static final long IDLE_LIMIT_MS = 60_000;

    private final String baseUrl;
    private final String user;
    private final String password;
    private final String scenario;
    private final int messages;
    private final int size;
    private final boolean async;
    private final int producers;
    private final int consumers;
    private final int queues;
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
        String defaultClients = "scale".equals(scenario) ? "10" : "1";
        this.producers = Integer.parseInt(o.getOrDefault("producers", defaultClients));
        this.consumers = Integer.parseInt(o.getOrDefault("consumers", defaultClients));
        this.queues = "scale".equals(scenario)
                ? Integer.parseInt(o.getOrDefault("queues", Integer.toString(Math.min(producers, consumers))))
                : 1;
        this.rate = Integer.parseInt(o.getOrDefault("rate", "1000"));
        this.warmup = Integer.parseInt(o.getOrDefault("warmup", "0"));
        this.holdSeconds = Integer.parseInt(o.getOrDefault("hold-seconds", "10"));
        this.timeoutMs = Math.max(1, Long.parseLong(o.getOrDefault("timeout-seconds", "1800"))) * 1000L;
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

    /** Generates and size-checks a whole message set (document for seq n at index n - 1). */
    static String[] generate(long seed, int count, int size) {
        String[] docs = new String[count];
        for (int i = 0; i < count; i++) {
            docs[i] = XmlPayload.document(seed, i + 1, size);
        }
        return docs;
    }

    private BenchResult result() {
        return new BenchResult(scenario)
                .put("messages", messages)
                .put("size", size)
                .put("send", async ? "async" : "sync");
    }

    public int run() {
        try {
            switch (scenario) {
                case "hold":
                    return hold();
                case "throughput":
                case "scale":
                    return flow();
                case "latency":
                    return latency();
                default:
                    new BenchResult(scenario).fail("unknown-scenario").print();
                    return 1;
            }
        } catch (Throwable t) {
            new BenchResult(scenario).fail(t.toString()).print();
            t.printStackTrace();
            return 1;
        }
    }

    /** Exchanges warm-up messages on a separate queue (not timed), cycling 1,000 documents. */
    private void doWarmup(Session s) throws Exception {
        if (warmup <= 0) {
            return;
        }
        phase("warmup-start");
        String[] docs = generate(SEED_WARMUP, Math.min(warmup, WARMUP_DOCUMENTS), size);
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

    /** Checks the sampled documents after timing: XML structure and equality with the expected text. */
    private static String verifySample(List<int[]> keys, List<String> texts, String[] docs) {
        for (int i = 0; i < texts.size(); i++) {
            int seq = keys.get(i)[1];
            String doc = texts.get(i);
            try {
                XmlPayload.verify(doc);
            } catch (Exception e) {
                return "malformed-document-seq-" + seq + ":" + e.getMessage();
            }
            if (!doc.equals(docs[seq - 1])) {
                return "content-mismatch-seq-" + seq;
            }
        }
        return null;
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
            List<int[]> sampleKeys = new ArrayList<>();
            List<String> sample = new ArrayList<>();
            String failure = null;
            for (int i = 0; i < messages; i++) {
                Message m = c.receive(IDLE_LIMIT_MS);
                if (m == null) {
                    failure = "missing-message-expected-seq-" + expectedSeq;
                    break;
                }
                TextMessage t = (TextMessage) m;
                int seq = t.getIntProperty("seq");
                String text = t.getText();
                if (seq != expectedSeq) {
                    failure = "out-of-order-expected-seq-" + expectedSeq + "-received-seq-" + seq;
                    break;
                }
                if (text.length() != size) {
                    failure = "wrong-length-seq-" + seq + "-length-" + text.length();
                    break;
                }
                if (XmlPayload.sampled(seq, messages)) {
                    sampleKeys.add(new int[] {0, seq});
                    sample.add(text);
                }
                expectedSeq++;
            }
            long consumeMs = (System.nanoTime() - t1) / 1_000_000;
            phase("consume-end");
            if (failure == null) {
                failure = verifySample(sampleKeys, sample, docs);
            }
            BenchResult r = result()
                    .put("hold_seconds", holdSeconds)
                    .put("produce_ms", produceMs)
                    .put("consume_ms", consumeMs)
                    .put("produce_msgs_s", BenchResult.perSecond(messages, produceMs), 1)
                    .put("consume_msgs_s", BenchResult.perSecond(messages, consumeMs), 1)
                    .put("produce_mb_s", BenchResult.mbPerSecond(messages, size, produceMs), 2)
                    .put("consume_mb_s", BenchResult.mbPerSecond(messages, size, consumeMs), 2)
                    .put("samples", sample.size())
                    .put("deflate_ratio", XmlPayload.deflateRatio(sample), 4);
            if (failure != null) {
                r.fail(failure);
            }
            r.print();
            return r.exitCode();
        }
    }

    /** Per-consumer state of the flow scenarios. */
    private static final class Received {
        final int[] lastSeq;
        final List<int[]> sampleKeys = new ArrayList<>();
        final List<String> sample = new ArrayList<>();

        Received(int producers) {
            lastSeq = new int[producers];
        }
    }

    /**
     * {@code throughput} (one queue) and {@code scale} (several queues): producer p sends to queue
     * p mod Q and consumer c reads queue c mod Q, all at the same time. Every message carries its
     * producer index and a per-producer {@code seq}.
     */
    private int flow() throws Exception {
        if (producers < 1 || consumers < 1 || queues < 1 || queues > Math.min(producers, consumers)) {
            throw new IllegalArgumentException("need 1 <= queues <= min(producers, consumers); got producers="
                    + producers + " consumers=" + consumers + " queues=" + queues);
        }
        int[] perProducer = new int[producers];
        long[] perQueue = new long[queues];
        int[] consumersOnQueue = new int[queues];
        for (int p = 0; p < producers; p++) {
            perProducer[p] = messages / producers + (p < messages % producers ? 1 : 0);
            perQueue[p % queues] += perProducer[p];
        }
        for (int c = 0; c < consumers; c++) {
            consumersOnQueue[c % queues]++;
        }
        String[] docs = generate(SEED_RUN + size, perProducer[0], size);
        List<Connection> conns = Collections.synchronizedList(new ArrayList<>());
        try {
            ActiveMQConnectionFactory f = new ActiveMQConnectionFactory(url());
            String[] names = new String[queues];
            AtomicLong[] receivedOnQueue = new AtomicLong[queues];
            for (int i = 0; i < queues; i++) {
                names[i] = q("BENCH.TP" + i);
                receivedOnQueue[i] = new AtomicLong();
            }
            if (warmup > 0) {
                Connection w = f.createConnection(user, password);
                conns.add(w);
                w.start();
                doWarmup(w.createSession(false, Session.AUTO_ACKNOWLEDGE));
            }
            AtomicReference<String> failure = new AtomicReference<>();
            AtomicLong lastReceive = new AtomicLong();
            CountDownLatch done = new CountDownLatch(consumers);
            Received[] state = new Received[consumers];
            for (int c = 0; c < consumers; c++) {
                Connection cc = f.createConnection(user, password);
                conns.add(cc);
                cc.start();
                Session cs = cc.createSession(false, Session.AUTO_ACKNOWLEDGE);
                int qi = c % queues;
                MessageConsumer consumer = cs.createConsumer(cs.createQueue(names[qi]));
                Received st = new Received(producers);
                state[c] = st;
                boolean strict = consumersOnQueue[qi] == 1;
                Thread t = new Thread(() -> {
                    try {
                        long lastProgress = System.nanoTime();
                        while (receivedOnQueue[qi].get() < perQueue[qi] && failure.get() == null) {
                            Message m = consumer.receive(200);
                            long now = System.nanoTime();
                            if (m == null) {
                                if (now - lastProgress > IDLE_LIMIT_MS * 1_000_000L) {
                                    failure.compareAndSet(null, "missing-messages-queue-" + qi + "-received-"
                                            + receivedOnQueue[qi].get() + "-of-" + perQueue[qi]);
                                }
                                continue;
                            }
                            lastProgress = now;
                            int prod = m.getIntProperty("producer");
                            int seq = m.getIntProperty("seq");
                            String text = ((TextMessage) m).getText();
                            int prev = st.lastSeq[prod];
                            if (strict ? seq != prev + 1 : seq <= prev) {
                                failure.compareAndSet(null, "out-of-order-producer-" + prod + "-expected-seq-"
                                        + (prev + 1) + "-received-seq-" + seq);
                                return;
                            }
                            if (text.length() != size) {
                                failure.compareAndSet(null, "wrong-length-seq-" + seq + "-length-" + text.length());
                                return;
                            }
                            st.lastSeq[prod] = seq;
                            if (XmlPayload.sampled(seq, perProducer[prod])) {
                                st.sampleKeys.add(new int[] {prod, seq});
                                st.sample.add(text);
                            }
                            receivedOnQueue[qi].incrementAndGet();
                            lastReceive.accumulateAndGet(now, Math::max);
                        }
                    } catch (Exception e) {
                        failure.compareAndSet(null, e.toString());
                    } finally {
                        done.countDown();
                    }
                }, "consumer-" + c);
                t.setDaemon(true);
                t.start();
            }
            Session[] ps = new Session[producers];
            MessageProducer[] mp = new MessageProducer[producers];
            for (int p = 0; p < producers; p++) {
                Connection pc = f.createConnection(user, password);
                conns.add(pc);
                pc.start();
                ps[p] = pc.createSession(false, Session.AUTO_ACKNOWLEDGE);
                mp[p] = ps[p].createProducer(ps[p].createQueue(names[p % queues]));
                mp[p].setDeliveryMode(DeliveryMode.NON_PERSISTENT);
            }
            CountDownLatch start = new CountDownLatch(1);
            long[] produceEnd = new long[producers];
            List<Thread> prods = new ArrayList<>();
            for (int p = 0; p < producers; p++) {
                final int idx = p;
                Thread t = new Thread(() -> {
                    try {
                        start.await();
                        for (int n = 0; n < perProducer[idx]; n++) {
                            TextMessage m = ps[idx].createTextMessage(docs[n]);
                            m.setIntProperty("producer", idx);
                            m.setIntProperty("seq", n + 1);
                            mp[idx].send(m);
                        }
                    } catch (Exception e) {
                        failure.compareAndSet(null, e.toString());
                    } finally {
                        produceEnd[idx] = System.nanoTime();
                    }
                }, "producer-" + p);
                t.setDaemon(true);
                t.start();
                prods.add(t);
            }
            phase("produce-start");
            long t0 = System.nanoTime();
            start.countDown();
            for (Thread t : prods) {
                t.join();
            }
            long produceMs = (Arrays.stream(produceEnd).max().orElse(t0) - t0) / 1_000_000;
            phase("produce-end");
            boolean finished = done.await(timeoutMs, TimeUnit.MILLISECONDS);
            long consumeMs = (Math.max(lastReceive.get(), t0) - t0) / 1_000_000;
            phase("consume-end");
            if (!finished) {
                failure.compareAndSet(null, "timeout-after-" + timeoutMs / 1000 + "-s");
            }
            List<String> sample = new ArrayList<>();
            if (failure.get() == null) {
                for (Received st : state) {
                    String bad = verifySample(st.sampleKeys, st.sample, docs);
                    if (bad != null) {
                        failure.compareAndSet(null, bad);
                        break;
                    }
                    sample.addAll(st.sample);
                }
            }
            long total = messages;
            BenchResult r = result()
                    .put("producers", producers)
                    .put("consumers", consumers)
                    .put("queues", queues)
                    .put("produce_ms", produceMs)
                    .put("consume_ms", consumeMs)
                    .put("elapsed_ms", consumeMs)
                    .put("produce_msgs_s", BenchResult.perSecond(total, produceMs), 1)
                    .put("consume_msgs_s", BenchResult.perSecond(total, consumeMs), 1)
                    .put("msgs_s", BenchResult.perSecond(total, consumeMs), 1)
                    .put("produce_mb_s", BenchResult.mbPerSecond(total, size, produceMs), 2)
                    .put("consume_mb_s", BenchResult.mbPerSecond(total, size, consumeMs), 2)
                    .put("mb_s", BenchResult.mbPerSecond(total, size, consumeMs), 2)
                    .put("samples", sample.size())
                    .put("deflate_ratio", XmlPayload.deflateRatio(sample), 4);
            if (failure.get() != null) {
                r.fail(failure.get());
            }
            r.print();
            return r.exitCode();
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
            AtomicInteger received = new AtomicInteger();
            AtomicReference<String> failure = new AtomicReference<>();
            Thread t = new Thread(() -> {
                try {
                    for (int n = 0; n < messages; n++) {
                        Message m = consumer.receive(IDLE_LIMIT_MS);
                        if (m == null) {
                            failure.compareAndSet(null, "missing-message-expected-seq-" + (n + 1));
                            return;
                        }
                        long now = System.nanoTime();
                        int seq = m.getIntProperty("seq");
                        if (seq != n + 1) {
                            failure.compareAndSet(null, "out-of-order-expected-seq-" + (n + 1) + "-received-seq-" + seq);
                            return;
                        }
                        lat[n] = (now - m.getLongProperty("sendNanos")) / 1000;
                        received.incrementAndGet();
                    }
                } catch (Exception e) {
                    failure.compareAndSet(null, e.toString());
                }
            }, "latency-consumer");
            t.setDaemon(true);
            t.start();
            phase("produce-start");
            long intervalNs = 1_000_000_000L / Math.max(1, rate);
            long next = System.nanoTime();
            for (int n = 0; n < messages; n++) {
                while (System.nanoTime() < next) {
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
            if (t.isAlive()) {
                failure.compareAndSet(null, "timeout-after-" + timeoutMs / 1000 + "-s-received-"
                        + received.get() + "-of-" + messages);
            }
            int n = received.get();
            long[] sorted = Arrays.copyOf(lat, n);
            Arrays.sort(sorted);
            BenchResult r = result()
                    .put("rate", rate)
                    .put("received", n)
                    .put("p50_us", n == 0 ? 0 : sorted[n / 2])
                    .put("p99_us", n == 0 ? 0 : sorted[Math.min(n - 1, (int) (n * 0.99))])
                    .put("max_us", n == 0 ? 0 : sorted[n - 1]);
            if (failure.get() != null) {
                r.fail(failure.get());
            }
            r.print();
            return r.exitCode();
        }
    }
}
