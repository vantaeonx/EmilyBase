# Native object archive publication

`ProjectDirectory::backup_to` publishes a complete experimental EMILYOBK v1
archive of one retained native object directory. This is an offline operator
operation. It grants no user/session/file-policy authority and is not integrated
into AccountRoot backups. Use synthetic data only.

The operator must supply an existing destination parent and a fresh final name.
The implementation retains that actual parent descriptor before reading the
source. A final-parent symlink is refused. Ancestors and the native operator are
trusted; this is not a sandbox against a hostile same-UID administrator. The
destination parent cannot be the source inode, including through path aliases.
No directories are created implicitly.

Publication proceeds as follows:

1. Verify and capture the complete bounded source, including every nested object
   envelope and the original scoped inventory digest.
2. Encode the canonical archive; recheck the complete current source inventory
   and the visible destination parent before creating any stage.
3. Use the original storage publisher: exclusive random private0600 stage, exact
   write, file fsync, full byte readback, no-replace rename and parent fsync.
4. Retain the selected file descriptor from that publisher. Allocate the duplicate
   descriptor before rename so descriptor exhaustion remains prepublication.
5. Rewind and fully verify that exact file; require the complete report to match
   the capture, the selected name to still refer to that inode, the original
   parent to remain visible, and the retained source marker to remain admitted.

The operation selects an immutable captured snapshot; it does not freeze source
bytes against later native filesystem writes. Cooperating owners respect the
source directory lock. Namespace moves of the source do not redirect reads into
its replacement. Destination namespace changes cannot redirect publication or
inspection into a replacement parent.

Existing names are never overwritten. Preselection failures use the original
owned-stage cleanup, which does not sweep unknown or substituted files. A failure
after selection is `PublicationUnknown`; the file is preserved and must be
explicitly inspected. Never automatically retry to replace it. A lost CLI stdout
result similarly does not mean that no archive was published.

```sh
emilybase object-directory ./objects 01010101010101010101010101010101 backup ./copy.object-archive
emilybase object-archive-verify ./copy.object-archive 01010101010101010101010101010101
```

CLI output contains only format/project/count/total/digest metadata. The binary
archive necessarily contains every captured payload and must be kept private.
The header/body/nested checksums detect accidental corruption; they neither
authenticate a writer nor encrypt data.

Bounds remain128 objects,64 MiB aggregate payload and67,124,352 complete archive
bytes, with8 MiB per object. Capture, encoded archive and final readback can be
retained concurrently: logical image storage approaches three complete images
plus working buffers/metadata. This is not streaming, a global heap reservation,
an upload quota or an aggregate service resource gate.

Verified restore into a fresh private directory, coordinated root integration,
HTTP/file policies, signed URLs, deletion, encryption, power-loss qualification
and production acceptance remain separate work.
