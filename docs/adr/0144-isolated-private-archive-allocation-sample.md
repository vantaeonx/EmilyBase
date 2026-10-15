# ADR0144: isolate private archive early-refusal allocation samples

Status: accepted for synthetic opt-in diagnostics.

## Context

Hosted run38066875640 at cc06efe failed the private archive early-refusal sample:
144 requested bytes in two blocks, with144 live bytes, instead of zero. The full
stable/minimum engine tests, SDK, containers and audit jobs succeeded. An unchanged
local release sample passed200 fresh repetitions. The exact origin of the hosted
two allocations was not traced; later hosted revisions also passed unchanged.

The sample used a process-wide allocator inside the test harness. A controlled
regression held an unrelated background-thread144-byte buffer during the same
early-refusal operation. Before isolation it failed with144 bytes/one block/live144.
This demonstrates false attribution without identifying the exact hosted origin.

## Decision

Apply the existing standalone schema/token diagnostic pattern to private archives.
A feature-gated diagnostic binary prepares synthetic1MiB input/header fixtures
before profiling, executes the same1000 iterations/four early refusals per iteration
and records exact requested cumulative/peak/live bytes and block count. The parent
test harness and its background threads do not share this process allocator.
Serialize count-only JSON and flush stdout after profiler lifetime ends.

Preserve strict zero operation allocations. A negative control retains144 measured
bytes in one block and must exit unsuccessfully while reporting that allocation.
Integration tests verify both cases, retain a controlled unrelated parent buffer
through the child sample, and refuse unknown/repeated options without sample output.
No counter tolerance, subtraction, warmup-based suppression or retry-to-green is added.

## Consequences and evidence

The auth parser, accepted archive formats, original engine, quotas and durability
rules are unchanged. The executable is opt-in and is never launched by the server.
Its fixture/output contain no real account data, credentials, paths or payloads.
It measures these early-refusal operations, not accepted-archive replay memory,
whole-process RSS, throughput, user request admission or a completed security audit.

[Source-bound evidence](../measurements/2026-10-15-private-archive-isolation/verification.json)
records full opt-in/default diagnostic suites on both supported toolchains, strict
lint/format checks, controlled background regression and normal/negative fresh-process
samples. Earlier failed preliminary compilations are excluded from final results.

Final615-source checks pass58 complete opt-in release diagnostics and18 default
report checks on each Rust1.99/1.89. Workspace formatting, strict default-workspace
and opt-in all-target lint pass. Each toolchain independently runs100 fresh zero
samples and one144-byte/one-block negative control, with exact counts and status.
All615 hashes remain unchanged. The separate full hosted9ad9194 evidence covers
1648 workspace checks per toolchain; it does not certify this later source.
