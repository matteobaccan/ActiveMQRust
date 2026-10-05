// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

package com.github.matteobaccan.activemqrust.it;

import java.util.ArrayList;
import java.util.List;
import java.util.UUID;
import java.util.regex.Pattern;
import javax.jms.Connection;
import javax.jms.InvalidSelectorException;
import javax.jms.JMSException;
import javax.jms.JMSSecurityException;
import javax.jms.Message;
import javax.jms.MessageConsumer;
import javax.jms.MessageProducer;
import javax.jms.Queue;
import javax.jms.Session;
import javax.jms.TextMessage;
import org.apache.activemq.ActiveMQConnectionFactory;

/** The three acceptance scenarios, run with the original ActiveMQ driver. */
public final class Acceptance {

    private static final Pattern MESSAGE_ID = Pattern.compile("^ID:.+-\\d+-\\d+-\\d+:\\d+:\\d+:\\d+:\\d+$");
    private static final long RECEIVE_MS = 5000;
    private static final long ABSENT_MS = 1000;

    private final String url;
    private final String user;
    private final String password;

    public Acceptance(String url, String user, String password) {
        this.url = url;
        this.user = user;
        this.password = password;
    }

    /** Runs the scenarios; returns the process exit code. */
    public int run(String only) {
        int failures = 0;
        if (only == null || only.equals("1")) {
            failures += report("Scenario 1 (queue round trip, FIFO, message IDs)", this::scenario1);
        }
        if (only == null || only.equals("2")) {
            failures += report("Scenario 2 (correlation ID selectors)", this::scenario2);
        }
        if (only == null || only.equals("3")) {
            failures += report("Scenario 3 (authentication)", this::scenario3);
        }
        return failures == 0 ? 0 : 1;
    }

    interface Check {
        void run() throws Exception;
    }

    private static int report(String name, Check c) {
        try {
            c.run();
            System.out.println("PASS " + name);
            return 0;
        } catch (Throwable t) {
            System.out.println("FAIL " + name + ": " + t);
            return 1;
        }
    }

    private static void check(boolean ok, String what) {
        if (!ok) {
            throw new AssertionError(what);
        }
    }

    private static String suffix() {
        return UUID.randomUUID().toString().substring(0, 8);
    }

    private Connection connect(String pass) throws JMSException {
        Connection c = new ActiveMQConnectionFactory(url).createConnection(user, pass);
        c.start();
        return c;
    }

    void scenario1() throws Exception {
        try (Connection c = connect(password)) {
            Session s = c.createSession(false, Session.AUTO_ACKNOWLEDGE);
            Queue q = s.createQueue("TEST.FIFO." + suffix());
            MessageProducer p = s.createProducer(q);
            List<String> sentIds = new ArrayList<>();
            for (int i = 1; i <= 10; i++) {
                TextMessage m = s.createTextMessage("msg-" + i);
                m.setIntProperty("seq", i);
                p.send(m);
                sentIds.add(m.getJMSMessageID());
            }
            MessageConsumer consumer = s.createConsumer(q);
            for (int i = 1; i <= 10; i++) {
                Message m = consumer.receive(RECEIVE_MS);
                check(m != null, "message " + i + " not received");
                TextMessage t = (TextMessage) m;
                check(("msg-" + i).equals(t.getText()), "expected msg-" + i + " but got " + t.getText());
                check(t.getIntProperty("seq") == i, "seq property " + t.getIntProperty("seq") + " instead of " + i);
                check(sentIds.get(i - 1).equals(t.getJMSMessageID()),
                        "JMSMessageID " + t.getJMSMessageID() + " differs from sent " + sentIds.get(i - 1));
                check(MESSAGE_ID.matcher(t.getJMSMessageID()).matches(),
                        "JMSMessageID " + t.getJMSMessageID() + " does not have the ActiveMQ structure");
                check(!t.getJMSRedelivered(), "message " + i + " is marked redelivered");
            }
            check(consumer.receive(ABSENT_MS) == null, "the queue is not empty after 10 messages");
        }
    }

    void scenario2() throws Exception {
        try (Connection c = connect(password)) {
            Session s = c.createSession(false, Session.AUTO_ACKNOWLEDGE);
            Queue q = s.createQueue("TEST.CORR." + suffix());
            MessageProducer p = s.createProducer(q);
            String[] ids = {"ORD-A", "ORD-B", "ORD-C"};
            for (int n = 1; n <= 4; n++) {
                for (String id : ids) {
                    TextMessage m = s.createTextMessage(id + "-" + n);
                    m.setJMSCorrelationID(id);
                    p.send(m);
                }
            }
            MessageConsumer filtered = s.createConsumer(q, "JMSCorrelationID IN ('ORD-A','ORD-C')");
            List<String> expected = new ArrayList<>();
            for (int n = 1; n <= 4; n++) {
                expected.add("ORD-A-" + n);
                expected.add("ORD-C-" + n);
            }
            for (String e : expected) {
                Message m = filtered.receive(RECEIVE_MS);
                check(m != null, "filtered consumer did not receive " + e);
                check(e.equals(((TextMessage) m).getText()), "expected " + e + " but got " + ((TextMessage) m).getText());
            }
            check(filtered.receive(ABSENT_MS) == null, "filtered consumer received an extra message");
            filtered.close();

            MessageConsumer all = s.createConsumer(q);
            for (int n = 1; n <= 4; n++) {
                Message m = all.receive(RECEIVE_MS);
                check(m != null, "unfiltered consumer did not receive ORD-B-" + n);
                check(("ORD-B-" + n).equals(((TextMessage) m).getText()),
                        "expected ORD-B-" + n + " but got " + ((TextMessage) m).getText());
            }
            check(all.receive(ABSENT_MS) == null, "unexpected message after the ORD-B messages");
            all.close();

            Queue q2 = s.createQueue("TEST.LIKE." + suffix());
            MessageProducer p2 = s.createProducer(q2);
            for (String id : new String[] {"ORD-A-100", "ORD-B-200", "ORD-A-300"}) {
                TextMessage m = s.createTextMessage(id);
                m.setJMSCorrelationID(id);
                p2.send(m);
            }
            MessageConsumer like = s.createConsumer(q2, "JMSCorrelationID LIKE 'ORD-A-%'");
            for (String e : new String[] {"ORD-A-100", "ORD-A-300"}) {
                Message m = like.receive(RECEIVE_MS);
                check(m != null, "LIKE consumer did not receive " + e);
                check(e.equals(m.getJMSCorrelationID()), "expected " + e + " but got " + m.getJMSCorrelationID());
            }
            check(like.receive(ABSENT_MS) == null, "LIKE consumer received ORD-B-200");

            try {
                s.createConsumer(q, "JMSCorrelationID = = 'X'");
                throw new AssertionError("invalid selector was accepted");
            } catch (InvalidSelectorException expectedError) {
                // expected
            }
        }
    }

    void scenario3() throws Exception {
        boolean rejected = false;
        try (Connection c = connect(password + "-wrong")) {
            c.createSession(false, Session.AUTO_ACKNOWLEDGE);
        } catch (JMSSecurityException e) {
            rejected = true;
        }
        check(rejected, "a wrong password was not rejected with JMSSecurityException");
        try (Connection c = connect(password)) {
            Session s = c.createSession(false, Session.AUTO_ACKNOWLEDGE);
            check(s != null, "no session with valid credentials");
        }
    }
}
