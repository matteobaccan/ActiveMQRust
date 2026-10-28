## ADDED Requirements

### Requirement: Failures confined to one connection
A malformed, truncated or hostile OpenWire frame, or any error while handling one connection, SHALL close only that connection with a logged reason; the broker and every other connection SHALL keep working.

#### Scenario: Fuzzed frames
- **WHEN** the fuzzer feeds random and mutated frames to the decoder for the configured time
- **THEN** no input makes the broker process exit

#### Scenario: Fuzzed selectors
- **WHEN** the fuzzer feeds random and mutated text to the selector parser for the configured time
- **THEN** no input makes the broker process exit, and the text it refuses is answered with an `InvalidSelectorException` that leaves the connection open

### Requirement: Nesting is bounded
Input that nests deeper than the broker accepts SHALL be refused rather than followed, because every level of nesting costs a level of recursion in the parser that reads it, in the code that walks what it built and in the drop of that structure, and a stack overflow ends the process rather than the connection. The limits SHALL be 32 levels for the data structures of a frame and for nested primitive maps, and 64 levels for a selector.

#### Scenario: A frame nested without end
- **WHEN** a client sends a frame whose data structures nest more than 32 levels deep
- **THEN** the broker refuses the frame, logs the reason with the client address, closes only that connection, and keeps serving every other one

#### Scenario: A selector nested without end
- **WHEN** a consumer subscribes with a selector that nests more than 64 levels deep
- **THEN** the broker refuses the subscription with an `InvalidSelectorException` naming the limit, and the connection stays open

### Requirement: Automatic restart of the Windows service
`mqrust.exe service install` SHALL configure the service recovery actions: restart after 5 s on the first failure, after 30 s on the second, after 2 minutes on later failures, reset of the failure count after 24 hours, with non-crash failures (non-zero exit) counted; `service status` SHALL show them.

#### Scenario: Process killed
- **WHEN** the installed service's process is terminated abnormally
- **THEN** Windows starts it again after about 5 seconds and the broker log records the restart after an unexpected stop
