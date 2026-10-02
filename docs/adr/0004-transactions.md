# ADR 0004: Serialized writer and strict locking

Status: accepted design, implementation pending.

Start with one writer per database and strict locking; hold write ownership until
commit or rollback. Readers must not observe uncommitted pages. This simplifies
recovery and establishes an explicit visibility boundary. MVCC is deferred because
version chains, garbage collection and snapshot visibility add substantial risk.
The initial pager file lock is only an ownership guard, not this transaction model.
