# ADR 0008: Self-contained journal compaction

Status: implemented and tested through API/CLI, format, recovery, ownership,
backup compatibility, publication process kills, directory sync faults and actual
WAL capacity. Broader fault campaigns and physical power-loss acceptance remain open.

The retained version-1 WAL repeats full page images on each transaction and
eventually reaches its 64 MiB limit. Compact explicitly into a version-2 WAL
containing one complete committed baseline, followed by normal page/commit frames.
The baseline retains the original relational history, database identity and last
transaction ID. Future transactions continue at that ID plus one. This removes
redundant page images, not table history or its capacity limits.

Keep the selected `redo.wal` self-contained. No checkpoint sidecar, manifest or
disposable cache authorizes retiring the old log. Header metadata binds baseline
transaction and page count. Dedicated baseline frames and a digest-bearing commit
must be complete before any recovery result is returned. Unlike an appended
uncommitted tail, an incomplete baseline always fails closed.

Create and sync a private replacement, reread and recover it, compare identity,
transaction and exact pages, then rename it over `redo.wal` and sync the enclosing
directory before reporting success. Before publication the old log remains
authoritative; after publication the new log contains the same acknowledged
state. Retain old and new WAL handles through publication. Post-rename I/O errors
poison the owner and report unknown maintenance durability; reopen before writing.

Lock the stable database directory before opening a WAL. A lock on the replaceable
WAL inode alone cannot serialize owners across rename. Keep that directory handle
locked until all WAL handles are released. This targets cooperating owners on a
Linux local filesystem; hostile local path replacement is outside the trust model.

New databases still use WAL version 1. Opening never silently upgrades them.
An explicit compaction writes version 2; old readers must reject it. Backup
envelopes bind the actual embedded WAL version and verify either supported version.
Page/record formats remain version 1. Checkpoint remains a disposable cache.

Do not claim power-loss safety, stable format or stage-2 completion from process
tests alone. Repeated compaction is bounded by the retained history size; history
vacuuming, MVCC, background rotation and shared concurrent readers are future work.
