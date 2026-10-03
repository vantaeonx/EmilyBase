# ADR 0004: Serialized writer and strict locking

Status: serialized managed-directory transaction API implemented.

Start with one writer per database and strict locking; hold write ownership until
commit or rollback. Readers must not observe uncommitted pages. This simplifies
recovery and establishes an explicit visibility boundary. MVCC is deferred because
version chains, garbage collection and snapshot visibility add substantial risk.
The initial pager file lock is only an ownership guard, not this transaction model.

The first implementation mutably borrows a single journal owner for each
transaction. Writes stage memory only; commit syncs redo before publishing memory.
Any failed write aborts the whole transaction. A rollback or dropped transaction
does not touch WAL. State copying is bounded but unsuitable for high throughput;
copy-on-write row maps and shared concurrent readers remain future work.
