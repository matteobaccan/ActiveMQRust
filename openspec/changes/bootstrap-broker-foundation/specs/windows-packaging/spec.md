## ADDED Requirements

### Requirement: Single executable deliverable
The project SHALL produce exactly one distributable file, `mqrust.exe`, for Windows x64 (`x86_64-pc-windows-msvc`). The file SHALL run when copied alone into any folder, without an installer, registry entries, environment variables or accompanying files.

#### Scenario: Run from an empty folder
- **WHEN** `mqrust.exe` is copied alone into an empty folder and started with no arguments
- **THEN** the broker starts and accepts OpenWire connections on `0.0.0.0:61616`

### Requirement: No runtime dependencies
The executable SHALL import only Windows system DLLs (such as `kernel32`, `ntdll`, `ws2_32`, `advapi32`, `bcrypt`, `bcryptprimitives`, `userenv`). It SHALL NOT require the Visual C++ Redistributable, .NET, Java, OpenSSL or any other library on the target machine. The C runtime SHALL be linked statically.

#### Scenario: Dependency check passes
- **WHEN** `scripts\check-deps.cmd` runs `dumpbin /dependents` on the release executable
- **THEN** every listed DLL is in the system allow-list and the script exits with code 0

#### Scenario: Dependency check rejects runtime DLLs
- **WHEN** the executable imports a DLL outside the allow-list, such as `VCRUNTIME140.dll`
- **THEN** `scripts\check-deps.cmd` reports that DLL and exits with a non-zero code

#### Scenario: Clean machine
- **WHEN** `mqrust.exe` alone is started in a clean Windows Sandbox without the Visual C++ Redistributable
- **THEN** the broker starts and the Java acceptance program run from the host passes the scenarios enabled so far

### Requirement: Supported platforms
The executable SHALL run on Windows 10, Windows 11 and Windows Server 2016 or later, x64. Builds for other operating systems are not required.

#### Scenario: Supported Windows version
- **WHEN** `mqrust.exe` is started on Windows 10 or Windows Server 2016 x64
- **THEN** it starts without errors

### Requirement: Windows version resource
The executable SHALL embed a Windows version resource with `ProductName` `ActiveMQRust`, and `FileVersion` and `ProductVersion` equal to the crate version.

#### Scenario: File properties
- **WHEN** a user opens Properties → Details on `mqrust.exe`
- **THEN** the product name is `ActiveMQRust` and the version equals the crate version

### Requirement: Embedded assets
All assets the executable needs at runtime (such as HTML, CSS and templates) SHALL be embedded in the binary.

#### Scenario: No asset files on disk
- **WHEN** the broker runs from a folder that contains only `mqrust.exe`
- **THEN** no feature fails because of a missing asset file
