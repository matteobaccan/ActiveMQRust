## RENAMED Requirements

- FROM: `### Requirement: Read-only console`
- TO: `### Requirement: Console write operations`

## MODIFIED Requirements

### Requirement: Console write operations
The console SHALL NOT change broker state, except through the removal operations of the `admin-message-removal` capability (deleting a pending message, purging a queue) when `admin.read_only` is `false`. Only `GET` and `HEAD` SHALL be accepted, except `POST /login` and `POST /logout`, which change only the console session, and the `POST`/`DELETE` paths of the removal operations; any other method or path SHALL receive `405 Method Not Allowed`. Viewing queue contents, a message in any view, or a confirmation page SHALL NOT consume, acknowledge, reorder, remove or redeliver messages, and SHALL NOT change any counter.

#### Scenario: Write method rejected
- **WHEN** an authenticated client sends `POST /queues/TEST.A` or `DELETE /api/queues/TEST.A`
- **THEN** the response is `405` and the queue is unchanged

#### Scenario: Browsing does not consume
- **WHEN** a queue holds 10 messages and an authenticated client views its contents page, every message detail page, raw and formatted, and the purge confirmation page
- **THEN** a consumer created afterwards receives all 10 messages in FIFO order with `JMSRedelivered=false`, and the queue counters are unchanged by the views
