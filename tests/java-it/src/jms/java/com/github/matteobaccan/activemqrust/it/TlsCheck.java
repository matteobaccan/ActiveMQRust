// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

package com.github.matteobaccan.activemqrust.it;

import java.util.ArrayList;
import java.util.List;
import java.util.UUID;
import javax.jms.Connection;
import javax.jms.Message;
import javax.jms.MessageConsumer;
import javax.jms.MessageProducer;
import javax.jms.Queue;
import javax.jms.Session;
import javax.jms.TextMessage;
import org.apache.activemq.ActiveMQConnectionFactory;

/**
 * OpenWire over TLS with the original ActiveMQ driver: the same send and receive as over
 * plain TCP, over {@code ssl://} and over {@code failover:(ssl://)}.
 * <pre>
 *   java -Djavax.net.ssl.trustStore=truststore.p12 -Djavax.net.ssl.trustStorePassword=&lt;password&gt; \
 *        -jar mqrust-acceptance.jar tls --url ssl://localhost:61617
 * </pre>
 */
public final class TlsCheck {

    private final String url;
    private final String user;
    private final String password;
    private int pass;
    private int fail;

    public TlsCheck(String url, String user, String password) {
        this.url = url;
        this.user = user;
        this.password = password;
    }

    interface Check {
        void run() throws Exception;
    }

    private void run(String name, Check c) {
        long t0 = System.currentTimeMillis();
        try {
            c.run();
            pass++;
            System.out.println("PASS " + name + " (" + (System.currentTimeMillis() - t0) + " ms)");
        } catch (Throwable t) {
            fail++;
            System.out.println("FAIL " + name + ": " + t);
        }
    }

    public int run() {
        if (!url.startsWith("ssl://")) {
            System.out.println("FAIL tls: --url must be an ssl:// URL, got " + url);
            return 1;
        }
        run("sslRoundTrip", () -> roundTrip(url));
        run("failoverSslRoundTrip", () -> roundTrip("failover:(" + url + ")?maxReconnectAttempts=2"));
        run("wrongPasswordRefused", this::wrongPassword);
        System.out.println("SUMMARY pass=" + pass + " fail=" + fail);
        return fail == 0 ? 0 : 1;
    }

    private static void check(boolean ok, String what) {
        if (!ok) {
            throw new AssertionError(what);
        }
    }

    /** Sends ten messages and reads them back in order, with their properties. */
    private void roundTrip(String connectUrl) throws Exception {
        String name = "TLS.ROUNDTRIP." + UUID.randomUUID().toString().substring(0, 8);
        ActiveMQConnectionFactory f = new ActiveMQConnectionFactory(connectUrl);
        Connection c = f.createConnection(user, password);
        try {
            c.start();
            Session s = c.createSession(false, Session.AUTO_ACKNOWLEDGE);
            Queue q = s.createQueue(name);
            MessageProducer p = s.createProducer(q);
            List<String> sent = new ArrayList<>();
            for (int i = 1; i <= 10; i++) {
                TextMessage m = s.createTextMessage("message " + i);
                m.setIntProperty("seq", i);
                p.send(m);
                sent.add(m.getText());
            }
            MessageConsumer consumer = s.createConsumer(q);
            List<String> got = new ArrayList<>();
            for (int i = 1; i <= 10; i++) {
                Message m = consumer.receive(10_000);
                check(m instanceof TextMessage, "message " + i + " not received");
                check(m.getIntProperty("seq") == i, "out of order at " + i);
                got.add(((TextMessage) m).getText());
            }
            check(sent.equals(got), "bodies differ: " + got);
            check(consumer.receive(200) == null, "the queue is empty afterwards");
        } finally {
            c.close();
        }
    }

    /** TLS does not replace the password: a wrong one is still refused. */
    private void wrongPassword() throws Exception {
        ActiveMQConnectionFactory f = new ActiveMQConnectionFactory(url);
        boolean refused = false;
        try {
            Connection c = f.createConnection(user, password + "-wrong");
            c.start();
            c.close();
        } catch (javax.jms.JMSException e) {
            refused = true;
            System.out.println("  refused: " + e.getMessage());
        }
        check(refused, "a wrong password was accepted over TLS");
    }
}
