## ADDED Requirements

### Requirement: TCP listener and framing
The broker SHALL accept OpenWire connections on the configured TCP address. Every frame SHALL consist of a 4-byte big-endian length, then a 1-byte data type, then the command body. The size prefix SHALL always be used (`SizePrefixDisabled=false`). A frame larger than `max_frame_size_mb` SHALL close the connection and log a warning.

#### Scenario: Oversized frame
- **WHEN** a client sends a frame whose length prefix exceeds the configured maximum frame size
- **THEN** the broker closes that connection, logs a warning, and other connections are unaffected

### Requirement: Wire format negotiation
After the client's `WireFormatInfo` (type 1), the broker SHALL send its own `WireFormatInfo` with magic `ActiveMQ`, version 12, `TightEncodingEnabled=false`, `CacheEnabled=false`, `SizePrefixDisabled=false`, `StackTraceEnabled=false`, `TcpNoDelayEnabled=true`, the client's `MaxInactivityDuration` and `MaxInactivityDurationInitalDelay`, and the configured `MaxFrameSize`. The effective version SHALL be `min(client version, 12)`, and all later commands SHALL use loose encoding without marshalling cache.

#### Scenario: Java driver connects
- **WHEN** the ActiveMQ Java driver 5.18.x or 6.x connects with default settings
- **THEN** negotiation completes with loose encoding, and `connection.start()` succeeds

#### Scenario: Version negotiation
- **WHEN** a client announces version 9
- **THEN** the broker encodes and decodes every later command for version 9

### Requirement: Supported protocol versions
The broker SHALL support OpenWire versions 9 to 12, the versions whose marshallers ActiveMQ 5.18 and 6.x still ship (`openwire.v9` … `v12`). A client announcing a version below 9 SHALL be refused: the broker logs the reason and closes the connection.

#### Scenario: Version too old
- **WHEN** a client announces version 8
- **THEN** the broker logs a warning with the version and closes the connection

### Requirement: Broker identity
The broker's `WireFormatInfo` SHALL include `ProviderName="ActiveMQRust"`, `ProviderVersion` equal to the crate version and `PlatformDetails` describing Rust, Windows and x64. After negotiation the broker SHALL send `BrokerInfo` (type 2) with a `BrokerId`, `brokerName` from `broker.name` (default `ActiveMQRust`), and `brokerURL`.

#### Scenario: Provider name visible to the client
- **WHEN** a Java client reads the negotiated remote `WireFormatInfo`
- **THEN** `getProviderName()` returns `ActiveMQRust` and `getProviderVersion()` returns the crate version

#### Scenario: Broker name
- **WHEN** a Java client calls `ActiveMQConnection.getBrokerName()` after connecting with default configuration
- **THEN** it returns `ActiveMQRust`

### Requirement: Command and response rules
Every command with `responseRequired=true` SHALL receive exactly one `Response` (type 30) or `ExceptionResponse` (type 31) whose `correlationId` equals the command's `commandId`. An unknown or unsupported command with `responseRequired=true` SHALL receive an `ExceptionResponse` carrying `java.lang.UnsupportedOperationException`. One without `responseRequired` SHALL be logged at debug level and ignored.

#### Scenario: Correlated response
- **WHEN** a client sends `SessionInfo` with `commandId=5` and `responseRequired=true`
- **THEN** the broker replies with a `Response` whose `correlationId` is 5

#### Scenario: Unsupported command
- **WHEN** a client sends an unsupported command with `responseRequired=true`
- **THEN** the broker replies with an `ExceptionResponse` carrying `java.lang.UnsupportedOperationException`, and the connection stays open

### Requirement: Keep-alive and inactivity
The broker SHALL send `KeepAliveInfo` (type 10) when it has written nothing for half the negotiated `MaxInactivityDuration`. It SHALL close a connection from which it has read nothing for the full negotiated duration. A negotiated duration of 0 SHALL disable both checks.

#### Scenario: Idle connection stays alive
- **WHEN** a Java client is connected and idle for more than 60 seconds with default settings
- **THEN** the connection remains open and usable

#### Scenario: Dead client is dropped
- **WHEN** a client stops sending anything, including keep-alives, for longer than the negotiated inactivity duration
- **THEN** the broker closes the connection and logs the reason

### Requirement: Connection and session lifecycle
The broker SHALL handle `ConnectionInfo` (after authentication), `SessionInfo`, `RemoveInfo` for connections and sessions, and `ShutdownInfo`. Closing a connection, whether by request or by network failure, SHALL release every resource the connection owns.

#### Scenario: Orderly close
- **WHEN** a Java client calls `connection.close()`
- **THEN** the broker receives `RemoveInfo` and `ShutdownInfo`, releases the connection's resources and logs the close

#### Scenario: Network drop
- **WHEN** the client's TCP connection drops without `ShutdownInfo`
- **THEN** the broker releases the connection's resources and logs the close with its reason

### Requirement: Advisory topics
The broker SHALL accept consumers on `ActiveMQ.Advisory.*` topics, including the composite `ActiveMQ.Advisory.TempQueue,ActiveMQ.Advisory.TempTopic` that the Java driver creates by default, and reply with `Response`. Consumers on `ActiveMQ.Advisory.TempQueue` and `ActiveMQ.Advisory.TempTopic` SHALL receive, as ActiveMQ does, a message carrying a `DestinationInfo` (operation ADD) for every temporary destination of that kind that exists when they subscribe, then one `DestinationInfo` (ADD or REMOVE) whenever a temporary destination of that kind is created or deleted. The driver uses these messages to know which temporary destinations still exist. No other advisory message SHALL be published, and advisory topics SHALL NOT be listed as user destinations.

#### Scenario: Default driver advisory consumer
- **WHEN** a Java client with `watchTopicAdvisories=true` (default) connects
- **THEN** its advisory consumer is accepted, and the connection works normally

#### Scenario: Temporary destination advisories
- **WHEN** a client has a consumer on `ActiveMQ.Advisory.TempQueue` and another connection creates and later deletes a temporary queue
- **THEN** the consumer receives a `DestinationInfo` advisory with operation ADD for that queue, then one with operation REMOVE

#### Scenario: No other advisories
- **WHEN** a client has a consumer on another advisory topic, such as `ActiveMQ.Advisory.Connection`
- **THEN** the consumer is accepted and receives no messages
