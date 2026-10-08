# ADR0082: Explicit native private-root executable mode

Status: accepted for experimental native transport; production gates remain open.

Select a verified existing AccountRoot with EMILYBASE_ACCOUNT_ROOT. An explicitly
set EMILYBASE_DATA_DIR is mutually exclusive; absent both, preserve the legacy
registry default. Validate master, mode selection and listen address before any
filesystem work. Startup open/validation runs in a blocking worker in either mode.
Keep the master input in an owned zeroizing string until router construction; this
does not clear process environment or every transport copy.

Account mode retains the inspected fixed roster, one password workspace and the
existing four-worker/private-rate admission. Opening performs no bootstrap,
migration, clock reset or account attachment. One corrupt declared private store
refuses the complete root before listening. This differs from legacy per-project
availability and is explicit rather than silently dropping declared accounts.

Native socket tests cover WAL1/2: two refresh requests select exactly one replacement,
ACKed rotation and public writes survive forced process death, old credentials fail,
normal restart preserves current sessions and logout survives graceful restart.
An offline captured/restored clone preserves public data/key metadata but rejects
source sessions; fresh clone sessions grant no authority on the running source.
Invalid/both/missing-root configuration and private WAL corruption fail without
creation, overwrite or sensitive startup/request logs. SIGKILL tests do not prove
hardware power-loss behavior.

The default image/Compose still select the legacy data path. A dedicated root-mode
container adapter, initial private bootstrap CLI, public signup, roles/RLS,
dynamic catalogs, whole-process quotas, encryption and production acceptance
remain separate increments. No format version or existing token semantics change.
