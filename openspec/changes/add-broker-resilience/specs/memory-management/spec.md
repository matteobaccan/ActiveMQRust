## MODIFIED Requirements

### Requirement: Memory limit configuration
The `[broker]` section SHALL accept the key `max_memory_mb`, a non-negative integer. When the key is absent, the broker SHALL limit the memory its messages may hold to half of the machine's physical memory, never more than 2048 MB and never less than 256 MB; when the physical memory cannot be read it SHALL use 2048 MB and log a warning saying so. `max_memory_mb = <n>` SHALL set the limit to exactly `<n>` MB, and `max_memory_mb = 0` SHALL mean no limit, which is how an operator asks for the behaviour of releases up to 0.4.1. A negative or non-integer value SHALL be a configuration validation error that names `broker.max_memory_mb` and exits with code 2. The limit applies to the accounted size of stored messages, not to the size of the process. The startup log SHALL state the limit and where it came from: the configuration file, the default with the physical memory it was computed from, or that there is no limit.

#### Scenario: Default on a machine with 16 GB
- **WHEN** the broker starts with no `max_memory_mb` on a machine with 16 GB of physical memory
- **THEN** the limit is 2048 MB, and the startup log gives that limit and the memory of the machine it was computed from

#### Scenario: Default on a machine with 1 GB
- **WHEN** the broker starts with no `max_memory_mb` on a machine with 1 GB of physical memory
- **THEN** the limit is 512 MB

#### Scenario: Default on a machine smaller than the floor
- **WHEN** the broker starts with no `max_memory_mb` on a machine with 256 MB of physical memory
- **THEN** the limit is 256 MB, the floor, rather than 128 MB

#### Scenario: Physical memory unknown
- **WHEN** the broker starts with no `max_memory_mb` and the physical memory of the machine cannot be read
- **THEN** the limit is 2048 MB and the log warns that the memory of the machine could not be read

#### Scenario: No limit on request
- **WHEN** the configuration file sets `max_memory_mb = 0`
- **THEN** sends are never refused for memory reasons, and the startup log says there is no limit

#### Scenario: Invalid value
- **WHEN** the configuration file sets `max_memory_mb = "big"`
- **THEN** the broker reports an error naming `broker.max_memory_mb` and exits with code 2
