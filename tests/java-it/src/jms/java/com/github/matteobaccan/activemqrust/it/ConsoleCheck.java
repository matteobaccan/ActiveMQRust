// ActiveMQRust by Matteo Baccan
// SPDX-License-Identifier: MIT

package com.github.matteobaccan.activemqrust.it;

import java.net.URI;
import java.net.URLEncoder;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.ArrayList;
import java.util.Base64;
import java.util.List;
import java.util.UUID;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.TimeUnit;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import javax.jms.Connection;
import javax.jms.Message;
import javax.jms.MessageConsumer;
import javax.jms.MessageProducer;
import javax.jms.Queue;
import javax.jms.Session;
import javax.jms.TextMessage;
import org.apache.activemq.ActiveMQConnectionFactory;
import org.apache.activemq.command.WireFormatInfo;
import org.apache.activemq.transport.DefaultTransportListener;
import org.apache.activemq.transport.Transport;
import org.apache.activemq.transport.TransportFactory;

/**
 * Admin console checks with the original ActiveMQ driver: the console logs in through the
 * login form for HTML pages and uses HTTP Basic for the JSON API.
 * <pre>
 *   java -jar mqrust-acceptance.jar console --url tcp://127.0.0.1:61616 --admin http://127.0.0.1:8161
 *        [--user admin --password admin --admin-user admin --admin-password admin]
 * </pre>
 */
public final class ConsoleCheck {

    private static final Pattern MESSAGE = Pattern.compile("\"messageId\":\"([^\"]+)\"[^}]*?\"position\":(\\d+)|\"position\":(\\d+)[^}]*?\"messageId\":\"([^\"]+)\"");
    private static final Pattern VERSION = Pattern.compile("\"version\":\"([^\"]+)\"");

    private final String url;
    private final String user;
    private final String password;
    private final String admin;
    private final String adminUser;
    private final String adminPassword;
    private final HttpClient http = HttpClient.newBuilder().followRedirects(HttpClient.Redirect.NEVER)
            .connectTimeout(Duration.ofSeconds(10)).build();
    private String session;
    private int pass;
    private int fail;

    public ConsoleCheck(String url, String user, String password, String admin, String adminUser, String adminPassword) {
        this.url = url;
        this.user = user;
        this.password = password;
        this.admin = admin.endsWith("/") ? admin.substring(0, admin.length() - 1) : admin;
        this.adminUser = adminUser;
        this.adminPassword = adminPassword;
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
        run("consoleLogin", this::login);
        run("providerVersion", this::providerVersion);
        run("compressedTextMessagePage", this::compressedText);
        run("browsingDoesNotConsume", this::browsingDoesNotConsume);
        System.out.println("SUMMARY pass=" + pass + " fail=" + fail);
        return fail == 0 ? 0 : 1;
    }

    // -- HTTP ---------------------------------------------------------------------------------

    private static void check(boolean ok, String what) {
        if (!ok) {
            throw new AssertionError(what);
        }
    }

    private static String enc(String s) {
        return URLEncoder.encode(s, StandardCharsets.UTF_8).replace("+", "%20");
    }

    private HttpResponse<String> page(String path) throws Exception {
        HttpRequest.Builder b = HttpRequest.newBuilder(URI.create(admin + path)).timeout(Duration.ofSeconds(30));
        if (session != null) {
            b.header("Cookie", "mqrust_session=" + session);
        }
        return http.send(b.GET().build(), HttpResponse.BodyHandlers.ofString());
    }

    private String api(String path) throws Exception {
        String basic = Base64.getEncoder().encodeToString((adminUser + ":" + adminPassword).getBytes(StandardCharsets.UTF_8));
        HttpResponse<String> r = http.send(HttpRequest.newBuilder(URI.create(admin + path)).timeout(Duration.ofSeconds(30))
                .header("Authorization", "Basic " + basic).GET().build(), HttpResponse.BodyHandlers.ofString());
        check(r.statusCode() == 200, "GET " + path + " answered " + r.statusCode());
        return r.body();
    }

    /** Logs in through the form and keeps the session cookie. */
    private void login() throws Exception {
        HttpResponse<String> r = page("/queues");
        check(r.statusCode() == 303, "unauthenticated page answered " + r.statusCode());
        check(r.headers().firstValue("Location").orElse("").startsWith("/login?next="), "redirect to the login page");
        String form = "username=" + enc(adminUser) + "&password=" + enc(adminPassword) + "&next=" + enc("/queues");
        r = http.send(HttpRequest.newBuilder(URI.create(admin + "/login"))
                .header("Content-Type", "application/x-www-form-urlencoded")
                .POST(HttpRequest.BodyPublishers.ofString(form)).build(), HttpResponse.BodyHandlers.ofString());
        check(r.statusCode() == 303, "login answered " + r.statusCode());
        String cookie = r.headers().firstValue("Set-Cookie").orElse("");
        check(cookie.startsWith("mqrust_session=") && cookie.contains("HttpOnly"), "session cookie: " + cookie);
        session = cookie.substring("mqrust_session=".length(), cookie.indexOf(';'));
        r = page("/queues");
        check(r.statusCode() == 200 && r.body().contains(">" + adminUser + "</span>"), "queues page after login");
    }

    /** The broker's WireFormatInfo carries the same version as the console. */
    private void providerVersion() throws Exception {
        CompletableFuture<WireFormatInfo> info = new CompletableFuture<>();
        Transport t = TransportFactory.connect(new URI(url));
        t.setTransportListener(new DefaultTransportListener() {
            @Override
            public void onCommand(Object command) {
                if (command instanceof WireFormatInfo) {
                    info.complete((WireFormatInfo) command);
                }
            }
        });
        t.start();
        WireFormatInfo wf;
        try {
            wf = info.get(10, TimeUnit.SECONDS);
        } finally {
            t.stop();
        }
        Object name = wf.getProperties().get("ProviderName");
        Object version = wf.getProperties().get("ProviderVersion");
        Matcher m = VERSION.matcher(api("/api/overview"));
        check(m.find(), "version in /api/overview");
        System.out.println("  ProviderName=" + name + " ProviderVersion=" + version + " console=" + m.group(1));
        check("ActiveMQRust".equals(String.valueOf(name)), "ProviderName " + name);
        check(m.group(1).equals(String.valueOf(version)), "ProviderVersion " + version + " vs console " + m.group(1));
        String footer = page("/").body();
        check(footer.contains("ActiveMQRust " + version), "footer shows ActiveMQRust " + version);
    }

    private static List<String[]> messages(String json) {
        List<String[]> out = new ArrayList<>();
        Matcher m = MESSAGE.matcher(json);
        while (m.find()) {
            out.add(m.group(1) != null ? new String[] {m.group(1), m.group(2)} : new String[] {m.group(4), m.group(3)});
        }
        return out;
    }

    private String messagePath(String queue, String[] idSeq) {
        return "/queues/" + enc(queue) + "/messages/" + enc(idSeq[0]) + "?seq=" + idSeq[1];
    }

    // -- checks -----------------------------------------------------------------------------

    /** A client with useCompression=true sends a 50 KB TextMessage; the page shows its text. */
    private void compressedText() throws Exception {
        String queueName = "CONSOLE.COMPRESS." + UUID.randomUUID().toString().substring(0, 8);
        StringBuilder sb = new StringBuilder(50 * 1024);
        int line = 0;
        while (sb.length() < 50 * 1024) {
            sb.append("line ").append(line++).append(" of a compressible console text\n");
        }
        String text = sb.substring(0, 50 * 1024);
        ActiveMQConnectionFactory f = new ActiveMQConnectionFactory(url);
        f.setUseCompression(true);
        Connection c = f.createConnection(user, password);
        try {
            c.start();
            Session s = c.createSession(false, Session.AUTO_ACKNOWLEDGE);
            Queue q = s.createQueue(queueName);
            MessageProducer p = s.createProducer(q);
            p.send(s.createTextMessage(text));
            String json = api("/api/queues/" + enc(queueName) + "/messages");
            check(json.contains("\"compressed\":true"), "compressed flag in the API: " + json);
            check(json.contains("\"compressedSize\":"), "compressed size in the API");
            List<String[]> ids = messages(json);
            check(ids.size() == 1, "one message listed, got " + ids.size());
            HttpResponse<String> r = page(messagePath(queueName, ids.get(0)));
            check(r.statusCode() == 200, "message page answered " + r.statusCode());
            String body = r.body();
            check(body.contains("line 0 of a compressible console text"), "first line shown");
            check(body.contains("line " + (line - 2) + " of a compressible console text"), "text shown up to the end");
            check(body.contains(">compressed</span>") && body.contains("bytes stored"), "compressed flag and size shown");
            MessageConsumer consumer = s.createConsumer(q);
            Message m = consumer.receive(5000);
            check(m instanceof TextMessage, "message received");
            check(text.equals(((TextMessage) m).getText()), "text received intact");
        } finally {
            c.close();
        }
    }

    /** Viewing contents and every message page (raw and formatted) consumes nothing. */
    private void browsingDoesNotConsume() throws Exception {
        String queueName = "CONSOLE.BROWSE." + UUID.randomUUID().toString().substring(0, 8);
        Connection c = new ActiveMQConnectionFactory(url).createConnection(user, password);
        try {
            c.start();
            Session s = c.createSession(false, Session.AUTO_ACKNOWLEDGE);
            Queue q = s.createQueue(queueName);
            MessageProducer p = s.createProducer(q);
            for (int i = 1; i <= 10; i++) {
                p.send(s.createTextMessage("<order id=\"" + i + "\"><item>m-" + i + "</item></order>"));
            }
            String before = api("/api/queues/" + enc(queueName));
            check(page("/queues/" + enc(queueName)).statusCode() == 200, "contents page");
            List<String[]> ids = messages(api("/api/queues/" + enc(queueName) + "/messages"));
            check(ids.size() == 10, "ten messages listed, got " + ids.size());
            for (String[] id : ids) {
                check(page(messagePath(queueName, id)).statusCode() == 200, "raw message page");
                String formatted = page(messagePath(queueName, id) + "&view=xml").body();
                check(formatted.contains("x-tag"), "formatted view");
            }
            check(before.equals(api("/api/queues/" + enc(queueName))), "counters unchanged by the views");
            MessageConsumer consumer = s.createConsumer(q);
            for (int i = 1; i <= 10; i++) {
                Message m = consumer.receive(5000);
                check(m instanceof TextMessage, "message " + i + " received");
                check(((TextMessage) m).getText().contains("m-" + i + "<"), "FIFO order at " + i);
                check(!m.getJMSRedelivered(), "message " + i + " not redelivered");
            }
        } finally {
            c.close();
        }
    }
}
