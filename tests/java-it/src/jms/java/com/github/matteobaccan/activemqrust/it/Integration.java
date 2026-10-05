// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

package com.github.matteobaccan.activemqrust.it;

import java.io.Serializable;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.Enumeration;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.UUID;
import javax.jms.BytesMessage;
import javax.jms.Connection;
import javax.jms.DeliveryMode;
import javax.jms.Destination;
import javax.jms.MapMessage;
import javax.jms.Message;
import javax.jms.MessageConsumer;
import javax.jms.MessageProducer;
import javax.jms.ObjectMessage;
import javax.jms.Queue;
import javax.jms.QueueBrowser;
import javax.jms.Session;
import javax.jms.StreamMessage;
import javax.jms.TemporaryQueue;
import javax.jms.TemporaryTopic;
import javax.jms.TextMessage;
import javax.jms.Topic;
import org.apache.activemq.ActiveMQConnection;
import org.apache.activemq.ActiveMQConnectionFactory;
import org.apache.activemq.RedeliveryPolicy;
import org.apache.activemq.command.ActiveMQMessage;

/**
 * Integration suite against a running broker with the original ActiveMQ driver.
 * Scenarios that test ActiveMQRust-only features (broker-side compression) are skipped on other brokers.
 * The "selectors" scenario prints SELECTOR lines that can be compared between brokers.
 */
public final class Integration {

    private final String url;
    private final String user;
    private final String password;
    private final boolean longTests;
    private int pass;
    private int fail;
    private int skip;

    public Integration(String url, String user, String password, boolean longTests) {
        this.url = url;
        this.user = user;
        this.password = password;
        this.longTests = longTests;
    }

    interface Check {
        void run() throws Exception;
    }

    static final class Skip extends RuntimeException {
        Skip(String why) {
            super(why);
        }
    }

    private void run(String name, String only, Check c) {
        if (only != null && !only.equals(name)) {
            return;
        }
        long t0 = System.currentTimeMillis();
        try {
            c.run();
            pass++;
            System.out.println("PASS " + name + " (" + (System.currentTimeMillis() - t0) + " ms)");
        } catch (Skip s) {
            skip++;
            System.out.println("SKIP " + name + ": " + s.getMessage());
        } catch (Throwable t) {
            fail++;
            System.out.println("FAIL " + name + ": " + t);
        }
    }

    public int runAll(String only) {
        run("fifo10k", only, this::fifo10k);
        run("messageTypes", only, this::messageTypes);
        run("requestReplyTempQueue", only, () -> requestReply(false));
        run("requestReplyTempTopic", only, () -> requestReply(true));
        run("clientAckRecover", only, this::clientAckRecover);
        run("queueBrowser", only, this::queueBrowser);
        run("topic3Subscribers", only, this::topic3);
        run("redeliveryAfterConnectionDrop", only, this::redeliveryAfterDrop);
        run("transactions", only, this::transactions);
        run("maxRedeliveriesToDlq", only, this::maxRedeliveriesToDlq);
        run("clientCompression", only, this::clientCompression);
        run("brokerCompression", only, this::brokerCompression);
        run("expiration", only, this::expiration);
        run("selectors", only, this::selectors);
        if (longTests || "idleKeepAlive".equals(only)) {
            run("idleKeepAlive", only, this::idleKeepAlive);
        }
        System.out.println("SUMMARY pass=" + pass + " fail=" + fail + " skip=" + skip);
        return fail == 0 ? 0 : 1;
    }

    // ------------------------------------------------------------------------------------------

    private static void check(boolean ok, String what) {
        if (!ok) {
            throw new AssertionError(what);
        }
    }

    private static String name(String prefix) {
        return prefix + "." + UUID.randomUUID().toString().substring(0, 8);
    }

    private Connection connect(ActiveMQConnectionFactory f) throws Exception {
        Connection c = f.createConnection(user, password);
        c.start();
        return c;
    }

    private Connection connect() throws Exception {
        return connect(new ActiveMQConnectionFactory(url));
    }

    /** A factory allowed to deserialize ObjectMessage payloads (client-side security setting). */
    private ActiveMQConnectionFactory trusting() {
        ActiveMQConnectionFactory f = new ActiveMQConnectionFactory(url);
        f.setTrustAllPackages(true);
        return f;
    }

    private static boolean isActiveMQRust(Connection c) throws Exception {
        String n = ((ActiveMQConnection) c).getBrokerName();
        return n != null && n.startsWith("ActiveMQRust");
    }

    void fifo10k() throws Exception {
        try (Connection c = connect()) {
            Session s = c.createSession(false, Session.AUTO_ACKNOWLEDGE);
            Queue q = s.createQueue(name("IT.FIFO10K"));
            MessageProducer p = s.createProducer(q);
            p.setDeliveryMode(DeliveryMode.NON_PERSISTENT);
            List<String> ids = new ArrayList<>();
            for (int i = 0; i < 10_000; i++) {
                TextMessage m = s.createTextMessage("m" + i);
                m.setIntProperty("seq", i);
                p.send(m);
                ids.add(m.getJMSMessageID());
            }
            MessageConsumer consumer = s.createConsumer(q);
            for (int i = 0; i < 10_000; i++) {
                Message m = consumer.receive(10_000);
                check(m != null, "missing message " + i);
                check(m.getIntProperty("seq") == i, "out of order at " + i + ": got " + m.getIntProperty("seq"));
                check(ids.get(i).equals(m.getJMSMessageID()), "JMSMessageID differs at " + i);
            }
            check(consumer.receive(500) == null, "extra message");
        }
    }

    private static void setAllProperties(Message m) throws Exception {
        m.setBooleanProperty("pBool", true);
        m.setByteProperty("pByte", (byte) 7);
        m.setShortProperty("pShort", (short) 300);
        m.setIntProperty("pInt", 70000);
        m.setLongProperty("pLong", 1L << 40);
        m.setFloatProperty("pFloat", 1.5f);
        m.setDoubleProperty("pDouble", 2.25);
        m.setStringProperty("pString", "città");
    }

    private static void checkAllProperties(Message m) throws Exception {
        check(m.getBooleanProperty("pBool"), "boolean property");
        check(m.getByteProperty("pByte") == 7, "byte property");
        check(m.getShortProperty("pShort") == 300, "short property");
        check(m.getIntProperty("pInt") == 70000, "int property");
        check(m.getLongProperty("pLong") == (1L << 40), "long property");
        check(m.getFloatProperty("pFloat") == 1.5f, "float property");
        check(m.getDoubleProperty("pDouble") == 2.25, "double property");
        check("città".equals(m.getStringProperty("pString")), "string property");
    }

    void messageTypes() throws Exception {
        try (Connection c = connect(trusting())) {
            Session s = c.createSession(false, Session.AUTO_ACKNOWLEDGE);
            Queue q = s.createQueue(name("IT.TYPES"));
            MessageProducer p = s.createProducer(q);
            TextMessage t = s.createTextMessage("héllo wörld 😀");
            BytesMessage b = s.createBytesMessage();
            b.writeBytes(new byte[] {1, 2, 3, 4, 5});
            b.writeInt(42);
            MapMessage mm = s.createMapMessage();
            mm.setString("name", "abc");
            mm.setInt("qty", 5);
            mm.setDouble("price", 9.99);
            ObjectMessage o = s.createObjectMessage(new ArrayList<>(Arrays.asList("x", "y")));
            StreamMessage st = s.createStreamMessage();
            st.writeBoolean(true);
            st.writeLong(42L);
            st.writeString("x");
            Message[] all = {t, b, mm, o, st};
            for (Message m : all) {
                setAllProperties(m);
                p.send(m);
            }
            MessageConsumer consumer = s.createConsumer(q);
            TextMessage rt = (TextMessage) consumer.receive(5000);
            check(rt != null && "héllo wörld 😀".equals(rt.getText()), "text body");
            checkAllProperties(rt);
            BytesMessage rb = (BytesMessage) consumer.receive(5000);
            byte[] five = new byte[5];
            check(rb.readBytes(five) == 5 && Arrays.equals(five, new byte[] {1, 2, 3, 4, 5}), "bytes body");
            check(rb.readInt() == 42, "bytes body int");
            checkAllProperties(rb);
            MapMessage rm = (MapMessage) consumer.receive(5000);
            check("abc".equals(rm.getString("name")) && rm.getInt("qty") == 5 && rm.getDouble("price") == 9.99, "map body");
            checkAllProperties(rm);
            ObjectMessage ro = (ObjectMessage) consumer.receive(5000);
            Serializable obj = ro.getObject();
            check(Arrays.asList("x", "y").equals(obj), "object body");
            checkAllProperties(ro);
            StreamMessage rs = (StreamMessage) consumer.receive(5000);
            check(rs.readBoolean() && rs.readLong() == 42L && "x".equals(rs.readString()), "stream body");
            checkAllProperties(rs);
        }
    }

    void requestReply(boolean topic) throws Exception {
        try (Connection requester = connect(); Connection responder = connect()) {
            Session rs = requester.createSession(false, Session.AUTO_ACKNOWLEDGE);
            Session ss = responder.createSession(false, Session.AUTO_ACKNOWLEDGE);
            Queue service = ss.createQueue(name("IT.SERVICE"));
            MessageConsumer serviceConsumer = ss.createConsumer(service);
            MessageProducer replier = ss.createProducer(null);
            Destination replyTo = topic ? rs.createTemporaryTopic() : rs.createTemporaryQueue();
            MessageConsumer replies = rs.createConsumer(replyTo);
            TextMessage req = rs.createTextMessage("ping");
            req.setJMSReplyTo(replyTo);
            req.setJMSCorrelationID("corr-1");
            rs.createProducer(service).send(req);
            TextMessage got = (TextMessage) serviceConsumer.receive(5000);
            check(got != null, "request not received");
            check(got.getJMSReplyTo() != null, "JMSReplyTo missing");
            TextMessage reply = ss.createTextMessage("pong");
            reply.setJMSCorrelationID(got.getJMSCorrelationID());
            replier.send(got.getJMSReplyTo(), reply);
            TextMessage back = (TextMessage) replies.receive(5000);
            check(back != null && "pong".equals(back.getText()), "reply not received");
            check("corr-1".equals(back.getJMSCorrelationID()), "correlation id lost");
            replies.close();
            if (topic) {
                ((TemporaryTopic) replyTo).delete();
            } else {
                ((TemporaryQueue) replyTo).delete();
            }
        }
    }

    void clientAckRecover() throws Exception {
        try (Connection c = connect()) {
            Session s = c.createSession(false, Session.CLIENT_ACKNOWLEDGE);
            Queue q = s.createQueue(name("IT.CLIENTACK"));
            MessageProducer p = s.createProducer(q);
            for (int i = 0; i < 3; i++) {
                p.send(s.createTextMessage("c" + i));
            }
            MessageConsumer consumer = s.createConsumer(q);
            List<String> first = new ArrayList<>();
            for (int i = 0; i < 3; i++) {
                Message m = consumer.receive(5000);
                check(m != null && !m.getJMSRedelivered(), "first delivery " + i);
                first.add(m.getJMSMessageID());
            }
            s.recover();
            Message last = null;
            for (int i = 0; i < 3; i++) {
                Message m = consumer.receive(5000);
                check(m != null, "redelivery " + i);
                check(m.getJMSRedelivered(), "JMSRedelivered false after recover");
                check(first.get(i).equals(m.getJMSMessageID()), "different message after recover");
                last = m;
            }
            last.acknowledge();
            consumer.close();
            MessageConsumer again = s.createConsumer(q);
            check(again.receive(1000) == null, "acknowledged messages delivered again");
        }
    }

    void queueBrowser() throws Exception {
        try (Connection c = connect()) {
            Session s = c.createSession(false, Session.AUTO_ACKNOWLEDGE);
            Queue q = s.createQueue(name("IT.BROWSE"));
            MessageProducer p = s.createProducer(q);
            for (int i = 0; i < 5; i++) {
                p.send(s.createTextMessage("b" + i));
            }
            QueueBrowser browser = s.createBrowser(q);
            Enumeration<?> e = browser.getEnumeration();
            List<String> seen = new ArrayList<>();
            while (e.hasMoreElements()) {
                seen.add(((TextMessage) e.nextElement()).getText());
            }
            browser.close();
            check(seen.equals(Arrays.asList("b0", "b1", "b2", "b3", "b4")), "browsed " + seen);
            MessageConsumer consumer = s.createConsumer(q);
            for (int i = 0; i < 5; i++) {
                Message m = consumer.receive(5000);
                check(m != null && ("b" + i).equals(((TextMessage) m).getText()), "browsing consumed messages");
            }
        }
    }

    void topic3() throws Exception {
        try (Connection c = connect()) {
            Session s = c.createSession(false, Session.AUTO_ACKNOWLEDGE);
            Topic t = s.createTopic(name("IT.TOPIC"));
            List<MessageConsumer> subs = new ArrayList<>();
            for (int i = 0; i < 3; i++) {
                subs.add(s.createConsumer(t));
            }
            MessageProducer p = s.createProducer(t);
            for (int i = 0; i < 100; i++) {
                TextMessage m = s.createTextMessage("t" + i);
                m.setIntProperty("seq", i);
                p.send(m);
            }
            for (int k = 0; k < 3; k++) {
                for (int i = 0; i < 100; i++) {
                    Message m = subs.get(k).receive(5000);
                    check(m != null && m.getIntProperty("seq") == i, "subscriber " + k + " message " + i);
                }
            }
        }
    }

    void redeliveryAfterDrop() throws Exception {
        String q = name("IT.DROP");
        try (Connection c = connect()) {
            Session s = c.createSession(false, Session.AUTO_ACKNOWLEDGE);
            MessageProducer p = s.createProducer(s.createQueue(q));
            for (int i = 0; i < 5; i++) {
                p.send(s.createTextMessage("d" + i));
            }
        }
        Connection victim = connect();
        Session vs = victim.createSession(false, Session.CLIENT_ACKNOWLEDGE);
        MessageConsumer vc = vs.createConsumer(vs.createQueue(q));
        for (int i = 0; i < 3; i++) {
            check(vc.receive(5000) != null, "victim did not receive " + i);
        }
        // Drop the connection without acknowledging.
        ((ActiveMQConnection) victim).getTransport().stop();
        try (Connection c = connect()) {
            Session s = c.createSession(false, Session.AUTO_ACKNOWLEDGE);
            MessageConsumer consumer = s.createConsumer(s.createQueue(q));
            for (int i = 0; i < 5; i++) {
                TextMessage m = (TextMessage) consumer.receive(10_000);
                check(m != null, "message " + i + " lost after the drop");
                check(("d" + i).equals(m.getText()), "order after drop: expected d" + i + " got " + m.getText());
                if (i < 3) {
                    check(m.getJMSRedelivered(), "d" + i + " not marked redelivered");
                }
            }
        }
        try {
            victim.close();
        } catch (Exception ignored) {
            // already broken
        }
    }

    void transactions() throws Exception {
        try (Connection c = connect()) {
            Session tx = c.createSession(true, Session.SESSION_TRANSACTED);
            Session plain = c.createSession(false, Session.AUTO_ACKNOWLEDGE);
            Queue q = tx.createQueue(name("IT.TX"));
            MessageProducer p = tx.createProducer(q);
            for (int i = 0; i < 5; i++) {
                p.send(tx.createTextMessage("x" + i));
            }
            MessageConsumer watcher = plain.createConsumer(q);
            check(watcher.receive(1000) == null, "uncommitted messages are visible");
            watcher.close();
            tx.commit();
            MessageConsumer consumer = tx.createConsumer(q);
            Message m0 = consumer.receive(5000);
            Message m1 = consumer.receive(5000);
            check(m0 != null && m1 != null, "committed messages not received");
            tx.rollback();
            Message r0 = consumer.receive(5000);
            check(r0 != null && r0.getJMSRedelivered(), "rolled back message not redelivered");
            check(((TextMessage) r0).getText().equals("x0"), "rollback changed the order");
            for (int i = 1; i < 5; i++) {
                check(consumer.receive(5000) != null, "message x" + i + " missing");
            }
            tx.commit();
            consumer.close();
            MessageConsumer after = plain.createConsumer(q);
            check(after.receive(1000) == null, "committed acks did not consume");
        }
    }

    void maxRedeliveriesToDlq() throws Exception {
        ActiveMQConnectionFactory f = new ActiveMQConnectionFactory(url);
        RedeliveryPolicy policy = new RedeliveryPolicy();
        policy.setMaximumRedeliveries(1);
        policy.setInitialRedeliveryDelay(0);
        policy.setRedeliveryDelay(0);
        f.setRedeliveryPolicy(policy);
        String q = name("IT.POISON");
        String marker = UUID.randomUUID().toString();
        try (Connection c = connect(f)) {
            Session s = c.createSession(true, Session.SESSION_TRANSACTED);
            MessageProducer p = s.createProducer(s.createQueue(q));
            p.setDeliveryMode(DeliveryMode.PERSISTENT);
            TextMessage m = s.createTextMessage("poison");
            m.setStringProperty("marker", marker);
            p.send(m);
            s.commit();
            MessageConsumer consumer = s.createConsumer(s.createQueue(q));
            for (int i = 0; i < 2; i++) {
                check(consumer.receive(5000) != null, "delivery " + i);
                s.rollback();
            }
            check(consumer.receive(1000) == null, "message still delivered after maximumRedeliveries");
            consumer.close();
            MessageConsumer dlq = s.createConsumer(s.createQueue("ActiveMQ.DLQ"), "marker = '" + marker + "'");
            Message d = dlq.receive(5000);
            s.commit();
            check(d != null, "poison message not in ActiveMQ.DLQ");
        }
    }

    void clientCompression() throws Exception {
        ActiveMQConnectionFactory f = new ActiveMQConnectionFactory(url);
        f.setUseCompression(true);
        String text = String.join("", Collections.nCopies(5000, "<field>value</field>")).substring(0, 50 * 1024);
        String q = name("IT.CLIENTZIP");
        try (Connection c = connect(f)) {
            Session s = c.createSession(false, Session.AUTO_ACKNOWLEDGE);
            s.createProducer(s.createQueue(q)).send(s.createTextMessage(text));
        }
        try (Connection c = connect()) {
            Session s = c.createSession(false, Session.AUTO_ACKNOWLEDGE);
            TextMessage m = (TextMessage) s.createConsumer(s.createQueue(q)).receive(5000);
            check(m != null, "compressed message not received");
            check(((ActiveMQMessage) m).isCompressed(), "message no longer compressed");
            check(text.equals(m.getText()), "compressed text differs");
        }
    }

    void brokerCompression() throws Exception {
        try (Connection c = connect(trusting())) {
            if (!isActiveMQRust(c)) {
                throw new Skip("broker-side compression is an ActiveMQRust feature");
            }
            Session s = c.createSession(false, Session.AUTO_ACKNOWLEDGE);
            Queue q = s.createQueue(name("IT.BROKERZIP"));
            MessageProducer p = s.createProducer(q);
            MessageConsumer consumer = s.createConsumer(q);
            // TextMessage content = 4-byte length + UTF-8, so text of 32764 chars is exactly 32 KB.
            int[] lengths = {32763, 32764, 32765};
            boolean[] expected = {false, false, true};
            for (int i = 0; i < lengths.length; i++) {
                String text = String.join("", Collections.nCopies(lengths[i] / 4 + 1, "abcd")).substring(0, lengths[i]);
                p.send(s.createTextMessage(text));
                TextMessage m = (TextMessage) consumer.receive(5000);
                check(m != null, "text " + lengths[i] + " not received");
                check(((ActiveMQMessage) m).isCompressed() == expected[i],
                        "text of " + (lengths[i] + 4) + " content bytes: compressed=" + ((ActiveMQMessage) m).isCompressed());
                check(text.equals(m.getText()), "text body changed by compression");
            }
            byte[] big = new byte[64 * 1024];
            for (int i = 0; i < big.length; i++) {
                big[i] = (byte) (i % 16);
            }
            BytesMessage b = s.createBytesMessage();
            b.writeBytes(big);
            p.send(b);
            BytesMessage rb = (BytesMessage) consumer.receive(5000);
            check(((ActiveMQMessage) rb).isCompressed(), "bytes message not compressed");
            byte[] back = new byte[big.length];
            check(rb.readBytes(back) == big.length && Arrays.equals(back, big), "bytes body changed");
            MapMessage mm = s.createMapMessage();
            for (int i = 0; i < 3000; i++) {
                mm.setString("k" + i, "value-value-value-" + i);
            }
            p.send(mm);
            MapMessage rm = (MapMessage) consumer.receive(5000);
            check(((ActiveMQMessage) rm).isCompressed(), "map message not compressed");
            check(("value-value-value-2999").equals(rm.getString("k2999")), "map body changed");
            StreamMessage sm = s.createStreamMessage();
            for (int i = 0; i < 4000; i++) {
                sm.writeString("stream-value-" + (i % 10));
            }
            p.send(sm);
            StreamMessage rs = (StreamMessage) consumer.receive(5000);
            check(((ActiveMQMessage) rs).isCompressed(), "stream message not compressed");
            for (int i = 0; i < 4000; i++) {
                check(("stream-value-" + (i % 10)).equals(rs.readString()), "stream body changed at " + i);
            }
            ArrayList<String> list = new ArrayList<>();
            for (int i = 0; i < 5000; i++) {
                list.add("object-value-" + i);
            }
            p.send(s.createObjectMessage(list));
            ObjectMessage ro = (ObjectMessage) consumer.receive(5000);
            check(((ActiveMQMessage) ro).isCompressed(), "object message not compressed");
            check(list.equals(ro.getObject()), "object body changed");
        }
    }

    void expiration() throws Exception {
        try (Connection c = connect()) {
            Session s = c.createSession(false, Session.AUTO_ACKNOWLEDGE);
            Queue q = s.createQueue(name("IT.TTL"));
            MessageProducer p = s.createProducer(q);
            p.send(s.createTextMessage("short"), DeliveryMode.NON_PERSISTENT, 4, 500);
            Thread.sleep(2000);
            p.send(s.createTextMessage("long"), DeliveryMode.NON_PERSISTENT, 4, 60_000);
            MessageConsumer consumer = s.createConsumer(q);
            TextMessage m = (TextMessage) consumer.receive(5000);
            check(m != null && "long".equals(m.getText()), "expected only the long-lived message, got " + (m == null ? null : m.getText()));
            long ttl = m.getJMSExpiration() - m.getJMSTimestamp();
            check(ttl == 60_000, "JMSExpiration - JMSTimestamp = " + ttl);
            check(consumer.receive(1000) == null, "expired message delivered");
        }
    }

    void selectors() throws Exception {
        String[] selectors = {
            "color = 'red'", "color <> 'red'", "NOT (color = 'red')", "size > 2", "size BETWEEN 2 AND 4",
            "size NOT BETWEEN 2 AND 4", "color IN ('red','blue')", "color NOT IN ('red','blue')",
            "name LIKE 'a%'", "name LIKE '_b%'", "name NOT LIKE '%c'", "missing IS NULL", "missing IS NOT NULL",
            "missing = 'x'", "NOT (missing = 'x')", "size = '3'", "NOT (size = '3')", "flag = TRUE", "flag",
            "size * 2 > 5", "weight > 1.5", "JMSPriority > 4", "JMSCorrelationID = 'c2'", "JMSType = 'kind1'",
            "JMSDeliveryMode = 'PERSISTENT'", "color = 'red' AND size > 1", "color = 'red' OR size > 4",
            "(color = 'red' OR color = 'green') AND NOT flag", "JMSXDeliveryCount = 1", "size + 1 = 4",
        };
        String[] colors = {"red", "blue", "green", null};
        String[] names = {"abc", "bbc", "xyz", "ab", "a"};
        try (Connection c = connect()) {
            Session s = c.createSession(false, Session.AUTO_ACKNOWLEDGE);
            Map<String, List<String>> results = new LinkedHashMap<>();
            for (String sel : selectors) {
                Queue q = s.createQueue(name("IT.SEL"));
                MessageProducer p = s.createProducer(q);
                for (int i = 0; i < 12; i++) {
                    TextMessage m = s.createTextMessage("m" + i);
                    if (colors[i % 4] != null) {
                        m.setStringProperty("color", colors[i % 4]);
                    }
                    m.setIntProperty("size", i % 6);
                    m.setDoubleProperty("weight", i * 0.5);
                    m.setBooleanProperty("flag", i % 3 == 0);
                    m.setStringProperty("name", names[i % 5]);
                    m.setJMSCorrelationID("c" + (i % 4));
                    m.setJMSType("kind" + (i % 2));
                    p.send(m, i % 2 == 0 ? DeliveryMode.PERSISTENT : DeliveryMode.NON_PERSISTENT, i % 10, 0);
                }
                MessageConsumer consumer = s.createConsumer(q, sel);
                List<String> got = new ArrayList<>();
                Message m;
                while ((m = consumer.receive(500)) != null) {
                    got.add(((TextMessage) m).getText());
                }
                consumer.close();
                results.put(sel, got);
                System.out.println("SELECTOR " + sel + " => " + got);
            }
            check(results.get("color = 'red'").equals(Arrays.asList("m0", "m4", "m8")), "color = 'red' gave " + results.get("color = 'red'"));
        }
    }

    void idleKeepAlive() throws Exception {
        try (Connection c = connect()) {
            Session s = c.createSession(false, Session.AUTO_ACKNOWLEDGE);
            Queue q = s.createQueue(name("IT.IDLE"));
            Thread.sleep(70_000);
            s.createProducer(q).send(s.createTextMessage("still alive"));
            check(s.createConsumer(q).receive(5000) != null, "connection unusable after 70 s idle");
        }
    }
}
