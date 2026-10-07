# Borrowed SQL candidates before retention

The original executor now evaluates primary JOIN conditions over two checked
borrowed source rows. It projects streamed matches without first concatenating
hidden payload. Ordinary sorted reads and sorted primary joins compare borrowed
candidates with their worst retained row; only admitted winners become owned.

All necessary source visits, physical validation, complete ON/WHERE and Boolean
branch work remain. Stable ties preserve primary source order. Matched-row and
retained/output byte limits are unchanged. A refused replacement preserves the
previous heap; output failure rolls back the existing staged script. Resolved
indices use checked access, and views cannot outlive their immutable source.

In a source-bound warmed 1500-row/3072-hidden-byte LIMIT 2 sample, ordinary sorting
requests 9819837 total allocation bytes before repair and 5074173 afterward.
Sorted/filtered primary joins fall from 19937652/19937358 to 10302324/10289358.
Peaks are 11634/17980/5180; all samples release to zero. Total allocation describes
repeated requests over the operation, not simultaneously retained heap or RSS.
Physical validation still allocates. Fixtures/caches precede profiling; allocator
overhead/rounding, stacks and profiler bookkeeping are excluded. See
[exact observations](measurements/2026-10-07-borrowed-candidates/operation-allocations.json).

[ADR 0062](adr/0062-admit-borrowed-candidates-before-cloning.md) records the ownership
and admission boundary. General fallback joins remain materialized. SQL grammar,
API, EXPLAIN, stored bytes and durable ACK are unchanged. Full numeric model/cache/
staging/transient admission, combined durable writer and production gates remain.
