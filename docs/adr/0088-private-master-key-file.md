# ADR0088: Bounded private master-key-file startup

Status: accepted for experimental Linux service configuration.

Support exactly one of the existing master environment value or a local secret
file. Conflicts (including empty values) refuse before data initialization. Keep
legacy exact environment validation. File transport accepts64 canonical hex
characters and at most one final LF; it does not normalize secret content.

Open a retained descriptor with NOFOLLOW/NONBLOCK/CLOEXEC on a blocking worker.
Require a regular singly linked readable file with exact0400/0600 permission bits;
validate64..65 bytes before allocating and cap the physical read at66 bytes.
Recheck descriptor/name identity and length/mode/mtime/ctime after reading. Reject
observed substitutions without logging paths or input. Parent traversal and local
operator/filesystem integrity remain trusted; no race-free hostile-filesystem or
process-wide secret-erasure claim is made. No required database engine is added.

Zeroize owned buffers; leave the persistent operator file unchanged and plaintext.
File ownership is not equated to service UID: read access and private mode are
required, enabling deliberately provisioned read-only secrets. There is no runtime
reload, permission repair, key generation or root bootstrap. Controlled restart
loads a replacement master digest without replacing project service keys or WAL.

Add a standalone private-root Compose variant using only a key-file path. Explicit
one-off UID10001 stdin provisioning uses noclobber/umask077 in its own named volume.
Keep the secret outside the strict account root and account bundles. Retain the
original environment Compose and default registry image. Do not auto-migrate or
merge configurations. The Docker operator can read the volume; encryption remains
open and production acceptance is unchanged.

Native/unit/property tests cover validation, bounded FIFO refusal, observed
replacement and actual restart authority. Extend the common six-ACK-kill lifecycle
for environment/file sources and both supported Rust toolchains. Hosted CI runs a
real file-key container and asserts absence of the master-value environment entry.
Test drafts needed a bound temporary value and the existing typed metadata/list
contract; fixture corrections did not weaken runtime validation.
