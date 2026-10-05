// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

package com.github.matteobaccan.activemqrust.it;

import java.io.File;
import java.io.FileOutputStream;
import java.util.Arrays;
import java.util.Map;
import javax.jms.BytesMessage;
import javax.jms.Connection;
import javax.jms.MapMessage;
import javax.jms.Message;
import javax.jms.MessageConsumer;
import javax.jms.MessageProducer;
import javax.jms.ObjectMessage;
import javax.jms.Queue;
import javax.jms.Session;
import javax.jms.StreamMessage;
import javax.jms.TextMessage;
import org.apache.activemq.ActiveMQConnection;
import org.apache.activemq.ActiveMQConnectionFactory;
import org.apache.activemq.command.ActiveMQDestination;
import org.apache.activemq.command.ActiveMQMessage;
import org.apache.activemq.command.MessageId;
import org.apache.activemq.openwire.OpenWireFormat;
import org.apache.activemq.util.ByteSequence;

/**
 * Message compression checks for all five body types.
 * <pre>
 *   java -jar mqrust-acceptance.jar compression --url ... --user ... --password ... [--threshold-kb 32]
 *   java -jar mqrust-acceptance.jar compression-golden --url ... --out tests/data/compression
 * </pre>
 * The checks: a client-compressed body reaches the consumer byte-for-byte as the client
 * produced it; uncompressed bodies of exactly threshold - 1, threshold and threshold + 1
 * content bytes are read correctly, and only the last one is compressed by ActiveMQRust.
 * The golden mode writes, for each type, the frame a compressing client marshals followed by
 * the frame of the same body without compression (OpenWire version 12, loose encoding).
 */
public final class CompressionChecks {

    static final String[] TYPES = {"text", "bytes", "map", "stream", "object"};

    private final String url;
    private final String user;
    private final String password;
    private final Map<String, String> opts;
    private int pass;
    private int fail;

    public CompressionChecks(String url, String user, String password, Map<String, String> opts) {
        this.url = url;
        this.user = user;
        this.password = password;
        this.opts = opts;
    }

    private Connection connect(boolean compress) throws Exception {
        ActiveMQConnectionFactory f = new ActiveMQConnectionFactory(url);
        f.setUseCompression(compress);
        f.setTrustAllPackages(true);
        Connection c = f.createConnection(user, password);
        c.start();
        return c;
    }

    /** A compressible payload of exactly {@code n} ASCII characters. */
    static String payload(int n) {
        StringBuilder sb = new StringBuilder(n);
        String unit = "<item><name>compressible</name><value>12345</value></item>";
        while (sb.length() < n) {
            sb.append(unit);
        }
        sb.setLength(n);
        return sb.toString();
    }

    /** Creates a message of {@code type} whose body is built from a payload of {@code n} characters. */
    static Message create(Session s, String type, int n) throws Exception {
        String p = payload(n);
        switch (type) {
            case "text":
                return s.createTextMessage(p);
            case "bytes": {
                BytesMessage m = s.createBytesMessage();
                m.writeBytes(p.getBytes("US-ASCII"));
                return m;
            }
            case "map": {
                MapMessage m = s.createMapMessage();
                m.setString("k", p);
                return m;
            }
            case "stream": {
                StreamMessage m = s.createStreamMessage();
                m.writeString(p);
                return m;
            }
            case "object":
                return s.createObjectMessage(p);
            default:
                throw new IllegalArgumentException(type);
        }
    }

    /** The body read back by a consumer, as a string. */
    static String body(Message m) throws Exception {
        if (m instanceof TextMessage) {
            return ((TextMessage) m).getText();
        } else if (m instanceof BytesMessage) {
            BytesMessage b = (BytesMessage) m;
            byte[] data = new byte[(int) b.getBodyLength()];
            b.readBytes(data);
            return new String(data, "US-ASCII");
        } else if (m instanceof MapMessage) {
            return ((MapMessage) m).getString("k");
        } else if (m instanceof StreamMessage) {
            return ((StreamMessage) m).readString();
        } else if (m instanceof ObjectMessage) {
            return (String) ((ObjectMessage) m).getObject();
        }
        throw new IllegalStateException("unexpected message " + m);
    }

    /** The content bytes the client puts on the wire for this message. */
    static byte[] wireContent(Message m) {
        ActiveMQMessage copy = (ActiveMQMessage) ((ActiveMQMessage) m).copy();
        copy.storeContent();
        return bytes(copy.getContent());
    }

    static byte[] bytes(ByteSequence b) {
        return b == null ? new byte[0] : Arrays.copyOfRange(b.getData(), b.getOffset(), b.getOffset() + b.getLength());
    }

    /** Payload length that gives exactly {@code target} uncompressed content bytes. */
    static int payloadFor(Session plain, String type, int target) throws Exception {
        // Probe near the target: some types switch to a wider length field for long strings.
        int probe = target - 200;
        int overhead = wireContent(create(plain, type, probe)).length - probe;
        int n = target - overhead;
        int got = wireContent(create(plain, type, n)).length;
        if (got != target) {
            throw new IllegalStateException(type + ": content of " + got + " bytes instead of " + target);
        }
        return n;
    }

    private void check(String name, boolean ok, String detail) {
        if (ok) {
            pass++;
            System.out.println("PASS " + name);
        } else {
            fail++;
            System.out.println("FAIL " + name + ": " + detail);
        }
    }

    public int run() throws Exception {
        int threshold = Integer.parseInt(opts.getOrDefault("threshold-kb", "32")) * 1024;
        try (Connection zip = connect(true); Connection plain = connect(false)) {
            Session zs = zip.createSession(false, Session.AUTO_ACKNOWLEDGE);
            Session ps = plain.createSession(false, Session.AUTO_ACKNOWLEDGE);
            boolean rust = ((ActiveMQConnection) plain).getBrokerName().startsWith("ActiveMQRust");
            for (String type : TYPES) {
                Queue q = ps.createQueue("IT.ZIP." + type + "." + System.nanoTime());
                MessageConsumer consumer = ps.createConsumer(q);
                // 1. Client-compressed body: byte-for-byte as produced by the client.
                MessageProducer zp = zs.createProducer(q);
                Message sent = create(zs, type, 50 * 1024);
                byte[] expected = wireContent(sent);
                zp.send(sent);
                Message got = consumer.receive(5000);
                if (got == null) {
                    check(type + " client-compressed", false, "not received");
                } else {
                    byte[] content = bytes(((ActiveMQMessage) got).getContent());
                    check(type + " client-compressed byte-identical",
                            ((ActiveMQMessage) got).isCompressed() && Arrays.equals(expected, content),
                            "compressed=" + ((ActiveMQMessage) got).isCompressed() + " sent " + expected.length
                                    + " bytes, received " + content.length);
                    check(type + " client-compressed body", payload(50 * 1024).equals(body(got)), "body differs");
                }
                // 2. Boundary sizes, sent without compression.
                MessageProducer pp = ps.createProducer(q);
                for (int delta = -1; delta <= 1; delta++) {
                    int target = threshold + delta;
                    int n = payloadFor(ps, type, target);
                    pp.send(create(ps, type, n));
                    Message m = consumer.receive(5000);
                    String name = type + " uncompressed content of " + target + " bytes";
                    if (m == null) {
                        check(name, false, "not received");
                        continue;
                    }
                    boolean compressed = ((ActiveMQMessage) m).isCompressed();
                    boolean expectCompressed = rust && delta > 0;
                    check(name + " compressed=" + expectCompressed, compressed == expectCompressed,
                            "compressed=" + compressed);
                    check(name + " body", payload(n).equals(body(m)), "body differs");
                }
                consumer.close();
            }
        }
        System.out.println("SUMMARY pass=" + pass + " fail=" + fail);
        return fail == 0 ? 0 : 1;
    }

    /** Writes the golden vectors: per type, the compressed frame then the plain frame. */
    public int golden() throws Exception {
        File dir = new File(opts.getOrDefault("out", "tests/data/compression"));
        if (!dir.isDirectory() && !dir.mkdirs()) {
            throw new IllegalStateException("cannot create " + dir);
        }
        OpenWireFormat wf = new OpenWireFormat(12);
        wf.setTightEncodingEnabled(false);
        wf.setCacheEnabled(false);
        wf.setSizePrefixDisabled(false);
        try (Connection zip = connect(true); Connection plain = connect(false)) {
            Session zs = zip.createSession(false, Session.AUTO_ACKNOWLEDGE);
            Session ps = plain.createSession(false, Session.AUTO_ACKNOWLEDGE);
            int seq = 1;
            for (String type : TYPES) {
                try (FileOutputStream out = new FileOutputStream(new File(dir, type + ".bin"))) {
                    for (Session s : new Session[] {zs, ps}) {
                        ActiveMQMessage m = (ActiveMQMessage) ((ActiveMQMessage) create(s, type, 40 * 1024)).copy();
                        m.setJMSDestination(ActiveMQDestination.createDestination("GOLDEN.ZIP", ActiveMQDestination.QUEUE_TYPE));
                        m.setMessageId(new MessageId("ID:golden-1-1-1:1:1", seq++));
                        m.setProducerId(m.getMessageId().getProducerId());
                        m.storeContent();
                        ByteSequence frame = wf.marshal(m);
                        out.write(frame.getData(), frame.getOffset(), frame.getLength());
                    }
                }
                System.out.println("wrote " + new File(dir, type + ".bin"));
            }
        }
        return 0;
    }
}
