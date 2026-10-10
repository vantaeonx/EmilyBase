# ADR0140: retained native paired archive publication and inspection

Status: accepted for standalone native operator backup publication.

## Decision

Publish an immutable captured FileSnapshot through the existing original storage
reader publisher: owned private staging, exact source length/EOF, file fsync,
complete source-to-file byte readback, no-replace selection and parent fsync.
Retain the actual selected file and original parent descriptors through final
private-file checks, whole-pair replay/graph verification and exact byte comparison
against the borrowed canonical snapshot encoder. A second valid archive substituted
in the selected inode must still fail this expected-source comparison.

Before selection, errors use original storage error semantics. Original publisher
uncertainty and any later verification error become OutcomeUnknown. Never remove,
repair, overwrite or retry a selected result. Readonly native inspection opens
one no-follow regular private file, bounds its size before allocation, retains its
parent/inode and rechecks both around complete decoding. Reports are copied metadata,
not retained publication authority or later namespace leases.

## Consequences

Operator output must be outside live object inventories. Immutable FileSnapshot
contains no source-directory authority and cannot classify arbitrary target paths
against a still-live or moved source. This is not a user HTTP upload/backup route.
Native trusted ancestor assumptions remain explicit.

Encoding is streamed with8 KiB scratch; final semantic inspection materializes
bounded complete archive bytes and transient original metadata replay. There is no
whole-process heap budget or measured maximum-size resource claim. Existing byte
formats, private schema1, fsync ordering and transaction ACK rules remain unchanged.

Process kills after preparation, after durable selection and after caller success
exercise absent-versus-verifiable destinations and unchanged acknowledged source
state. The original publisher's internal staging/fsync crash cases also rerun;
these are process-crash evidence, not physical power-loss guarantees. Coordinated
common restore, Root integration, current user policy and production acceptance
remain separate gates.
