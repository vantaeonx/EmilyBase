# ADR0083: Explicit creation of the first private root

Status: accepted for experimental offline initialization; production gates remain open.

Provide initialize_account_root and account-root-init for one new empty project
and its private v3 store. Require a bounded display name and explicit trusted
initial time floor. Validate both before staging. Existing files, directories and
links refuse; never infer a reset time or attach private stores during startup.

Reuse root-manifest version1, original project/WAL creation, private clock activation,
common bundle capture, complete owned inspection and no-replace root publication.
Compare the prepared registry/private histories exactly with the common captured
image. Flush private directory, manifest, root and publication parent at their
required boundaries. Parent-sync failure reports an unknown publication outcome;
inspect the selected target before further action.

Discard and zeroize the generated initial plaintext project key. Return aggregate
CLI counts without project IDs, names, keys, passwords or private rows. A separately
configured master credential lists the new project and rotates its key through the
existing operator API. Provision the first user through the current private service
key. This creates no public signup policy, user SQL authority or initial password.

A new adversarial test reproduced late private-container substitution after full
inspection and before root selection, both in the initializer candidate and the
existing restore path. Share retained-owner/inventory checks with live AccountRoot
and repeat them at both final publication boundaries, alongside the exact original
manifest-owner comparison. The regression refuses substituted private/registry/data
owners while preserving detached and foreign objects. No format or lifecycle
semantics are weakened.

Retain every failed initialization stage under the private initialization prefix.
An error may indicate foreign substitutions: do not recursively sweep uncertain
entries. Successful publication leaves no stage. Hidden stages are never selected
automatically, and repository ignores cover the documented root and stage prefix.
Stage disk quotas, automated salvage/cleanup, multi-project private bootstrap,
container root mode, public signup/RLS and production acceptance remain open.
