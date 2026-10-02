# Contributing

Keep changes small and explain their observable behavior. Discuss file-format
or durability changes in an issue and record major decisions in `docs/adr`.
Reproduce bugs with a failing test before implementing a fix.

Run formatting, Clippy with warnings denied, workspace tests and the build before
opening a pull request. Storage changes need corruption and boundary tests;
transaction changes need subprocess crash-recovery tests. Do not claim a test
passed unless it was executed. Explain platform and durability limitations.

Do not commit credentials, environment files, signing keys, real database files
or user data. Keep the storage layer synchronous and independent of networking.
Dependencies must have compatible licenses and must not provide a database engine.
Contributions are licensed under Apache-2.0.
