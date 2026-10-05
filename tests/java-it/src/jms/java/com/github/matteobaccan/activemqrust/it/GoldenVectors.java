// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

package com.github.matteobaccan.activemqrust.it;

import java.io.ByteArrayOutputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;

import org.apache.activemq.command.ActiveMQBytesMessage;
import org.apache.activemq.command.ActiveMQMessage;
import org.apache.activemq.command.ActiveMQQueue;
import org.apache.activemq.command.ActiveMQTempQueue;
import org.apache.activemq.command.ActiveMQTextMessage;
import org.apache.activemq.command.ActiveMQTopic;
import org.apache.activemq.command.BrokerId;
import org.apache.activemq.command.BrokerInfo;
import org.apache.activemq.command.ConnectionId;
import org.apache.activemq.command.ConnectionInfo;
import org.apache.activemq.command.ConsumerId;
import org.apache.activemq.command.ConsumerInfo;
import org.apache.activemq.command.DestinationInfo;
import org.apache.activemq.command.ExceptionResponse;
import org.apache.activemq.command.KeepAliveInfo;
import org.apache.activemq.command.LocalTransactionId;
import org.apache.activemq.command.MessageAck;
import org.apache.activemq.command.MessageDispatch;
import org.apache.activemq.command.MessageId;
import org.apache.activemq.command.MessagePull;
import org.apache.activemq.command.ProducerAck;
import org.apache.activemq.command.ProducerId;
import org.apache.activemq.command.ProducerInfo;
import org.apache.activemq.command.RemoveInfo;
import org.apache.activemq.command.RemoveSubscriptionInfo;
import org.apache.activemq.command.Response;
import org.apache.activemq.command.SessionId;
import org.apache.activemq.command.SessionInfo;
import org.apache.activemq.command.ShutdownInfo;
import org.apache.activemq.command.TransactionInfo;
import org.apache.activemq.command.WireFormatInfo;
import org.apache.activemq.command.XATransactionId;
import org.apache.activemq.openwire.OpenWireFormat;
import org.apache.activemq.util.ByteSequence;

/**
 * Writes golden byte vectors made by the ActiveMQ client's own OpenWire marshaller, one file per
 * protocol version (9 to 12) with the same commands in the same order: the commands a broker sends
 * (WireFormatInfo, BrokerInfo, MessageDispatch, ProducerAck, responses) and the messaging,
 * transaction and subscription commands. The Rust codec tests decode and re-encode every frame.
 * <pre>
 *   java -cp mqrust-acceptance.jar com.github.matteobaccan.activemqrust.it.GoldenVectors tests/data/golden/broker
 * </pre>
 */
public final class GoldenVectors {

    private GoldenVectors() {
    }

    public static void main(String[] args) throws Exception {
        Path out = Path.of(args.length > 0 ? args[0] : "tests/data/golden/broker");
        Files.createDirectories(out);
        for (int version = 9; version <= 12; version++) {
            Path file = out.resolve(String.format("v%02d.bin", version));
            Files.write(file, stream(version));
            System.out.println("written " + file);
        }
    }

    private static byte[] stream(int version) throws Exception {
        OpenWireFormat wf = new OpenWireFormat(version);
        wf.setTightEncodingEnabled(false);
        wf.setCacheEnabled(false);
        wf.setSizePrefixDisabled(false);
        wf.setStackTraceEnabled(false);
        ByteArrayOutputStream bytes = new ByteArrayOutputStream();
        for (Object command : commands(version)) {
            ByteSequence seq = wf.marshal(command);
            bytes.write(seq.getData(), seq.getOffset(), seq.getLength());
        }
        return bytes.toByteArray();
    }

    private static List<Object> commands(int version) throws Exception {
        ConnectionId conn = new ConnectionId("ID:golden-51234-1759672800000-1:1");
        SessionId session = new SessionId(conn, 1);
        ConsumerId consumer = new ConsumerId(session, 3);
        ProducerId producer = new ProducerId(session, 2);
        ActiveMQQueue queue = new ActiveMQQueue("GOLDEN.Q");
        ActiveMQTopic topic = new ActiveMQTopic("GOLDEN.T");
        ActiveMQTempQueue temp = new ActiveMQTempQueue("ID:golden-51234-1759672800000-1:1:1");
        LocalTransactionId localTx = new LocalTransactionId(conn, 5);
        XATransactionId xaTx = new XATransactionId();
        xaTx.setFormatId(0x1234);
        xaTx.setGlobalTransactionId("global".getBytes(StandardCharsets.UTF_8));
        xaTx.setBranchQualifier("branch".getBytes(StandardCharsets.UTF_8));

        List<Object> list = new ArrayList<>();

        // 1. Negotiation, as the broker sends it.
        WireFormatInfo info = new WireFormatInfo();
        info.setVersion(version);
        info.setTightEncodingEnabled(false);
        info.setCacheEnabled(false);
        info.setSizePrefixDisabled(false);
        info.setStackTraceEnabled(false);
        info.setMaxInactivityDuration(30000);
        info.setMaxInactivityDurationInitalDelay(10000);
        info.setMaxFrameSize(104857600L);
        list.add(info);

        BrokerInfo broker = new BrokerInfo();
        broker.setBrokerId(new BrokerId("ID:golden-61616-1759672800000-0:1"));
        broker.setBrokerURL("tcp://0.0.0.0:61616");
        broker.setBrokerName("ActiveMQRust");
        broker.setConnectionId(7);
        list.add(broker);

        // 2. Connection setup.
        ConnectionInfo ci = new ConnectionInfo(conn);
        ci.setUserName("admin");
        ci.setPassword("secret");
        ci.setClientId("golden-client");
        ci.setCommandId(1);
        ci.setResponseRequired(true);
        list.add(ci);
        SessionInfo si = new SessionInfo(session);
        si.setCommandId(2);
        si.setResponseRequired(true);
        list.add(si);
        ProducerInfo pi = new ProducerInfo(producer);
        pi.setDestination(queue);
        pi.setWindowSize(1024 * 1024);
        pi.setCommandId(3);
        pi.setResponseRequired(true);
        list.add(pi);

        // 3. Responses.
        Response ok = new Response();
        ok.setCorrelationId(3);
        list.add(ok);
        ExceptionResponse failed = new ExceptionResponse(new SecurityException("User name [x] or password is invalid."));
        failed.setCorrelationId(4);
        list.add(failed);

        // 4. Messages: text with headers and properties, bytes inside a transaction, an advisory.
        ActiveMQTextMessage text = new ActiveMQTextMessage();
        MessageId mid = new MessageId(producer, 42);
        mid.setBrokerSequenceId(1001);
        if (version >= 10) {
            mid.setTextView(mid.toString());
        }
        text.setMessageId(mid);
        text.setProducerId(producer);
        text.setDestination(queue);
        text.setText("golden text è中");
        text.setCorrelationId("ORD-A");
        text.setPersistent(true);
        text.setExpiration(1759672860000L);
        text.setTimestamp(1759672800000L);
        text.setPriority((byte) 7);
        text.setReplyTo(temp);
        text.setType("golden-type");
        text.setGroupID("group");
        text.setGroupSequence(3);
        text.setUserID("admin");
        text.setBrokerInTime(1759672800001L);
        text.setBrokerOutTime(1759672800002L);
        text.setStringProperty("color", "blue");
        text.setIntProperty("count", 12);
        text.setBooleanProperty("flag", true);
        text.setDoubleProperty("ratio", 0.5);
        MessageDispatch dispatch = new MessageDispatch();
        dispatch.setConsumerId(consumer);
        dispatch.setDestination(queue);
        dispatch.setMessage(text);
        dispatch.setRedeliveryCounter(2);
        list.add(dispatch);

        MessageDispatch end = new MessageDispatch();
        end.setConsumerId(consumer);
        end.setDestination(queue);
        list.add(end);

        ActiveMQBytesMessage bytesMessage = new ActiveMQBytesMessage();
        MessageId bid = new MessageId(producer, 43);
        if (version >= 10) {
            bid.setTextView(bid.toString());
        }
        bytesMessage.setMessageId(bid);
        bytesMessage.setProducerId(producer);
        bytesMessage.setDestination(topic);
        bytesMessage.setTransactionId(localTx);
        bytesMessage.writeBytes(new byte[] {1, 2, 3, 4, 5});
        bytesMessage.setCommandId(9);
        bytesMessage.setResponseRequired(true);
        list.add(bytesMessage);

        ActiveMQMessage advisory = new ActiveMQMessage();
        advisory.setMessageId(new MessageId(new ProducerId("ID:golden-61616-1759672800000-0:2:0:0"), 1));
        advisory.setDestination(new ActiveMQTopic("ActiveMQ.Advisory.TempQueue"));
        advisory.setType("Advisory");
        advisory.setDataStructure(new DestinationInfo(conn, DestinationInfo.ADD_OPERATION_TYPE, temp));
        MessageDispatch advisoryDispatch = new MessageDispatch();
        advisoryDispatch.setConsumerId(consumer);
        advisoryDispatch.setDestination(new ActiveMQTopic("ActiveMQ.Advisory.TempQueue"));
        advisoryDispatch.setMessage(advisory);
        list.add(advisoryDispatch);

        list.add(new ProducerAck(producer, 1024 + 32));

        // 5. Acks, plain and transacted.
        MessageAck standard = new MessageAck();
        standard.setAckType(MessageAck.STANDARD_ACK_TYPE);
        standard.setConsumerId(consumer);
        standard.setDestination(queue);
        standard.setFirstMessageId(mid);
        standard.setLastMessageId(mid);
        standard.setMessageCount(1);
        standard.setTransactionId(localTx);
        list.add(standard);
        MessageAck poison = new MessageAck();
        poison.setAckType(MessageAck.POISON_ACK_TYPE);
        poison.setConsumerId(consumer);
        poison.setDestination(queue);
        poison.setLastMessageId(mid);
        poison.setMessageCount(1);
        poison.setPoisonCause(new Throwable("Delivery[7] exceeds redelivery policy limit"));
        list.add(poison);
        MessageAck xaAck = new MessageAck();
        xaAck.setAckType(MessageAck.INDIVIDUAL_ACK_TYPE);
        xaAck.setConsumerId(consumer);
        xaAck.setDestination(queue);
        xaAck.setLastMessageId(mid);
        xaAck.setMessageCount(1);
        xaAck.setTransactionId(xaTx);
        list.add(xaAck);

        // 6. Transactions.
        for (byte type : new byte[] {TransactionInfo.BEGIN, TransactionInfo.COMMIT_ONE_PHASE, TransactionInfo.ROLLBACK}) {
            TransactionInfo ti = new TransactionInfo(conn, localTx, type);
            ti.setCommandId(20 + type);
            ti.setResponseRequired(true);
            list.add(ti);
        }
        TransactionInfo xaBegin = new TransactionInfo(conn, xaTx, TransactionInfo.BEGIN);
        xaBegin.setResponseRequired(true);
        list.add(xaBegin);

        // 7. Subscriptions and destinations.
        ConsumerInfo durable = new ConsumerInfo(new ConsumerId(session, 4));
        durable.setDestination(topic);
        durable.setSubscriptionName("golden-durable");
        durable.setNoLocal(true);
        durable.setSelector("color = 'blue' AND count > 3");
        durable.setPrefetchSize(100);
        durable.setClientId("golden-client");
        durable.setCommandId(30);
        durable.setResponseRequired(true);
        list.add(durable);
        ConsumerInfo plain = new ConsumerInfo(consumer);
        plain.setDestination(queue);
        plain.setPrefetchSize(0);
        plain.setBrowser(true);
        list.add(plain);
        RemoveSubscriptionInfo rsi = new RemoveSubscriptionInfo();
        rsi.setConnectionId(conn);
        rsi.setSubscriptionName("golden-durable");
        rsi.setClientId("golden-client");
        rsi.setResponseRequired(true);
        list.add(rsi);
        DestinationInfo add = new DestinationInfo(conn, DestinationInfo.ADD_OPERATION_TYPE, temp);
        add.setResponseRequired(true);
        list.add(add);
        list.add(new DestinationInfo(conn, DestinationInfo.REMOVE_OPERATION_TYPE, temp));
        MessagePull pull = new MessagePull();
        pull.setConsumerId(consumer);
        pull.setDestination(queue);
        pull.setTimeout(2000);
        pull.setResponseRequired(true);
        list.add(pull);
        RemoveInfo remove = new RemoveInfo(consumer);
        remove.setLastDeliveredSequenceId(1001);
        list.add(remove);
        list.add(new KeepAliveInfo());
        list.add(new ShutdownInfo());
        return list;
    }
}
