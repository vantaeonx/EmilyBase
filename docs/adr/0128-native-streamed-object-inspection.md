# ADR0128: streamed native single-object metadata inspection

Status: accepted for experimental synthetic-data use.

## Context

The offline directory inspect command returned only metadata but called the owned
get API, materializing the complete payload. ADR0126 already provides an original
complete streaming decoder and descriptor-relative report admission for inventory.
Reusing those checks should preserve current scope and selected-file ownership.

## Decision

Add native `ProjectDirectory::inspect`, keeping the actual checked file descriptor
across the final marker check and rechecking its stability/private metadata and
visible inode before returning a report. Route the existing CLI command through it.
Keep owned get/capture, inventories, archive/write behavior and all bytes unchanged.

## Consequences and acceptance

Complete hashing remains mandatory. The bounded payload scratch is not a heap/RSS
quota or zero-allocation claim. This exact selected-object operation is not a full
inventory, authorization capability or retained lease. Namespace retention follows
the owned directory rather than a former pathname.

Require actual maximum/readonly/binary CLI behavior, complete late corruption,
foreign scope, post-verification inode/metadata/marker mutations, renamed directory
separation, missing/unsafe objects and generated native images on both supported
Rust toolchains. Measure any CLI memory claim in fresh actual processes with an
explicit earlier source and unchanged payload/report. No platform stage closes.
See [contract](../native-object-inspection.md).

Executed172 checks per toolchain on572 frozen source hashes, including seven new
regular cases,64 generated native images and nine controlled mutation shapes.
Formatting and strict stable workspace/fuzz lint pass; minimum tests/build and
all-fuzz compilation pass. The unchanged byte/reader ASAN target completed937259
inputs/46s without findings. Five fresh
CLI samples per shape/source preserve reports; full local RSS observations,
including the small shape without improvement, are recorded in the contract.
