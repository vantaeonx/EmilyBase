# Experimental joined account/file archive bytes

`AccountFileArchiveReader::from_snapshot` borrows a successful AccountFileSnapshot.
`verify_account_file_archive` checks an untrusted immutable byte slice and returns
opaque borrowed VerifiedAccountFileArchive. `from_verified` re-encodes that value
canonically. These are synchronous native byte APIs, outside HTTP/user routes.
No filesystem publication or common restore is implemented by this format.

## Header and exact body

All integers are little endian. The header has128 bytes.

| Offset | Width | Meaning |
| --- | --- | --- |
| 0 | 8 | EMILYAFB magic |
| 8 | 2 | Version1 |
| 10 | 2 | Header length128 |
| 12 | 4 | File count,0..4 |
| 16 | 4 | Canonical manifest byte length,1..32768 |
| 20 | 4 | Reserved zero |
| 24 | 8 | Existing account bundle length |
| 32 | 8 | Exact body length |
| 40 | 32 | SHA256 of the entire body |
| 72 | 52 | Reserved zero |
| 124 | 4 | CRC32 of header bytes0..124 |

The body contains exact canonical v2 declaration bytes, the unchanged v1 account
bundle, then file frames in the declared order. Each48-byte frame contains32 ASCII
project bytes, an8-byte paired archive length and8 reserved zero bytes, immediately
followed by that unchanged native paired archive. No gaps/duplicates/trailing data.

Complete input is bounded before parsing at134222400 bytes (128MiB+4672). Header
semantics/CRC and exact body length/hash are checked before JSON or nested replay.
Lengths use checked partitioning; bounded native component sizes precede casts.
Unsupported versions refuse explicitly, without fallback to old formats.

## Cross-component validation and bounds

The manifest's private roster must exactly match the account bundle's private
roster. File count/order/project must exactly match its bindings and every paired
archive's native scope. Metadata database IDs must match those bindings. Public,
private and file database IDs must be globally distinct. Reset_at is preserved
declaration metadata; verification neither observes time nor resets sessions.

Apply four files,128 total physical objects and128MiB charged native image bytes.
Charge account bundle, canonical manifest, file metadata backups and every actual
native96-byte object header/payload, including orphans. Extra encoding consists of
the outer header and file/object archive frames. Decoded models, vector capacity,
descriptors and scratch are outside the image charge; no whole-process bound.

The encoder borrows complete immutable inputs and retains only bounded framing
and native readers. Body hashing uses8192-byte scratch. Reads span exact component
boundaries; seeks past EOF produce EOF, invalid negative/overflow seeks preserve
the previous position. Verified getter bytes remain borrowed from original input.
Debug prints counts and lengths, without data, verifier records or display names.

## Compatibility and remaining work

Existing database/WAL/account/native file archive bytes are unchanged. This new
outer version is experimental, not a promise of a stable production format.
Ordinary AccountRoot continues using its v1 filesystem layout and refusing v2.
Byte checksums authenticate neither backup origin nor current user permission.
No common target is published or restored; no user upload/download route is added.
Common original-owner publication/restore and current file policies remain open.

## Evidence

[Source-bound verification](measurements/2026-10-15-component-archive/verification.json)
records commands, supported Rust versions, source hashes, fuzz seeds/campaign and
exact exclusions. An independently built empty reset_at50 archive has484 bytes,
header CRC2800842180 and SHA256
`f983e4f3a626b67cc1f49dba2da181f02af9ca8c1e75a096076c06c157a388f4`.
Tests cover every header byte, every truncated empty archive, body mutations with
and without repaired outer integrity, repaired invalid lengths/rosters/bindings/
frames, arbitrary inputs, native frame equality, bounded chunk/range/seek models
and actual valid128-object versus129-object nested pairs. Synthetic accepted
fuzz corpus includes zero/one/two/four-project sources on both native WAL versions.

Final affected suites pass1179 Rust cases on each stable1.99/minimum1.89. The36
ignored entries comprise31 parent-invoked fixtures and five separate opt-in corpus
generators. Formatting, strict workspace/fuzz lint, builds and minimum fuzz
compilation pass; all645 source hashes match after final processes close with0.
The new ASAN campaign completes699173 runs in46 seconds, with21 initial inputs
(eight accepted native archives), maximum1MiB fuzz input, peak reported426MiB RSS
under512MiB and no failure artifacts. New cases include128 arbitrary models and
16 valid mixed models. This is affected-source evidence; the prior complete0b36669
workspace/SDK run and earlier full hosted4cc89e2 result are recorded separately.

The later [private file publisher](account-file-archive-publication.md) adds native
no-replace publication and readonly path-bound verification for these bytes.
Common root restore remains a separate original-owner protocol.
