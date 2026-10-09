# Selected object identity through the complete result

Native publication must attribute its final readback to the actual selected inode.
Equal bytes, project IDs, checksums and reports alone do not establish that fact.
A name can address a different private regular file after the original storage
publisher returns. The original no-replace rename/fsync contract still applies.

## Retained publication

The additive storage API `publish_private_file_retained` returns the actual selected
file at a trusted operator path. Like the retained-directory variant, it duplicates
the descriptor before selection, avoiding a new descriptor-allocation failure after
selection. The handle starts at EOF. Existing unit-returning APIs preserve their
signatures, failure outcomes, bounds and cleanup behavior.

Standalone object publication seeks/reads that file, verifies complete encoded bytes
and requires the visible name to match its private singly linked inode. Project
initialization retains the actual selected marker. Native put reads the actual
selected object under its retained project directory and rechecks current scope.
Observed substitution or postselection read/metadata failure returns
`PublicationUnknown`, preserving selected and foreign artifacts.

Bounded put retains the same file beyond inner put through complete final inventory
verification. It then re-reads the selected file, verifies visible identity and exact
payload/report, and rechecks scope before returning the larger receipt. Dropping the
file after inner put would reopen the same defect at that later boundary.

## Regressions and limits

Four controlled regressions replace a selected name with another inode containing
identical bytes: standalone object, native object, scope marker, and bounded receipt
after inner put. Each fails before its corresponding correction. Additional cases
observe removal, public mode, symlink, hardlink, corruption and directory replacement
and require uncertainty without cleanup. Existing capacity/property/crash/CLI tests
exercise the same corrected paths.

This is observational filesystem validation under native operator authority, not
a namespace lease, hostile-local-administrator sandbox or user capability. Changes
after the last observation remain possible. No format, fsync point, HTTP endpoint,
AccountRoot layout, parser or persisted quota changes. No new sanitizer campaign is
claimed for these descriptor/result changes. Process kills are not power-loss tests.

See [ADR0125](adr/0125-retained-object-publication-identity.md).
