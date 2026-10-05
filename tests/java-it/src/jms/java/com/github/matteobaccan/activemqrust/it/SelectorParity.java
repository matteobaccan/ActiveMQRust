// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

package com.github.matteobaccan.activemqrust.it;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.UUID;
import javax.jms.Connection;
import javax.jms.DeliveryMode;
import javax.jms.Message;
import javax.jms.MessageConsumer;
import javax.jms.MessageProducer;
import javax.jms.Queue;
import javax.jms.Session;
import javax.jms.TextMessage;
import org.apache.activemq.ActiveMQConnectionFactory;

/**
 * Selector parity scenario: selectors that exercise ActiveMQ's type rules (byte, short, long, float and
 * string properties), NOT precedence, arithmetic and NULL handling. Each selector gets a fresh queue
 * with the same 12 messages; the received messages must equal {@link SelectorExpectations#PARITY}.
 */
final class SelectorParity {

    private final String url;
    private final String user;
    private final String password;

    SelectorParity(String url, String user, String password) {
        this.url = url;
        this.user = user;
        this.password = password;
    }

    void run() throws Exception {
        String[] colors = {"red", "blue", "green", null};
        String[] names = {"abc", "bbc", "xyz", "ab", "a"};
        Connection c = new ActiveMQConnectionFactory(url).createConnection(user, password);
        try {
            c.start();
            Session s = c.createSession(false, Session.AUTO_ACKNOWLEDGE);
            Map<String, List<String>> results = new LinkedHashMap<>();
            for (String sel : SelectorExpectations.PARITY.keySet()) {
                Queue q = s.createQueue("IT.SELP." + UUID.randomUUID().toString().substring(0, 8));
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
                    m.setByteProperty("b", (byte) i);
                    m.setShortProperty("s", (short) (i * 100));
                    m.setLongProperty("l", i * 10_000_000_000L);
                    m.setFloatProperty("f", i * 0.25f);
                    m.setStringProperty("text", String.valueOf(i % 3));
                    m.setJMSCorrelationID("c" + (i % 4));
                    m.setJMSType("kind" + (i % 2));
                    p.send(m, i % 2 == 0 ? DeliveryMode.PERSISTENT : DeliveryMode.NON_PERSISTENT, i % 10, 0);
                }
                p.close();
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
            SelectorExpectations.verify(SelectorExpectations.PARITY, results);
        } finally {
            c.close();
        }
    }
}
