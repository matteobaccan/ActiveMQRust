# ActiveMQRust (progetto MQRust) — Broker OpenWire in Rust

**Data:** 2026-10-05
**Stato:** completa, in attesa di approvazione
**Autore:** Matteo Baccan (con Claude Code)

---

## 1. Obiettivo

Sostituire un'istanza di Apache ActiveMQ Classic con un broker scritto in Rust che:

- consumi molta meno memoria di una JVM;
- parli **OpenWire**, così che le applicazioni esistenti basate sul driver ActiveMQ (`activemq-client`, URL `tcp://host:porta`) si colleghino senza modifiche al codice né alla configurazione, a parte host e porta;
- copra le **funzioni effettivamente usate** di ActiveMQ, non tutto il prodotto.

### Criteri di successo

1. Un'applicazione Java che usa `ActiveMQConnectionFactory` si connette, si autentica, produce e consuma messaggi senza modifiche.
2. I consumer ricevono i messaggi di una coda **in ordine FIFO**, cioè nell'ordine di arrivo al broker.
3. Gli ID dei messaggi sono **strutturalmente identici** a quelli di ActiveMQ (§6.4).
4. Con broker inattivo l'occupazione di memoria è nell'ordine di pochi MB (obiettivo: < 20 MB RSS a vuoto).
5. La console di amministrazione mostra code, consumer, producer e contenuto delle code, dietro login.
6. Si ottiene un singolo eseguibile Windows (`mqrust.exe`) senza dipendenze esterne a runtime, che risponde sulla porta 61616 appena avviato (§19).

---

## 2. Requisiti

### 2.1 Requisiti espliciti (dall'utente)

| ID | Requisito |
|---|---|
| R1 | Protocollo OpenWire, riconosciuto in modo trasparente dal driver ActiveMQ. |
| R2 | Autenticazione con username e password. |
| R3 | Tutto in RAM, **nessuno storage**: al riavvio i messaggi vanno persi. |
| R4 | Code create automaticamente al primo utilizzo. |
| R5 | Nessun limite al numero di consumer e producer. |
| R6 | Console admin autenticata con: elenco code, numero di consumer e producer per coda, contenuto delle code. |
| R7 | Log essenziale all'avvio. |
| R8 | Configurazione di IP e porta di ascolto, degli utenti abilitati e dell'utente admin. |
| R9 | Consegna **FIFO**: i consumer ricevono i messaggi nell'ordine di arrivo. |
| R10 | ID dei messaggi strutturalmente compatibili con ActiveMQ. |
| R11 | Interessa **solo il build Windows** (x86_64). |
| R12 | Il broker si identifica come **`ActiveMQRust <versione>`** (es. `ActiveMQRust 0.1.0`) ovunque dichiari nome e versione (§5.2, §8, §10). |
| R13 | **Prestazioni massime** in throughput e latenza (§14). |
| R14 | Gestione della **compressione di OpenWire**: messaggi compressi dal client con `useCompression=true` (§15.1). |
| R15 | Il broker comprime i messaggi in modo **veloce** quando superano una dimensione ragionevole (§15.2). |
| R16 | **Selettori JMS** (`messageSelector`) su consumer di code e topic e su QueueBrowser, con la semantica di ActiveMQ (§16). |
| R17 | **Scadenza dei messaggi** (`JMSExpiration` / time-to-live): alla scadenza il messaggio viene **cancellato**, mai spostato in DLQ (§17). |
| R18 | Test di accettazione: **programma Java** che usa il driver ActiveMQ per collegarsi, creare una coda, inviare e rileggere 10 messaggi, e verificare un consumer che filtra per `JMSCorrelationID` (§18). |
| R19 | **Un solo eseguibile Windows senza dipendenze**: `mqrust.exe` gira su un Windows x64 pulito senza installare nulla (niente runtime VC++, .NET, Java o DLL a corredo) e senza file accessori obbligatori (§19). |
| R20 | Di default il broker risponde sulla **porta 61616**, anche senza alcun file di configurazione (§9, §19). |

### 2.2 Decisioni di progetto (default adottati, modificabili in revisione)

Sono le scelte adottate dove i requisiti non si pronunciano. Ognuna resta modificabile.

| ID | Decisione |
|---|---|
| A1 | Il client di riferimento è **Java `activemq-client` 5.x / 6.x**. Gli altri client OpenWire (NMS .NET, CMS C++) dovrebbero funzionare, ma non sono testati nella prima versione. |
| A2 | Oltre alle code servono i **topic non durevoli**, perché sono comuni nelle applicazioni ActiveMQ. Le sottoscrizioni durevoli sono escluse. |
| A3 | Servono le **transazioni locali** (sessioni `transacted`), perché sono molto usate con Spring JMS. Le transazioni XA sono escluse. |
| A4 | Servono le **destinazioni temporanee** (pattern request/reply con `JMSReplyTo`). |
| A5 | La console admin è un'interfaccia **web HTTP** su porta separata, in sola lettura. |
| A6 | La configurazione è un file **TOML**. |
| A7 | Qualunque utente autenticato può usare qualunque destinazione: non ci sono permessi per singola coda. |
| A8 | Le password nel file di configurazione possono essere in chiaro oppure come hash Argon2 (consigliato). |
| A9 | Il broker gira come applicazione console. L'installazione come servizio Windows è esclusa dalla prima versione (vedi §12). |
| A10 | Senza file di configurazione il broker parte con i default incorporati: OpenWire su `0.0.0.0:61616`, admin su `127.0.0.1:8161`, un utente OpenWire `admin`/`admin` e un admin `admin`/`admin` (le stesse credenziali di default di ActiveMQ), con un warning a ogni avvio che invita a configurare password proprie (§9.2). |

### 2.3 Esclusioni (non obiettivi della prima versione)

- Persistenza di qualunque tipo (KahaDB, JDBC).
- Sottoscrizioni durevoli ai topic.
- Transazioni XA.
- Selettori XPath/XQuery di ActiveMQ (`XPATH '...'`): rifiutati con `InvalidSelectorException`. I selettori JMS SQL-92 standard sono invece inclusi (R16, §16).
- Destinazioni wildcard (`FOO.>`, `FOO.*`) e destinazioni composite.
- Network of brokers, failover lato broker, master/slave.
- Consumer esclusivi, message group, priorità dei messaggi (incompatibili con il FIFO puro, R9).
- Protocolli diversi da OpenWire (STOMP, AMQP, MQTT).
- Tight encoding e cache di marshalling di OpenWire: si disabilitano in negoziazione (§5.2).
- Operazioni di scrittura dall'admin (svuotare o cancellare code, inviare messaggi).
- Build per Linux e macOS.

---

## 3. Approcci considerati

### 3.1 Codec OpenWire

| Approccio | Pro | Contro |
|---|---|---|
| **A. Solo loose encoding, con tight e cache disabilitate in negoziazione** (scelto) | Codec molto più semplice: niente `BooleanStream`, niente marshalling a due passaggi, niente cache. È pienamente conforme al protocollo, perché il client usa la codifica tight solo se *entrambe* le parti la annunciano. | Frame leggermente più grandi (pochi byte a comando). |
| B. Tight e loose complete, con cache | Massima efficienza sul filo. | Circa il triplo del codice e molto più rischio di bug di compatibilità. |
| C. Codec generato dalle definizioni Java (`openwire-generator`) | Copertura automatica di tutte le versioni. | Dipendenza da tooling Java/Groovy, codice generato difficile da mantenere. |

**Scelta: A.** La negoziazione di `WireFormatInfo` usa l'AND logico di `TightEncodingEnabled` e `CacheEnabled` tra client e broker. Se il broker annuncia `false` per entrambi, il client usa loose encoding senza cache. Il risparmio sul filo dell'approccio B non giustifica la complessità per questo caso d'uso.

### 3.2 Modello di concorrenza

| Approccio | Pro | Contro |
|---|---|---|
| **A. Runtime Tokio; ogni connessione ha un task di lettura e un task di scrittura; ogni destinazione è una struttura protetta da mutex che fa il dispatch al momento** (scelto) | Semplice da capire e testare, basso overhead, ordine FIFO facile da garantire. | La contesa sul mutex di una coda molto trafficata è il collo di bottiglia, accettabile per il carico previsto. |
| B. Un attore (task) per destinazione, con comunicazione via canali | Nessun lock, isolamento netto. | Più task e canali, più latenza per operazione, debug più difficile. |
| C. Thread di sistema, senza async | Nessuna dipendenza da Tokio. | Un thread per connessione: costoso in memoria con molti client, contrario all'obiettivo. |

**Scelta: A.** I lock si tengono solo per operazioni in memoria brevi, mai durante l'I/O. L'I/O avviene sempre nei task di connessione, alimentati da canali.

---

## 4. Architettura

```
                    ┌──────────────────────────────────────────┐
  client JMS ──TCP──▶  openwire listener (es. 0.0.0.0:61616)    │
                    │    └─ per connessione:                    │
                    │        reader task ──▶ connection handler │
                    │        writer task ◀── canale mpsc        │
                    │                │                          │
                    │                ▼                          │
                    │        ┌───────────────┐                  │
                    │        │  broker core  │  registry delle  │
                    │        │ (in memoria)  │  destinazioni,   │
                    │        └───────────────┘  connessioni,    │
                    │                ▲          statistiche     │
                    │                │ solo lettura (snapshot)  │
  browser ──HTTP────▶  admin listener (es. 127.0.0.1:8161)      │
                    └──────────────────────────────────────────┘
```

### 4.1 Struttura del progetto (crate unico, binario `mqrust`)

```
mqrust/
├─ Cargo.toml
├─ build.rs                risorsa di versione Windows (§19.3)
├─ .cargo/config.toml      crt-static per x86_64-pc-windows-msvc (§19.2)
├─ mqrust.example.toml
├─ scripts/check-deps.cmd  verifica delle DLL importate (§19.4)
├─ src/
│  ├─ main.rs              avvio: CLI, config, log, listener, shutdown
│  ├─ config.rs            caricamento e validazione TOML
│  ├─ auth.rs              verifica credenziali (chiaro / Argon2)
│  ├─ openwire/
│  │  ├─ mod.rs
│  │  ├─ frame.rs          framing: prefisso di lunghezza, limite MaxFrameSize
│  │  ├─ primitives.rs     int/long/string/bytes/oggetti annidati (loose)
│  │  ├─ types.rs          costanti dei tipi di dato (1 = WireFormatInfo, …)
│  │  ├─ commands.rs       struct dei comandi e degli ID
│  │  ├─ marshal.rs        encode/decode loose per versione negoziata
│  │  ├─ wireformat.rs     WireFormatInfo e negoziazione
│  │  └─ message_body.rs   decodifica body/proprietà (solo per l'admin)
│  ├─ broker/
│  │  ├─ mod.rs            Broker: registry, generatori di ID, statistiche
│  │  ├─ destination.rs    Queue e Topic: dispatch, prefetch, ack
│  │  ├─ subscription.rs   stato del consumer (prefetch, inflight)
│  │  ├─ transaction.rs    transazioni locali
│  │  └─ memory.rs         contabilità della memoria e limite opzionale
│  ├─ selector/
│  │  ├─ mod.rs            API: compile(&str) -> Selector, matches(&msg)
│  │  ├─ lexer.rs          token SQL-92 (letterali, identificatori, operatori)
│  │  ├─ parser.rs         parser a discesa ricorsiva → AST
│  │  └─ eval.rs           valutazione a tre valori (TRUE/FALSE/UNKNOWN)
│  ├─ connection.rs        macchina a stati di una connessione OpenWire
│  └─ admin/
│     ├─ mod.rs            server HTTP, autenticazione Basic
│     ├─ api.rs            endpoint JSON
│     └─ pages.rs          pagine HTML (template compilati nel binario)
└─ tests/
   ├─ codec_golden.rs      vettori di byte catturati da un client reale
   ├─ broker_semantics.rs  FIFO, ack, redelivery, transazioni
   └─ java-it/             test di integrazione con il vero activemq-client
```

### 4.2 Dipendenze previste

Identità del prodotto (R12): nome **ActiveMQRust**, versione presa da `CARGO_PKG_VERSION`. Il nome dell'eseguibile resta `mqrust.exe`.

| Crate | Uso |
|---|---|
| `tokio` | runtime async, TCP, timer, segnali (Ctrl+C) |
| `bytes` | buffer per il codec |
| `axum` | server HTTP dell'admin |
| `serde`, `toml` | configurazione |
| `clap` | argomenti da riga di comando |
| `tracing`, `tracing-subscriber` | log |
| `argon2` | hash delle password |
| `parking_lot` | mutex veloci e compatti |
| `flate2` (backend `zlib-rs`) | compressione/decompressione deflate compatibile con `java.util.zip` (§15) |
| `mimalloc` | allocatore globale veloce e con bassa frammentazione (§14) |
| `rpassword` | input nascosto della password per `hash-password` |
| `criterion` (dev) | benchmark del codec e del dispatch |
| `embed-resource` (build) | risorsa di versione Windows nell'exe (§19.3) |

Niente database e niente librerie native esterne: tutte le dipendenze sono crate Rust compilati dentro l'eseguibile. Build, collegamento statico del runtime C e verifica dell'assenza di dipendenze sono descritti in §19.

---

## 5. Protocollo OpenWire

### 5.1 Framing

- Ogni frame è composto da: lunghezza `u32` big-endian, poi 1 byte con il tipo di dato, poi il corpo.
- `SizePrefixDisabled` resta `false`: il prefisso di lunghezza è sempre presente.
- Un frame più grande di `MaxFrameSize` (configurabile, default 100 MB come ActiveMQ) chiude la connessione con un log di warning.

### 5.2 Negoziazione del wire format

1. Il client apre il TCP e invia `WireFormatInfo` (tipo 1), sempre in loose encoding.
2. Il broker invia il proprio `WireFormatInfo` con:
   - magic `ActiveMQ`, versione **12**;
   - `TightEncodingEnabled=false`, `CacheEnabled=false`, `SizePrefixDisabled=false`, `StackTraceEnabled=false`;
   - `TcpNoDelayEnabled=true`;
   - `MaxInactivityDuration` e `MaxInactivityDurationInitalDelay` uguali ai valori del client (il broker ne accetta i tempi);
   - `MaxFrameSize` dalla configurazione;
   - `ProviderName="ActiveMQRust"`, `ProviderVersion="<versione del crate>"` e `PlatformDetails="Rust <rustc>, Windows <build>, x86_64"` (R12). Sono le stesse proprietà con cui ActiveMQ annuncia `ActiveMQ` e la propria versione; il client le espone con `WireFormatInfo.getProviderName()/getProviderVersion()`.
3. Versione effettiva = `min(versione client, 12)`. Versioni supportate: **da 6 a 12**. Una versione inferiore viene rifiutata con log e chiusura della connessione.
4. Il broker invia `BrokerInfo` (tipo 2) con `BrokerId`, `brokerName` (da `broker.name`, default `ActiveMQRust`) e `brokerURL`.

> Da verificare in implementazione: i campi che cambiano tra le versioni da 6 a 12 per ogni comando usato si ricavano dai marshaller Java `org.apache.activemq.openwire.vN`. Il codec li gestisce con controlli `if version >= N` sul singolo campo.

### 5.3 Primitive in loose encoding

| Tipo | Codifica |
|---|---|
| boolean | 1 byte |
| byte / short / int / long | big-endian a dimensione fissa |
| string | flag di presenza (1 byte), poi lunghezza `u16` e UTF-8 modificato (formato `DataOutput.writeUTF`) |
| byte[] | flag di presenza, poi lunghezza `i32` e byte |
| oggetto annidato (ID, destinazioni, messaggi) | flag di presenza, poi tipo di dato (1 byte), poi corpo |
| array di oggetti | flag di presenza, poi conteggio `u16`, poi oggetti |
| Throwable | flag di presenza, poi nome della classe (string), poi messaggio (string); stack trace omesso perché `StackTraceEnabled=false` |

### 5.4 Comandi gestiti

**Ricevuti dal client:**

| Tipo | Comando | Comportamento |
|---|---|---|
| 1 | WireFormatInfo | negoziazione (§5.2) |
| 3 | ConnectionInfo | autenticazione (§7), registrazione della connessione |
| 4 | SessionInfo | registrazione della sessione |
| 5 | ConsumerInfo | creazione del consumer, creazione automatica della destinazione |
| 6 | ProducerInfo | registrazione del producer, creazione automatica della destinazione se indicata |
| 7 | TransactionInfo | BEGIN / COMMIT_ONE_PHASE / ROLLBACK / END / FORGET (§6.7) |
| 8 | DestinationInfo | creazione e rimozione di destinazioni temporanee |
| 10 | KeepAliveInfo | aggiorna il timestamp di lettura |
| 11 | ShutdownInfo | chiusura ordinata della connessione |
| 12 | RemoveInfo | rimozione di connessione, sessione, consumer o producer |
| 20 | MessagePull | consumer con prefetch 0 (§6.3) |
| 22 | MessageAck | ack (§6.5) |
| 23–29 | ActiveMQ*Message | invio di un messaggio (§6.1) |
| 9 | RemoveSubscriptionInfo | risponde con errore: sottoscrizioni durevoli non supportate |

**Inviati al client:**

| Tipo | Comando | Quando |
|---|---|---|
| 1 | WireFormatInfo | dopo quello del client |
| 2 | BrokerInfo | dopo la negoziazione |
| 10 | KeepAliveInfo | keep-alive (§5.5) |
| 21 | MessageDispatch | consegna di un messaggio a un consumer |
| 30 | Response | risposta positiva, con `correlationId = commandId` del comando |
| 31 | ExceptionResponse | errore, con Throwable (classe e messaggio) |
| 16 | ConnectionError | errore asincrono fatale prima della chiusura |
| 19 | ProducerAck | se il producer ha `windowSize > 0` (flow control asincrono) |

**Regola generale:** ogni comando con `responseRequired=true` riceve sempre una `Response` o una `ExceptionResponse`. Un comando sconosciuto o non supportato con `responseRequired=true` riceve `ExceptionResponse` (`java.lang.UnsupportedOperationException`). Senza `responseRequired`, viene registrato nel log a livello debug e ignorato.

### 5.5 Keep-alive e inattività

- Il client Java ha un InactivityMonitor (default 30 s) e chiude la connessione se non *riceve* nulla per quel tempo. Quindi il broker invia `KeepAliveInfo` se non ha scritto nulla per `MaxInactivityDuration / 2`.
- Il broker chiude la connessione se non *legge* nulla per `MaxInactivityDuration`. Se la durata negoziata è 0, il monitor è disattivato.

### 5.6 Topic advisory

Il client Java, con `watchTopicAdvisories=true` (default), crea un consumer su `ActiveMQ.Advisory.TempQueue,ActiveMQ.Advisory.TempTopic`. Il broker **accetta** i consumer sui topic `ActiveMQ.Advisory.*` e risponde con `Response`, ma non pubblica alcun advisory. Questi topic non compaiono nell'admin.

---

## 6. Semantica del broker

### 6.1 Destinazioni

- Tipi: `queue://`, `topic://`, `temp-queue://`, `temp-topic://`.
- **Creazione automatica (R4):** una destinazione nasce alla prima `ProducerInfo` con destinazione, alla prima `ConsumerInfo` o al primo messaggio inviato. Nessuna configurazione preliminare.
- Una coda vuota senza consumer né producer **resta** nel registry, come fa ActiveMQ di default, e resta visibile nell'admin. Opzione `auto_delete_empty_after_secs` (default disattivata).
- Le destinazioni temporanee appartengono alla connessione che le crea e vengono eliminate, con i loro messaggi, alla chiusura di quella connessione. Solo quella connessione può creare consumer su di esse.
- Una coda speciale `ActiveMQ.DLQ` riceve i messaggi "avvelenati" (§6.6).

### 6.2 Coda: ordinamento FIFO (R9)

Ogni coda contiene:

- `pending: BTreeMap<broker_seq, Arc<StoredMessage>>` con i messaggi in attesa, ordinati per numero di sequenza di arrivo. L'ordine delle chiavi garantisce il FIFO, mentre il reinserimento in posizione originale (regola 4) e la rimozione di messaggi scaduti in mezzo alla coda (§17) costano O(log n);
- `expiry_index: BTreeSet<(expiration, broker_seq)>` contenente solo i messaggi con scadenza (§17);
- per ogni consumer, `inflight: BTreeMap<seq, StoredMessage>` con i messaggi consegnati e non ancora confermati.

Regole:

1. Ogni messaggio riceve all'arrivo un **numero di sequenza** monotono per la coda (`broker_seq`). L'ordine di arrivo è l'ordine in cui il broker completa la ricezione del frame, serializzata dal mutex della coda.
2. Il dispatch prende **sempre dalla testa** di `pending`. Con consumer che hanno un selettore, ogni consumer riceve il **primo messaggio in ordine FIFO che soddisfa il suo selettore**. Un messaggio che nessun consumer accetta resta in coda e non blocca i successivi (§16.3).
3. Con più consumer, il broker sceglie a rotazione il prossimo consumer con prefetch disponibile. Ogni messaggio va a un solo consumer. L'ordine di *consegna* resta FIFO; ogni consumer riceve una sottosequenza ordinata.
4. Un messaggio non confermato rientra in `pending` **nella sua posizione originale**, inserito per `broker_seq` e non in coda. Succede in tre casi: chiusura del consumer, caduta della connessione, rollback della transazione. Il messaggio torna così davanti ai successivi.
5. La priorità JMS (`JMSPriority`) viene conservata nel messaggio ma **non altera l'ordine**.

> Nota: con più consumer in parallelo l'ordine di *elaborazione* applicativa non è garantito, perché due consumer lavorano in contemporanea. Per un ordine di elaborazione stretto serve un solo consumer per coda, come in ActiveMQ.

### 6.3 Prefetch e flow control verso il consumer

- Il prefetch arriva da `ConsumerInfo.prefetchSize`. Default del client: 1000 per le code, 32767 per i topic.
- Il broker invia `MessageDispatch` finché `inflight.len() < prefetchSize`.
- **Prefetch 0:** il broker consegna solo dopo un `MessagePull`. Se la coda è vuota e il pull ha un timeout, allo scadere invia un `MessageDispatch` con messaggio nullo.
- **QueueBrowser** (`ConsumerInfo.browser=true`): il broker invia una copia dei messaggi presenti in `pending`, in ordine FIFO e senza rimuoverli, poi un `MessageDispatch` con messaggio nullo che segnala la fine.

### 6.4 ID dei messaggi e degli oggetti (R10)

Gli ID sono strutturati come in ActiveMQ e si serializzano con gli stessi tipi OpenWire, quindi sono indistinguibili da quelli di un broker ActiveMQ.

| Oggetto | Tipo | Struttura | Esempio testuale |
|---|---|---|---|
| ConnectionId | 120 | `value: String` | `ID:host-51234-1759672800000-1:1` |
| SessionId | 121 | `connectionId: String`, `value: i64` | `ID:host-51234-1759672800000-1:1:1` |
| ProducerId | 123 | `connectionId: String`, `sessionId: i64`, `value: i64` | `ID:host-51234-1759672800000-1:1:1:1` |
| ConsumerId | 122 | `connectionId: String`, `sessionId: i64`, `value: i64` | `ID:host-51234-1759672800000-1:1:1:1` |
| MessageId | 110 | `producerId: ProducerId`, `producerSequenceId: i64`, `brokerSequenceId: i64` (v10+ anche `textView: String`) | `ID:host-51234-1759672800000-1:1:1:1:42` |
| BrokerId | 124 | `value: String` | `ID:host-61616-1759672800000-0:1` |

Regole:

1. Il `MessageId` è **generato dal client** e il broker lo **conserva intatto**: stessi campi, stessa rappresentazione testuale, quindi `JMSMessageID` è identico tra producer e consumer.
2. Il broker imposta solo `brokerSequenceId` con il proprio contatore globale monotono, come fa ActiveMQ.
3. Gli ID generati dal broker (`BrokerId`, eventuali messaggi creati dal broker) seguono il formato di ActiveMQ `ID:<hostname>-<porta>-<timestamp ms>-<contatore>:<n>`, prodotto da un generatore equivalente a `IdGenerator`.
4. Rappresentazione testuale del `MessageId` per log e admin: `<connectionId>:<sessionId>:<producerId>:<producerSequenceId>`, uguale a `MessageId.toString()` di ActiveMQ.
5. **Rilevamento dei duplicati:** se lo stesso producer reinvia un `MessageId` già presente nella coda, per esempio dopo un failover del client, il messaggio viene scartato e il broker risponde comunque `Response`, come la `producerAudit` di ActiveMQ. La finestra di controllo è limitata (ultimi N `producerSequenceId` per producer, N=1024).

### 6.5 Ack

`MessageAck` contiene `consumerId`, `ackType`, `firstMessageId`, `lastMessageId` e `messageCount`.

| ackType | Valore | Effetto |
|---|---|---|
| DELIVERED | 0 | Nessuna rimozione; libera spazio di prefetch se il client lo usa per l'ack ottimizzato. |
| POISON | 1 | Rimuove il messaggio da `inflight` e lo sposta in `ActiveMQ.DLQ` (§6.6). |
| STANDARD | 2 | Rimuove da `inflight` tutti i messaggi fino a `lastMessageId` incluso (ack cumulativo). |
| REDELIVERED | 3 | Incrementa il contatore di riconsegna dei messaggi indicati. |
| INDIVIDUAL | 4 | Rimuove solo il messaggio indicato. |
| UNMATCHED | 5 | Come STANDARD (topic). |
| EXPIRED | 6 | Il client ha ricevuto il messaggio già scaduto: il broker lo rimuove da `inflight` e lo tratta come scaduto (§17.3). |

Dopo ogni ack che libera spazio, il broker riprende il dispatch.

### 6.6 Riconsegna e DLQ

- La politica di riconsegna (`maximumRedeliveries`, ritardi) è **lato client**, come in ActiveMQ. Il broker si limita a:
  - incrementare `redeliveryCounter` quando un messaggio rientra in `pending` (§6.2, regola 4);
  - spostare in `ActiveMQ.DLQ` i messaggi che ricevono un ack POISON. Il messaggio conserva l'ID originale e riceve la proprietà `dlqDeliveryFailureCause`.
- Messaggi scaduti: vedi §17.

### 6.7 Transazioni locali

- `TransactionInfo` con `LocalTransactionId` (tipo 111): la transazione appartiene alla connessione.
- I messaggi inviati in transazione restano in un buffer della transazione e **non sono visibili** finché non arriva COMMIT. Al commit entrano nelle code in ordine di invio e ricevono lì il loro `broker_seq`.
- Gli ack in transazione vengono registrati e applicati solo al commit.
- Al ROLLBACK i messaggi inviati vengono scartati. I messaggi consumati tornano in `pending` nella loro posizione originale, con `redeliveryCounter + 1`.
- Alla caduta della connessione le transazioni aperte vanno in rollback.
- Un `XATransactionId` (tipo 112) viene rifiutato con `ExceptionResponse` (`javax.jms.JMSException: XA transactions not supported`).

### 6.8 Topic (non durevoli)

- Ogni consumer collegato a un topic ha una propria lista di messaggi in attesa. Un messaggio pubblicato viene copiato logicamente in ogni lista; il corpo è condiviso tramite `Arc`.
- Un messaggio pubblicato senza consumer collegati viene scartato, come nella semantica JMS.
- Per ogni sottoscrizione valgono FIFO, prefetch e ack come per le code.
- Consumer lento: se la lista in attesa di un consumer supera `topic_max_pending_per_consumer` (default 10000), i messaggi più vecchi vengono scartati e conteggiati. È la strategia "evict oldest" di ActiveMQ e serve a non far crescere la memoria senza limite.

### 6.9 Errori verso il client

| Situazione | Risposta |
|---|---|
| Credenziali errate | `ExceptionResponse` con `java.lang.SecurityException` ("User name [x] or password is invalid."), poi chiusura. Il client solleva `JMSSecurityException`. |
| Selettore sintatticamente errato o XPath | `ExceptionResponse` con `javax.jms.InvalidSelectorException`, con posizione e motivo dell'errore (§16.5) |
| Destinazione wildcard o composita | `ExceptionResponse` con `javax.jms.InvalidDestinationException` |
| Consumer su destinazione temporanea di un'altra connessione | `ExceptionResponse` con `javax.jms.InvalidDestinationException` |
| Limite di memoria superato (§6.10) | invio sincrono: `ExceptionResponse` con `javax.jms.ResourceAllocationException`; invio asincrono: messaggio scartato, warning nel log |
| Transazione XA, sottoscrizione durevole | `ExceptionResponse` con `javax.jms.JMSException` e messaggio esplicito |

### 6.10 Memoria

- **R5:** nessun limite al numero di connessioni, sessioni, producer o consumer.
- La dimensione dei messaggi in memoria è contabilizzata: body più proprietà più un overhead stimato.
- `max_memory_mb` è opzionale (default **nessun limite**). Se impostato e superato, gli invii vengono rifiutati come in §6.9 finché la memoria non scende sotto il 90% del limite.

---

## 7. Autenticazione

- Le credenziali arrivano in `ConnectionInfo.userName` e `ConnectionInfo.password`.
- Si confrontano con la sezione `[[users]]` della configurazione, oppure con l'utente di default `admin`/`admin` se non c'è un file di configurazione (§9.2). La password può essere:
  - `password = "..."` in chiaro, sconsigliato, con un warning al primo avvio;
  - `password_hash = "$argon2id$..."`, generato con `mqrust.exe hash-password`.
- Il confronto delle password in chiaro avviene a tempo costante.
- Sono vietate le connessioni anonime (username vuoto), a meno che `allow_anonymous = true` (default `false`).
- L'utente admin è **separato** dagli utenti OpenWire: le credenziali admin non danno accesso al broker e viceversa, salvo che l'utente le configuri uguali.
- Ogni login fallito, sia OpenWire sia admin, viene registrato con IP remoto e username, mai con la password.

---

## 8. Console di amministrazione (R6)

### 8.1 Accesso

- Server HTTP su `admin.bind:admin.port`. Default `127.0.0.1:8161`, cioè solo locale; per esporla in rete va configurata esplicitamente.
- Autenticazione **HTTP Basic** con le credenziali `[admin]`. Il browser mostra il proprio prompt di login, senza gestione di sessioni.
- HTTPS escluso dalla prima versione: per l'accesso remoto si consiglia un reverse proxy (§12).
- La console è in sola lettura.

### 8.2 Pagine HTML

Le pagine sono generate lato server, CSS minimale incluso nel binario, nessun framework JavaScript. Si aggiornano a mano oppure con un auto-refresh opzionale ogni 5 s.

| Pagina | Contenuto |
|---|---|
| `/` Panoramica | `ActiveMQRust <versione>`, uptime, indirizzi di ascolto, connessioni attive, numero di code e topic, memoria messaggi usata e limite, RSS del processo |
| `/queues` Code | tabella con: nome, messaggi in attesa, messaggi inflight, **consumer**, **producer**, totale accodati, totale consumati, scaduti; ordinabile per colonna |
| `/queues/{nome}` Dettaglio coda | contatori, elenco dei consumer (connessione, IP client, prefetch, inflight) e dei producer, poi il **contenuto della coda** paginato (50 per pagina, ordine FIFO) |
| `/queues/{nome}/messages/{id}` Messaggio | header JMS (MessageID, CorrelationID, Type, ReplyTo, DeliveryMode, Priority, Timestamp, Expiration, RedeliveryCounter), proprietà applicative, body |
| `/topics` Topic | nome, consumer, producer, messaggi pubblicati, scartati |
| `/connections` Connessioni | ConnectionId, utente, IP remoto, versione OpenWire, data di connessione, sessioni, consumer, producer |

**Visualizzazione del body:**

| Tipo di messaggio | Rendering |
|---|---|
| TextMessage | testo, troncato a 64 KB con indicazione |
| BytesMessage | dump esadecimale dei primi 4 KB |
| MapMessage | tabella chiave / tipo / valore |
| ObjectMessage | solo dimensione e "oggetto Java serializzato" (il broker non deserializza oggetti Java) |
| StreamMessage | elenco dei valori decodificati |
| body compresso (`compressed=true`) | decompresso con zlib prima del rendering |

### 8.3 API JSON

Stessi dati delle pagine, sotto `/api/…` (`/api/overview`, `/api/queues`, `/api/queues/{nome}`, `/api/queues/{nome}/messages?offset=&limit=`, `/api/topics`, `/api/connections`), con la stessa autenticazione Basic. Serve per monitoraggio e script.

### 8.4 Coerenza dei dati

L'admin legge **snapshot** prese sotto il mutex della destinazione e rilasciate subito: l'admin non blocca mai a lungo il traffico. La pagina del contenuto copia solo la pagina richiesta (al massimo 50 messaggi), non l'intera coda.

---

## 9. Configurazione (R8)

File TOML **facoltativo**. Il broker lo cerca in quest'ordine:
1. il percorso passato con `--config <file>`: se è indicato ma non esiste, è un errore;
2. `mqrust.toml` nella stessa cartella dell'eseguibile;
3. se non lo trova, usa i **default incorporati** (§9.2).

Ogni chiave mancante nel file prende il valore di default, quindi un file con la sola sezione `[[users]]` è valido.

Esempio completo, con i valori di default:

```toml
[broker]
name = "ActiveMQRust"
bind = "0.0.0.0"          # IP di ascolto OpenWire
port = 61616              # porta OpenWire
max_frame_size_mb = 100
max_memory_mb = 0         # 0 = nessun limite
compress_threshold_kb = 32    # il broker comprime i body più grandi; 0 = mai (§15.2)
compress_min_saving_pct = 10  # tiene la versione compressa solo se risparmia almeno il 10%
topic_max_pending_per_consumer = 10000
allow_anonymous = false

[expiry]                  # §17
check_interval_ms = 1000  # frequenza dello sweeper delle scadenze
use_broker_clock = false  # true = ricalcola la scadenza sull'orologio del broker
ttl_ceiling_ms = 0        # 0 = nessun tetto al TTL
default_ttl_ms = 0        # TTL applicato ai messaggi senza scadenza; 0 = nessuno

[admin]
bind = "127.0.0.1"        # IP di ascolto della console
port = 8161
username = "admin"
password_hash = "$argon2id$v=19$m=19456,t=2,p=1$..."

[log]
level = "info"            # error | warn | info | debug | trace

[[users]]
username = "app1"
password_hash = "$argon2id$v=19$..."

[[users]]
username = "app2"
password = "segreta"      # in chiaro: consentito ma sconsigliato
```

Validazione all'avvio (solo quando c'è un file):
- porte valide, IP parsabili, almeno un utente (o `allow_anonymous`), admin con password;
- username duplicati vietati;
- per ogni campo presente è valorizzato `password` oppure `password_hash`, non entrambi.

Un errore di configurazione stampa un messaggio chiaro con il campo errato ed esce con codice 2.

### 9.1 Riga di comando

```
mqrust.exe                             avvia il broker (porta 61616 di default)
mqrust.exe --config <file>             avvia con un file di configurazione specifico
mqrust.exe --bind <ip> --port <n>      sovrascrive bind/porta OpenWire del file
mqrust.exe --admin-bind <ip> --admin-port <n>   sovrascrive bind/porta dell'admin
mqrust.exe hash-password               chiede la password (input nascosto) e stampa l'hash Argon2
mqrust.exe check-config [--config f]   valida la configurazione ed esce
mqrust.exe init-config                 scrive mqrust.toml commentato accanto all'exe (non sovrascrive)
mqrust.exe --version                   stampa "ActiveMQRust 0.1.0"
```

Priorità dei valori: riga di comando > file di configurazione > default incorporati.

### 9.2 Default incorporati

| Chiave | Default |
|---|---|
| OpenWire | `0.0.0.0:61616` |
| Admin | `127.0.0.1:8161` |
| Utente OpenWire | `admin` / `admin` |
| Utente admin | `admin` / `admin` |
| `broker.name` | `ActiveMQRust` |
| Altre chiavi | come nell'esempio sopra |

Quando sono in uso le credenziali di default, a ogni avvio il broker scrive un warning: `credenziali di default admin/admin in uso: configurare [[users]] e [admin] in mqrust.toml`.

---

## 10. Log (R7)

Log essenziale su stdout con `tracing`, una riga per evento, timestamp locale.

All'avvio, livello info:
```
2026-10-05 16:30:00 INFO ActiveMQRust 0.1.0 avvio
2026-10-05 16:30:00 INFO config: C:\mqrust\mqrust.toml (2 utenti)
                                  (oppure: "config: nessun file, uso i default incorporati" + warning credenziali)
2026-10-05 16:30:00 INFO OpenWire in ascolto su 0.0.0.0:61616 (versione max 12)
2026-10-05 16:30:00 INFO admin in ascolto su http://127.0.0.1:8161
2026-10-05 16:30:00 INFO pronto
```

A runtime, al livello info si registrano solo:
- connessione aperta (IP, utente) e chiusa (motivo);
- login fallito (warning);
- limite di memoria raggiunto o rientrato (warning);
- errori di protocollo (warning).

Creazione delle destinazioni e singoli messaggi vanno a livello debug. Nessun file di log nella prima versione: stdout si redirige se serve.

Arresto con Ctrl+C: il broker smette di accettare connessioni, invia `ShutdownInfo` ai client, attende al massimo 5 s ed esce. Registra quanti messaggi sono stati persi in memoria.

---

## 11. Strategia di test

1. **Test unitari del codec:** round-trip encode/decode per ogni comando e versione supportata.
2. **Vettori golden:** frame reali catturati da un client Java contro ActiveMQ 5.18 / 6.x (Wireshark o un proxy di cattura) e salvati in `tests/data/`. Il decoder deve leggerli e il re-encode deve produrre byte identici (con loose encoding).
3. **Test della semantica del broker** (senza rete): FIFO con 1 e N consumer, redelivery in posizione originale, ack di ogni tipo, prefetch 0 e pull, browser, transazioni commit e rollback, destinazioni temporanee, DLQ, scadenza, duplicati, limite di memoria.
4. **Integrazione con il driver reale** (`tests/java-it/`) — il programma di accettazione obbligatorio è descritto in §18; inoltre progetto Maven minimo con `activemq-client` (una versione 5.18.x e una 6.x) che esegue gli scenari contro `mqrust.exe` avviato dal test:
   - connessione con credenziali corrette e errate (atteso `JMSSecurityException`);
   - invio di 10.000 messaggi e ricezione in ordine identico (verifica FIFO e `JMSMessageID` uguali tra producer e consumer);
   - tutti e 5 i tipi di messaggio con proprietà;
   - request/reply con `TemporaryQueue`;
   - sessione transazionale con commit e rollback;
   - `CLIENT_ACKNOWLEDGE` con recover;
   - `QueueBrowser`;
   - topic con 3 subscriber;
   - kill del consumer con messaggi in volo, verificando che vengano riconsegnati per primi e con `JMSRedelivered=true`;
   - inattività: connessione aperta per oltre 60 s senza traffico che resta viva.
   Richiede un JDK sulla macchina di sviluppo, non a runtime.
5. **Test admin:** risposte 401 senza credenziali, contenuto delle API JSON dopo uno scenario noto.
6. **Compressione:** un body compresso dal client arriva al consumer identico byte per byte. Un body grande inviato senza compressione viene compresso dal broker e letto correttamente dal client Java, per **ognuno dei 5 tipi di messaggio**, con dimensioni al limite (soglia-1, soglia, soglia+1).
7. **Scadenza** (§17.7): sweeper, scadenza al dispatch, ack EXPIRED, cancellazione senza DLQ, orologio del broker, `ttl_ceiling_ms` e `default_ttl_ms`; integrazione Java con `setTimeToLive()` e `JMSExpiration` verificati lato consumer.
8. **Selettori** (§16.6): test unitari di lexer, parser e valutazione, inclusi i casi a tre valori; test di semantica sul dispatch; test di integrazione Java con gli stessi selettori eseguiti anche contro ActiveMQ reale, confrontando i messaggi ricevuti.
9. **Benchmark** (§14.3).
10. **Misura della memoria:** RSS a vuoto e con 100.000 messaggi da 1 KB, confrontato con ActiveMQ nella stessa situazione. Il risultato va registrato nel README.

---

## 12. Possibili evoluzioni (fuori dalla prima versione)

- Installazione come servizio Windows (`mqrust.exe service install`, crate `windows-service`).
- HTTPS per l'admin e SSL/TLS per OpenWire (`ssl://`).
- Azioni admin: svuotare o cancellare code, spostare messaggi dalla DLQ.
- Permessi per destinazione.
- Wildcard e sottoscrizioni durevoli (in memoria).

---

## 13. Rischi

| Rischio | Mitigazione |
|---|---|
| Differenze di campo tra versioni OpenWire non documentate | Si ricavano dai marshaller Java e si verificano con i vettori golden e i test di integrazione su due versioni del client. |
| Comportamenti impliciti del client (advisory, ack ottimizzati, `optimizeAcknowledge`, `useAsyncSend`) | La suite di integrazione copre le opzioni più comuni della connection factory; ogni divergenza diventa un test. |
| Il client chiude per inattività | Keep-alive del broker a metà dell'intervallo negoziato (§5.5), coperto da test. |
| Crescita della memoria con consumer lenti o assenti | Contabilità della memoria, limite opzionale (§6.10), limite di attesa per i topic (§6.8). |
| Selettori che non trovano mai corrispondenza fanno accumulare messaggi; scansioni costose su code lunghe | Cursore per consumer (§16.3), messaggi non selezionati visibili nell'admin, limite di memoria (§6.10). |
| Differenze sottili tra il nostro valutatore e quello di ActiveMQ | Test di integrazione che confrontano ActiveMQ reale e ActiveMQRust con gli stessi selettori (§11, punto 8). |

---

## 14. Prestazioni (R13)

### 14.1 Principi

1. **Il body non viene mai decodificato nel percorso caldo.** `content` e `marshalledProperties` restano `bytes::Bytes` opachi, ricavati dal frame senza copie (slice del buffer di lettura). Il broker legge solo gli header che gli servono: destinazione, ID, scadenza, persistenza, flag `compressed`, `redeliveryCounter`. Le proprietà si decodificano solo quando serve valutare un selettore, una volta per messaggio (§16.4).
2. **Un messaggio, una sola allocazione condivisa.** Il messaggio memorizzato è un `Arc<StoredMessage>`. Topic e browser condividono lo stesso body senza copiarlo.
3. **Re-encode solo degli header.** Ogni connessione serializza nella propria versione OpenWire. Il body viene scritto con write vettoriali (`write_vectored`), senza copiarlo nel buffer di uscita.
4. **Batching delle scritture.** Il task di scrittura raccoglie tutto ciò che è già disponibile nel canale prima di fare `flush`. Sotto carico si ha una sola syscall per molti `MessageDispatch`; a basso carico la latenza resta minima. `TCP_NODELAY` è attivo.
5. **Lock brevi e partizionati.** Il registry delle destinazioni è una mappa concorrente partizionata. Ogni coda ha il proprio mutex, tenuto solo per operazioni in memoria O(1) o O(log n). Due code diverse non si contendono mai lo stesso lock.
6. **Niente allocazioni per comando dove si possono evitare:** buffer di lettura riusati, ID come `Arc<str>` condivisi tra i comandi della stessa connessione.
7. **Risposte sincrone immediate.** Per un invio sincrono la `Response` parte appena il messaggio è in coda: senza persistenza non c'è nessun fsync da attendere.
8. **Profilo di release:** `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `opt-level = 3`, allocatore `mimalloc`.

### 14.2 Wire format e prestazioni

La scelta di §3.1 (loose encoding) regge anche dal punto di vista delle prestazioni. La codifica tight risparmia pochi byte per comando, ma richiede due passaggi di marshalling e più CPU; su una LAN il collo di bottiglia è la CPU per messaggio, non la banda. Il codec passa comunque da un trait `WireCodec`, così che la codifica tight si possa aggiungere in seguito come opzione (`broker.tight_encoding`), **solo se** i benchmark ne mostrano un vantaggio reale.

### 14.3 Obiettivi misurabili

Da misurare sulla macchina di sviluppo (broker e client Java sulla stessa macchina, poi su LAN) e messaggi non persistenti da 1 KB:

| Scenario | Obiettivo |
|---|---|
| 1 producer → 1 consumer, invio asincrono | ≥ 100.000 msg/s |
| 1 producer → 1 consumer, invio sincrono | limitato solo dal round-trip di rete |
| 10 producer → 10 consumer su 10 code | scala in modo quasi lineare con i core |
| Latenza p99 a 1.000 msg/s | < 1 ms lato broker |
| Confronto con ActiveMQ 5.18 / 6.x, stesso scenario | throughput ≥ ActiveMQ, memoria ≤ 1/5 |

I benchmark `criterion` coprono decode ed encode di `ActiveMQTextMessage`, enqueue e dispatch, e la compressione. Un harness Java in `tests/java-it` misura il throughput end-to-end.

---

## 15. Compressione

### 15.1 Compressione OpenWire lato client (R14)

In OpenWire la compressione è **per messaggio**, non per connessione. Con `useCompression=true` sulla connection factory, il client comprime con deflate (`java.util.zip.Deflater`) il `content` del messaggio e imposta `compressed=true`. Le proprietà (`marshalledProperties`) restano non compresse. Il trasporto `tcp://` non prevede una compressione dell'intero stream.

Il broker:
- **conserva il body compresso così com'è**, senza decomprimerlo né ricomprimerlo, e lo consegna intatto ai consumer. Il client destinatario lo decomprime in modo trasparente in base al flag `compressed`, qualunque sia la sua impostazione `useCompression`;
- tiene in RAM il body compresso, risparmiando memoria;
- registra nelle statistiche la dimensione compressa (quella che occupa memoria) e il fatto che il messaggio è compresso;
- decomprime **solo** nell'admin, per l'anteprima, fermandosi a 64 KB decompressi per difendersi dalle "zip bomb".

### 15.2 Compressione lato broker (R15)

Quando arriva un messaggio **non compresso** con `content` più grande di `compress_threshold_kb` (default **32 KB**), il broker lo comprime prima di metterlo in coda:

1. Usa deflate in formato **zlib** (quello che `java.util.zip.Inflater` legge di default), **livello 1** (il più veloce), con il backend `zlib-rs`.
2. Se la compressione non fa risparmiare almeno `compress_min_saving_pct` (default 10%), come succede per dati già compressi (JPEG, ZIP), il broker tiene l'originale. La decisione si prende una volta sola, all'ingresso.
3. Se comprime, sostituisce `content` e imposta `compressed=true`. `MessageId` e tutti gli header restano invariati. Per il client destinatario il messaggio è identico a uno inviato con `useCompression=true`.
4. Per body oltre 1 MB la compressione gira in `spawn_blocking`, per non bloccare il runtime async. **L'ordine FIFO non cambia**: il reader della connessione attende la fine della compressione prima di processare il frame successivo, e il messaggio riceve il suo `broker_seq` quando entra nella coda.
5. Vantaggi: meno RAM occupata dalla coda e meno banda verso i consumer. Il costo in CPU all'ingresso è contenuto dal livello 1 e dalla soglia.
6. `compress_threshold_kb = 0` disattiva la compressione lato broker.

**Formato per tipo di messaggio (da verificare sui sorgenti Java prima dell'implementazione):** il client si aspetta per ogni tipo un formato preciso del `content` compresso. Per esempio, `ActiveMQBytesMessage` mette la lunghezza originale prima dei dati deflate, mentre gli altri tipi usano un `DeflaterOutputStream` sul contenuto serializzato. L'implementazione deve replicare **esattamente** `storeContent()` di ogni classe `ActiveMQ*Message` delle versioni 5.18 / 6.x. La compatibilità è verificata dai test di integrazione su tutti e 5 i tipi (§11, punto 6). Un tipo per cui la verifica fallisce viene escluso dalla compressione lato broker.

---

## 16. Selettori JMS (R16)

### 16.1 Ambito

- Si usano su `ConsumerInfo.selector` per consumer di code e di topic, e per `QueueBrowser`.
- La sintassi è il sottoinsieme SQL-92 definito dalla specifica JMS 1.1 / 2.0, con la stessa semantica di ActiveMQ.
- Il selettore viene compilato **una sola volta** alla creazione del consumer, in un AST immutabile condiviso (`Arc<Selector>`). Un selettore vuoto o di soli spazi equivale a nessun selettore.

### 16.2 Grammatica e semantica supportate

| Elemento | Supporto |
|---|---|
| Letterali | stringhe `'...'` (con `''` per l'apice), interi (anche con suffisso `L`, esadecimali `0x`, ottali), decimali e notazione esponenziale, `TRUE` / `FALSE` |
| Identificatori | proprietà applicative (case-sensitive); parole chiave case-insensitive |
| Logici | `AND`, `OR`, `NOT`, con logica a **tre valori** (TRUE / FALSE / UNKNOWN) come da specifica JMS |
| Confronto | `=`, `<>`, `<`, `<=`, `>`, `>=`. Stringhe e booleani solo con `=` e `<>` |
| Aritmetica | `+`, `-`, `*`, `/`, `-` unario, con promozione numerica (intero → long → double) |
| Altri | `[NOT] BETWEEN a AND b`, `[NOT] IN ('a','b',…)`, `[NOT] LIKE 'pat%_' [ESCAPE 'c']`, `IS [NOT] NULL` |
| Parentesi | sì, con le precedenze della specifica JMS |

Valori degli identificatori:

| Identificatore | Valore |
|---|---|
| `JMSDeliveryMode` | `'PERSISTENT'` / `'NON_PERSISTENT'` |
| `JMSPriority` | intero 0–9 |
| `JMSMessageID` | rappresentazione testuale del `MessageId` (§6.4) |
| `JMSTimestamp` | long (ms) |
| `JMSCorrelationID`, `JMSType` | stringa o NULL |
| `JMSXGroupID`, `JMSXGroupSeq` | dai campi `groupID` / `groupSequence` del messaggio |
| `JMSXDeliveryCount` | `redeliveryCounter + 1` |
| altre proprietà | da `marshalledProperties`; una proprietà assente vale NULL |

La lista degli identificatori `JMS*` riconosciuti e i relativi valori vanno verificati sui sorgenti di ActiveMQ (`org.apache.activemq.filter.PropertyExpression`) per allinearsi esattamente.

Regole di tipo (come ActiveMQ):
- un confronto tra tipi incompatibili, per esempio stringa con numero, vale UNKNOWN e quindi il messaggio **non** viene selezionato;
- l'aritmetica con NULL dà NULL;
- un messaggio è selezionato solo se il selettore vale **TRUE**.

### 16.3 Dispatch con selettori e FIFO

- **Code.** Ogni consumer riceve il primo messaggio di `pending`, in ordine `broker_seq`, che soddisfa il suo selettore. Un messaggio che non corrisponde a nessun consumer resta in coda e **non blocca** i successivi: è il caso tipico di consumer diversi su sottoinsiemi diversi della stessa coda.
- **Cursore per consumer.** Per evitare di riscansionare ogni volta la testa della coda, ogni consumer con selettore tiene l'ultimo `broker_seq` esaminato. I nuovi messaggi arrivano sempre in coda; un messaggio che rientra in posizione originale (§6.2, regola 4) riporta indietro i cursori. Il costo ammortizzato è O(1) per messaggio e consumer, invece di O(n) a ogni dispatch. Il broker **non ha un limite di pagina** come il `maxPageSize` di ActiveMQ, quindi non soffre del noto blocco dei consumer selettivi su code lunghe.
- **Ordine di rotazione.** Tra più consumer che accettano lo stesso messaggio vale la rotazione di §6.2. Il messaggio va al primo consumer, in ordine di rotazione, con prefetch libero e selettore soddisfatto.
- **Topic.** Il filtro si applica al momento della pubblicazione: il messaggio entra solo nelle liste dei consumer il cui selettore lo accetta.
- **QueueBrowser con selettore:** il broker restituisce solo i messaggi che corrispondono, in ordine FIFO.
- **Admin:** nel dettaglio di una coda, per ogni consumer viene mostrato il suo selettore.

### 16.4 Prestazioni

- Le `marshalledProperties` (mappa primitiva OpenWire) si decodificano in modo **pigro**: solo la prima volta che un selettore deve valutare il messaggio. Il risultato resta in cache nel messaggio (`OnceLock<Arc<PropertyMap>>`). I consumer senza selettore non pagano mai questo costo.
- Gli header JMS si leggono dai campi già decodificati, senza toccare le proprietà. Un selettore che usa solo header non decodifica mai le proprietà.
- `LIKE` viene compilato in un matcher dedicato alla compilazione del selettore (prefisso, suffisso o contenuto, oppure automa generico). `IN` con molti elementi usa un `HashSet`.
- La compressione (§15) riguarda solo il `content`: le proprietà non sono mai compresse, quindi i selettori non richiedono decompressione.

Formato della mappa primitiva da decodificare: numero di voci, poi per ogni voce chiave (string) e valore tipizzato. Codici: NULL=0, BOOLEAN=1, BYTE=2, CHAR=3, SHORT=4, INTEGER=5, LONG=6, DOUBLE=7, FLOAT=8, STRING=9, BYTE_ARRAY=10, MAP=11, LIST=12, BIG_STRING=13, da verificare su `MarshallingSupport`.

### 16.5 Errori

- Errore di sintassi: `ExceptionResponse` con `javax.jms.InvalidSelectorException` e messaggio del tipo `"Unexpected token 'AN' at column 14 in selector: color = 'red' AN size > 2"`. Il client solleva l'eccezione in `createConsumer()`.
- Selettore XPath/XQuery (`XPATH '…'`, `XQUERY '…'`): `InvalidSelectorException("XPath selectors are not supported")`.
- Un errore di valutazione a runtime, come la divisione per zero, rende UNKNOWN quel messaggio per quel consumer, senza alcun errore verso il client.

### 16.6 Test specifici

- Test unitari del parser su tutta la grammatica, sulle precedenze e sugli errori con posizione.
- Tabella di verità a tre valori per AND, OR e NOT.
- `LIKE` con `%`, `_` ed `ESCAPE`; `BETWEEN` e `IN` con valori NULL.
- Dispatch: due consumer con selettori disgiunti sulla stessa coda ricevono ciascuno la propria sottosequenza FIFO completa. Un messaggio non selezionato da nessuno resta in coda senza bloccare.
- Integrazione Java: lo stesso insieme di selettori e messaggi eseguito contro ActiveMQ reale e contro ActiveMQRust deve produrre gli stessi messaggi ricevuti, nello stesso ordine.
- Benchmark: dispatch con 10 consumer selettivi su una coda di 100.000 messaggi.

---

## 17. Scadenza dei messaggi (R17)

### 17.1 Origine della scadenza

- Il producer imposta il TTL con `MessageProducer.setTimeToLive()` o con `send(..., timeToLive)`. Il client calcola `expiration = timestamp + ttl` con **il proprio orologio** e lo invia nel campo `expiration` del messaggio. `expiration = 0` significa che il messaggio non scade.
- Il broker conserva `expiration` intatto, salvo le opzioni di §17.5. Il consumer lo legge come `JMSExpiration`.

### 17.2 Rimozione attiva (sweeper)

Senza persistenza, un messaggio scaduto che resta in coda occupa RAM inutilmente. Il broker lo rimuove **attivamente**, non solo quando lo incontra al dispatch:

- Ogni coda e ogni lista d'attesa dei topic ha un `expiry_index: BTreeSet<(expiration, broker_seq)>` che contiene **solo** i messaggi con `expiration > 0`. I messaggi senza scadenza non hanno alcun costo aggiuntivo.
- Un unico task periodico (`check_interval_ms`, default 1000 ms) visita le destinazioni che hanno messaggi in scadenza. Per ciascuna estrae dalla testa dell'indice tutti i messaggi con `expiration <= now` e li rimuove da `pending` in O(log n) ciascuno.
- Per non tenere a lungo il mutex di una coda, lo sweeper elabora al massimo 10.000 messaggi per giro e per destinazione, poi rilascia il lock e passa alla successiva. Il resto viene ripreso al giro dopo.
- Il task non fa nulla se nessuna destinazione ha messaggi con scadenza.

### 17.3 Controlli al momento della consegna

Oltre allo sweeper, la scadenza si controlla in ogni punto in cui un messaggio sta per essere consegnato. Così non si consegna mai un messaggio già scaduto, anche tra un giro dello sweeper e l'altro:

| Punto | Comportamento |
|---|---|
| Dispatch a un consumer (code e topic) | se `expiration <= now`, il messaggio non viene consegnato e viene trattato come scaduto; il dispatch passa al successivo |
| `MessagePull` (prefetch 0) | come sopra; se tutto quello che c'è in coda è scaduto e il pull ha un timeout, si applica il timeout normale |
| QueueBrowser | i messaggi scaduti vengono saltati e trattati come scaduti |
| Messaggio in `inflight` | **non** viene tolto al consumer: è già consegnato. Se il client lo riceve scaduto, lo scarta e invia un ack `EXPIRED` (comportamento standard del client ActiveMQ), poi il broker lo tratta come scaduto |
| Reinserimento in `pending` dopo rollback, chiusura del consumer o caduta della connessione | se è già scaduto non rientra in coda e viene trattato come scaduto |
| Admin, contenuto della coda | i messaggi scaduti non ancora rimossi dallo sweeper sono mostrati con l'indicazione "scaduto" |

La scadenza non altera l'ordine FIFO: rimuovere un messaggio non cambia la posizione relativa degli altri.

### 17.4 Cosa succede a un messaggio scaduto

Un messaggio scaduto viene **cancellato definitivamente**: niente DLQ, niente opzioni. È una scelta voluta che si discosta da ActiveMQ, il quale per default sposta in `ActiveMQ.DLQ` i messaggi persistenti scaduti.

- La cancellazione libera subito la memoria contabilizzata (§6.10).
- Il messaggio viene tolto da `pending`, dall'indice delle scadenze e, nel caso di ack `EXPIRED`, da `inflight`.
- La regola vale per tutte le destinazioni, compresa `ActiveMQ.DLQ`: un messaggio finito in DLQ per un ack POISON (§6.6) conserva la propria `expiration` e, se ne ha una, viene cancellato alla scadenza come ogni altro messaggio.
- Il contatore `expired` della destinazione viene incrementato.

### 17.5 Opzioni lato broker

Le opzioni sono equivalenti a `TimeStampingBrokerPlugin` di ActiveMQ e sono tutte disattivate per default:

- **`use_broker_clock = true`:** corregge lo sfasamento tra l'orologio del client e quello del broker. All'arrivo, se `expiration > 0`, il broker ricalcola `expiration = now_broker + (expiration - timestamp)` e imposta `timestamp = now_broker`.
- **`ttl_ceiling_ms > 0`:** limita il TTL massimo, così un producer non può tenere messaggi in RAM oltre questo tempo. Si applica anche ai messaggi senza scadenza se `default_ttl_ms` è impostato.
- **`default_ttl_ms > 0`:** assegna questo TTL ai messaggi con `expiration = 0`. È una protezione della memoria per i messaggi non consumati, utile in un broker solo-RAM.

Quando un'opzione modifica `expiration`, il nuovo valore è quello che il consumer vede come `JMSExpiration`.

### 17.6 Statistiche, admin e log

- Contatore `expired` per ogni destinazione, già presente nella tabella code (§8.2), che conta i messaggi cancellati per scadenza.
- Nel dettaglio della coda: numero di messaggi con scadenza e prossima scadenza.
- Nel dettaglio del messaggio: `Expiration` come data e ora leggibile, più il tempo residuo o "scaduto".
- Log: a livello debug per i singoli messaggi. A livello info, al massimo una riga al minuto per destinazione con il riepilogo ("coda X: 1.234 messaggi scaduti nell'ultimo minuto").

### 17.7 Test specifici

- Messaggio con TTL 100 ms senza consumer: dopo 100 ms + `check_interval_ms` non è più in coda e la memoria contabilizzata scende.
- Coda con messaggi scaduti e validi alternati: il consumer riceve solo i validi, in ordine FIFO.
- Messaggio scaduto durante un rollback: non viene riconsegnato.
- Ack `EXPIRED` dal client: il messaggio esce da `inflight` e viene cancellato.
- Messaggi scaduti persistenti e non persistenti: entrambi cancellati, `ActiveMQ.DLQ` resta vuota.
- Messaggio in DLQ (da ack POISON) con TTL: cancellato alla scadenza.
- `use_broker_clock` con un client simulato in anticipo e in ritardo di 1 ora.
- `ttl_ceiling_ms` e `default_ttl_ms`.
- Topic: un messaggio scaduto nella lista di un subscriber lento viene rimosso.
- Benchmark: 1.000.000 di messaggi con TTL casuale e verifica che lo sweeper non aumenti in modo misurabile la latenza p99 del dispatch.

---

## 18. Programma Java di accettazione (R18)

Il broker lavorerà con applicazioni Java. Il criterio di accettazione principale è quindi un **programma Java che usa il driver ActiveMQ originale**. Il programma va eseguito con successo contro ActiveMQRust e, come riferimento, contro un ActiveMQ reale.

### 18.1 Progetto

- Posizione: `tests/java-it/`, progetto Maven.
- Java **17**, richiesto da `activemq-client` 6.x e compatibile con 5.18.
- Dipendenza: `org.apache.activemq:activemq-client`, con versione selezionabile tramite profilo Maven: `amq5` = 5.18.x con API `javax.jms`, `amq6` = 6.x con API `jakarta.jms`. Il codice di test è lo stesso; cambiano solo gli import, generati dal profilo o tenuti in due sorgenti minimi.
- Si produce un **jar eseguibile con dipendenze** (`maven-shade-plugin`): `mqrust-acceptance.jar`.
- Si esegue su Windows con:
  ```
  java -jar mqrust-acceptance.jar --url tcp://127.0.0.1:61616 --user app1 --password segreta
  ```
  Esiste anche lo script `tests\java-it\run-acceptance.cmd`, che compila con il **Maven Wrapper** incluso (`mvnw.cmd -q package`) ed esegue il jar con gli argomenti passati. Il wrapper evita di dover installare Maven: sulla macchina di sviluppo c'è solo il JDK 21, che compila anche con target 17.
- Uscita: una riga `PASS` o `FAIL` per ogni scenario, con il motivo del fallimento; codice di uscita **0** se tutto passa, **1** altrimenti. Così il programma si può usare anche in CI.
- Ogni scenario usa nomi di coda univoci con suffisso casuale (es. `TEST.FIFO.<uuid8>`). Esecuzioni ripetute non interferiscono e non serve svuotare il broker.
- Ogni `receive` ha un timeout di 5 s: un messaggio che manca è un FAIL, non un blocco.

### 18.2 Scenario 1: coda, 10 messaggi, lettura FIFO

1. `ActiveMQConnectionFactory(url)` con `user` e `password`, poi `createConnection()` e `start()`.
2. Sessione `AUTO_ACKNOWLEDGE`. La coda `TEST.FIFO.<uuid8>` **non esiste prima**: viene creata automaticamente dal broker (R4).
3. Un producer invia **10 `TextMessage`** con testo `msg-1` … `msg-10` e proprietà intera `seq` da 1 a 10. Il programma memorizza il `JMSMessageID` assegnato a ciascuno.
4. Un consumer sulla stessa coda riceve i messaggi con `receive(5000)`.
5. Verifiche:
   - si ricevono **esattamente 10** messaggi, e un ulteriore `receive(1000)` restituisce `null`, quindi la coda è vuota;
   - l'ordine è **identico all'invio** (`msg-1` … `msg-10`, `seq` crescente): verifica del FIFO (R9);
   - ogni `JMSMessageID` ricevuto è **uguale** a quello visto dal producer e ha la forma `ID:<host>-<porta>-<timestamp>-<n>:<n>:<n>:<n>:<n>` (R10);
   - `JMSRedelivered` è `false` per tutti.

### 18.3 Scenario 2: filtro per Correlation ID

1. Coda `TEST.CORR.<uuid8>`, nuova.
2. Un producer invia **12 messaggi** alternando 3 correlation ID, in quest'ordine:
   `ORD-A`, `ORD-B`, `ORD-C`, `ORD-A`, `ORD-B`, `ORD-C`, … (4 messaggi per ID). Il testo è `<correlationId>-<n>`, ad esempio `ORD-A-1`.
3. **Consumer filtrato** con selettore `JMSCorrelationID IN ('ORD-A','ORD-C')`.
4. Verifiche sul consumer filtrato:
   - riceve **esattamente 8** messaggi, tutti con correlation ID `ORD-A` o `ORD-C`, e nessun `ORD-B`;
   - li riceve **nell'ordine di invio relativo**: `ORD-A-1`, `ORD-C-1`, `ORD-A-2`, `ORD-C-2`, … (FIFO con selettore, §16.3);
   - un ulteriore `receive(1000)` restituisce `null`, anche se in coda restano messaggi `ORD-B`: i messaggi non corrispondenti non bloccano il consumer.
5. Il consumer filtrato viene chiuso. Un **consumer senza selettore** sulla stessa coda riceve **esattamente i 4 messaggi `ORD-B`**, in ordine (`ORD-B-1` … `ORD-B-4`). Così si verifica che i messaggi filtrati non vengono persi né consumati per errore.
6. Variante con `LIKE`: su una nuova coda si inviano `ORD-A-100`, `ORD-B-200`, `ORD-A-300` come correlation ID. Un consumer con selettore `JMSCorrelationID LIKE 'ORD-A-%'` deve ricevere solo `ORD-A-100` e `ORD-A-300`, in quest'ordine.
7. Selettore non valido: `createConsumer(queue, "JMSCorrelationID = = 'X'")` deve sollevare `InvalidSelectorException` (§16.5).

### 18.4 Scenario 3: autenticazione

- Una connessione con password errata deve fallire con `JMSSecurityException` su `createConnection()` o `start()`.
- Una connessione con le credenziali corrette continua a funzionare: è implicito negli scenari 1 e 2.

### 18.5 Esecuzione di riferimento contro ActiveMQ reale

Lo stesso jar, lanciato contro un ActiveMQ 5.18 / 6.x configurato con lo stesso utente, deve dare **PASS su tutti gli scenari**. Questo dimostra che il test descrive il comportamento di ActiveMQ e non una peculiarità di ActiveMQRust. Il README documenta come avviare un ActiveMQ locale per questo confronto.

### 18.6 Collocazione nel piano

Il programma di accettazione si scrive **prima** del broker, insieme allo scheletro del progetto, e si valida contro ActiveMQ reale. Diventa così il riferimento eseguibile per ogni fase dell'implementazione:
- lo scenario 3 e la connessione dello scenario 1 sono il primo traguardo (negoziazione e autenticazione);
- lo scenario 1 completo è il secondo traguardo (invio, dispatch, ack, FIFO, ID);
- lo scenario 2 è il traguardo dei selettori.

---

## 19. Build e distribuzione: un solo exe Windows (R11, R19, R20)

### 19.1 Risultato

- Il prodotto distribuito è **un solo file**: `mqrust.exe`, Windows x64, obiettivo di dimensione < 10 MB.
- Si copia in una cartella qualunque e si lancia: il broker risponde subito su **`0.0.0.0:61616`**, con l'admin su `127.0.0.1:8161`. Non servono installer, registro di sistema, variabili d'ambiente o file di configurazione (§9.2).
- Sistemi supportati: Windows 10 / 11 e Windows Server 2016 o successivi, x64. È il minimo del target Rust `x86_64-pc-windows-msvc`.

### 19.2 Nessuna dipendenza a runtime

| Possibile dipendenza | Come si evita |
|---|---|
| Runtime Visual C++ (`vcruntime140.dll`, `msvcp140.dll`) | runtime C collegato staticamente: `.cargo/config.toml` con `rustflags = ["-C", "target-feature=+crt-static"]` per `x86_64-pc-windows-msvc` |
| OpenSSL o altre librerie native | nessun TLS nella prima versione; tutti i crate sono Rust puro, oppure codice C compilato staticamente (`mimalloc`) |
| File HTML, CSS e template dell'admin | incorporati nel binario (`include_str!` / template compilati) |
| File di configurazione | facoltativo (§9) |
| .NET, Java, servizi esterni | nessuno; Java serve solo sulla macchina di sviluppo per il programma di accettazione (§18) |

L'eseguibile importa **solo DLL di sistema** sempre presenti in Windows: `kernel32`, `ntdll`, `ws2_32`, `advapi32`, `bcrypt`/`bcryptprimitives`, `userenv` e simili.

### 19.3 Build

- Toolchain: Rust stable con target `x86_64-pc-windows-msvc`. Visual Studio Build Tools (MSVC + Windows SDK) servono **solo per compilare**.
- Comando: `cargo build --release`. Profilo di release come in §14.1, punto 8.
- L'exe contiene una **risorsa di versione Windows**, visibile in Proprietà → Dettagli: `ProductName = ActiveMQRust`, `FileVersion` e `ProductVersion` uguali alla versione del crate (R12). La risorsa si genera in fase di build con `embed-resource` o `winresource`.
- Applicazione console (subsystem console): log su stdout. Ctrl+C e chiusura della finestra (`CTRL_CLOSE_EVENT`) producono lo stesso arresto ordinato di §10.

### 19.4 Verifica dell'assenza di dipendenze

Fa parte dei criteri di accettazione:
1. `dumpbin /dependents target\release\mqrust.exe` deve elencare solo DLL di sistema della lista consentita. Lo script `scripts\check-deps.cmd` fallisce se compare altro, per esempio `VCRUNTIME140.dll`.
2. Test su macchina pulita: `mqrust.exe` copiato **da solo** in una **Windows Sandbox** (o una VM pulita, senza Visual C++ Redistributable), avviato senza argomenti. Il programma di accettazione Java (§18), eseguito dall'host verso la porta 61616 della sandbox, deve dare PASS su tutti gli scenari con le credenziali `admin`/`admin`.

### 19.5 Note operative

- Al primo avvio in ascolto su `0.0.0.0`, Windows Defender Firewall può chiedere di consentire l'accesso di rete a `mqrust.exe`. È un comportamento del sistema, documentato nel README.
- Per eseguirlo come servizio nella prima versione si possono usare strumenti esterni (`sc.exe` con un wrapper, NSSM). Il supporto nativo come servizio Windows è un'evoluzione (§12).
