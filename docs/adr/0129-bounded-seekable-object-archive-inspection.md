# ADR0129: bounded seekable object archive metadata inspection

Status: accepted for experimental synthetic-data use.

## Context

Native archive inspection and selected-backup readback allocated a complete archive
image, potentially more than64 MiB, to return only aggregate metadata. The borrowed
codec verifies outer body integrity before nested formats. A single-pass structural
parser would change that error priority or need to drain rejected input carefully.

## Decision

Share the exact original header decoder and add a bounded synchronous Read+Seek
metadata verifier. Reset to zero, validate header/complete outer hash/EOF, then
seek to the first frame and verify every nested object, count, order, total and
canonical inventory digest. Bound each nested reader so its EOF probe stays within
the frame. Retain only bounded metadata tuples; reuse the original object reader.

Route native inspection/readback through it while preserving private file metadata,
visible inode, selected descriptor ownership and publication uncertainty. Keep
owned capture/encoding/restore unchanged. No additional durable format or service
entry point is selected.

## Consequences and acceptance

The second source pass saves complete image retention but is not a throughput,
network deadline or whole-process memory guarantee. Standard reader contracts and
native owner/filesystem assumptions remain necessary. Returned reports confer no
project/user authority or later filesystem lease.

Require independent known bytes, byte/reader differential mutations and repaired
headers, every prefix, nested framing/order/digest checks, bounded interrupted
reads, read/seek/EOF errors, native between-pass substitutions, generated archives,
maximum real CLI behavior, both supported Rust toolchains and an executed sanitizer
comparison. Any memory claim needs fresh actual processes with unchanged files and
reports, including the small shape. No platform or production gate closes here.
See [contract](../seekable-object-archive-inspection.md).

Executed181 relevant checks per toolchain on575 frozen hashes. Nine new regular
cases include64 generated archives and actual maximum CLI/controlled native faults.
Expanded ASAN differential:756330 inputs/46s/RSS391 under512, no findings. Complete
source-bound checks and all local RSS samples, including the small shape without
improvement, are recorded in the contract. The earlier broad1518-test baseline is
not claimed for this later source.
