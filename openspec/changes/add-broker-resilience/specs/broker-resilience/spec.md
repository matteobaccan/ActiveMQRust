## ADDED Requirements

### Requirement: Failures confined to one connection
A malformed, truncated or hostile OpenWire frame, or any error while handling one connection, SHALL close only that connection with a logged reason; the broker and every other connection SHALL keep working.

#### Scenario: Fuzzed frames
- **WHEN** the fuzzer feeds random and mutated frames to the decoder for the configured time
- **THEN** no input makes the broker process exit

### Requirement: Automatic restart of the Windows service
`mqrust.exe service install` SHALL configure the service recovery actions: restart after 5 s on the first failure, after 30 s on the second, after 2 minutes on later failures, reset of the failure count after 24 hours, with non-crash failures (non-zero exit) counted; `service status` SHALL show them.

#### Scenario: Process killed
- **WHEN** the installed service's process is terminated abnormally
- **THEN** Windows starts it again after about 5 seconds and the broker log records the restart after an unexpected stop
