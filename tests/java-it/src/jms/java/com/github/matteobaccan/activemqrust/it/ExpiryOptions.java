// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

package com.github.matteobaccan.activemqrust.it;

import java.util.UUID;
import javax.jms.Connection;
import javax.jms.DeliveryMode;
import javax.jms.Message;
import javax.jms.MessageConsumer;
import javax.jms.MessageProducer;
import javax.jms.Queue;
import javax.jms.Session;
import javax.jms.TextMessage;
import org.apache.activemq.ActiveMQConnection;
import org.apache.activemq.ActiveMQConnectionFactory;
import org.apache.activemq.command.ActiveMQQueue;
import org.apache.activemq.command.ActiveMQTextMessage;
import org.apache.activemq.command.MessageId;
import org.apache.activemq.command.ProducerId;

/**
 * Checks the broker-side `[expiry]` options. The broker under test must be started with:
 * <pre>
 *   [expiry]
 *   ttl_ceiling_ms = 30000
 *   default_ttl_ms = 1500
 *   use_broker_clock = true
 * </pre>
 * Usage: {@code java -jar mqrust-acceptance.jar expiry-options --url ... --user ... --password ...}
 */
public final class ExpiryOptions {

    static final long CEILING_MS = 30_000;
    static final long DEFAULT_TTL_MS = 1_500;
    static final long CLOCK_TOLERANCE_MS = 50;

    private final String url;
    private final String user;
    private final String password;
    private int pass;
    private int fail;

    public ExpiryOptions(String url, String user, String password) {
        this.url = url;
        this.user = user;
        this.password = password;
    }

    interface Check {
        void run(Session s) throws Exception;
    }

    private void run(String name, Connection c, Check check) {
        long t0 = System.currentTimeMillis();
        try (Session s = c.createSession(false, Session.AUTO_ACKNOWLEDGE)) {
            check.run(s);
            pass++;
            System.out.println("PASS " + name + " (" + (System.currentTimeMillis() - t0) + " ms)");
        } catch (Throwable t) {
            fail++;
            System.out.println("FAIL " + name + ": " + t);
        }
    }

    public int run() throws Exception {
        try (Connection c = new ActiveMQConnectionFactory(url).createConnection(user, password)) {
            c.start();
            run("ttlCeiling", c, this::ttlCeiling);
            run("defaultTtl", c, this::defaultTtl);
            run("brokerClockBehind", c, s -> skewedClient(c, s, -3_600_000L));
            run("brokerClockAhead", c, s -> skewedClient(c, s, 3_600_000L));
            run("brokerClockWithoutTimestamp", c, this::withoutTimestamp);
        }
        System.out.println("SUMMARY pass=" + pass + " fail=" + fail + " skip=0");
        return fail == 0 ? 0 : 1;
    }

    private static void check(boolean ok, String what) {
        if (!ok) {
            throw new AssertionError(what);
        }
    }

    private static Queue queue(Session s, String prefix) throws Exception {
        return s.createQueue(prefix + "." + UUID.randomUUID().toString().substring(0, 8));
    }

    /** A 60 s time-to-live is capped to the 30 s ceiling, measured from the stored timestamp. */
    void ttlCeiling(Session s) throws Exception {
        Queue q = queue(s, "IT.TTL.CEILING");
        s.createProducer(q).send(s.createTextMessage("capped"), DeliveryMode.NON_PERSISTENT, 4, 60_000);
        Message m = s.createConsumer(q).receive(5000);
        check(m != null, "message not received");
        long ttl = m.getJMSExpiration() - m.getJMSTimestamp();
        check(ttl == CEILING_MS, "JMSExpiration - JMSTimestamp = " + ttl + ", expected " + CEILING_MS);
    }

    /** A message without time-to-live gets the default one and is deleted when it passes. */
    void defaultTtl(Session s) throws Exception {
        Queue q = queue(s, "IT.TTL.DEFAULT");
        MessageProducer p = s.createProducer(q);
        p.send(s.createTextMessage("default"), DeliveryMode.PERSISTENT, 4, 0);
        MessageConsumer consumer = s.createConsumer(q);
        Message m = consumer.receive(5000);
        check(m != null, "message not received");
        long ttl = m.getJMSExpiration() - m.getJMSTimestamp();
        check(ttl == DEFAULT_TTL_MS, "JMSExpiration - JMSTimestamp = " + ttl + ", expected " + DEFAULT_TTL_MS);
        consumer.close();
        p.send(s.createTextMessage("expires"), DeliveryMode.PERSISTENT, 4, 0);
        Thread.sleep(DEFAULT_TTL_MS + 1_000);
        check(s.createConsumer(q).receive(1000) == null, "message without TTL not expired by default_ttl_ms");
    }

    /**
     * A client whose clock is off by {@code skew} ms: the message carries timestamp and expiration
     * computed on that clock (sent as a raw command, since the driver always uses the local clock).
     * With the broker clock it is delivered and expires 10 s after arrival.
     */
    void skewedClient(Connection c, Session s, long skew) throws Exception {
        Queue q = queue(s, "IT.TTL.CLOCK");
        ActiveMQConnection conn = (ActiveMQConnection) c;
        ProducerId pid = new ProducerId(conn.getConnectionInfo().getConnectionId().getValue() + ":99:" + (skew > 0 ? 1 : 2));
        ActiveMQTextMessage raw = new ActiveMQTextMessage();
        raw.setText("skewed");
        raw.setDestination((ActiveMQQueue) q);
        raw.setProducerId(pid);
        raw.setMessageId(new MessageId(pid, 1));
        raw.setPersistent(false);
        long clientNow = System.currentTimeMillis() + skew;
        raw.setTimestamp(clientNow);
        raw.setExpiration(clientNow + 10_000);
        long before = System.currentTimeMillis();
        conn.syncSendPacket(raw);
        long after = System.currentTimeMillis();
        TextMessage m = (TextMessage) s.createConsumer(q).receive(5000);
        check(m != null, "message from a client with skew " + skew + " ms not delivered");
        // A small tolerance: the JVM and the broker may read the system clock with different resolution.
        check(m.getJMSTimestamp() >= before - CLOCK_TOLERANCE_MS && m.getJMSTimestamp() <= after + CLOCK_TOLERANCE_MS,
            "JMSTimestamp " + m.getJMSTimestamp() + " is not the broker arrival time (sent between " + before + " and " + after + ")");
        long ttl = m.getJMSExpiration() - m.getJMSTimestamp();
        check(ttl == 10_000, "time-to-live after rebasing = " + ttl);
    }

    /** Timestamps disabled: the default TTL is measured from, and the timestamp set to, the arrival time. */
    void withoutTimestamp(Session s) throws Exception {
        Queue q = queue(s, "IT.TTL.NOSTAMP");
        MessageProducer p = s.createProducer(q);
        p.setDisableMessageTimestamp(true);
        long before = System.currentTimeMillis();
        p.send(s.createTextMessage("no timestamp"), DeliveryMode.NON_PERSISTENT, 4, 0);
        Message m = s.createConsumer(q).receive(5000);
        check(m != null, "message not received");
        check(m.getJMSTimestamp() >= before - CLOCK_TOLERANCE_MS, "JMSTimestamp " + m.getJMSTimestamp() + " not set to the arrival time");
        check(m.getJMSExpiration() - m.getJMSTimestamp() == DEFAULT_TTL_MS, "expiration " + m.getJMSExpiration());
    }
}
