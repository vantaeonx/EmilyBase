#!/usr/bin/env python3
"""Synthetic private-root lifecycle against real Rust processes or containers."""

import argparse
import hashlib
import json
import os
import re
import secrets
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

from containers import ROOT, CheckFailed, Probe, phase, require


class AccountProbe(Probe):
    data_directory = "/var/lib/emilybase/account-root"

    def __init__(self):
        super().__init__()
        self.compose_args[-1] = str(ROOT / "compose.accounts.yaml")
        self.container = None
        self.logs = []
        self.archive = "/var/lib/emilybase/account.backup"
        self.clone = "/var/lib/emilybase/account-copy"

    def setup(self, build):
        self.docker_run("info", "--format", "{{.ServerVersion}}")
        no_secret = {**self.env}
        no_secret.pop("EMILYBASE_MASTER_KEY", None)
        require(
            self.compose("config", "--quiet", env=no_secret, ok=False).returncode != 0,
            "private Compose rejects missing master key",
        )
        self.compose("config", "--quiet")
        if build:
            phase("Build separate private-root image target")
            self.compose("build", timeout=900)

    def up(self, directory=None):
        if directory is None:
            super().up(recreate=True)
            self.container = (
                self.compose("ps", "--quiet", "server").stdout.decode().strip()
            )
        else:
            self.container = (
                self.compose(
                    "run",
                    "--detach",
                    "--no-deps",
                    "--service-ports",
                    "--env",
                    "EMILYBASE_ACCOUNT_ROOT=" + directory,
                    "server",
                )
                .stdout.decode()
                .strip()
            )
            self.wait()
        require(
            bool(re.fullmatch("[0-9a-f]{64}", self.container)),
            "private container identity",
        )

    def wait(self):
        deadline = time.monotonic() + 30
        while True:
            try:
                require(
                    self.request("/health") == {"status": "experimental"},
                    "private liveness",
                )
                return
            except (OSError, CheckFailed):
                require(time.monotonic() < deadline, "private startup deadline")
                time.sleep(0.1)

    def configuration(self):
        super().configuration()
        environment = self.inspect()["Config"]["Env"]
        require(
            "EMILYBASE_ACCOUNT_ROOT=" + self.data_directory in environment
            and not any(
                entry.startswith("EMILYBASE_DATA_DIR=") for entry in environment
            ),
            "private image selects exactly one data mode",
        )
        for directory in [self.data_directory, self.data_directory + "/private"]:
            mode = self.compose(
                "exec", "-T", "server", "stat", "-c", "%a:%u", directory
            ).stdout.strip()
            require(mode == b"700:10001", "private root owner and permissions")

    def stop(self, hard=False):
        if self.container is not None:
            if hard:
                self.docker_run("kill", "--signal", "SIGKILL", self.container)
            else:
                self.docker_run("stop", "--time", "30", self.container)
            output = self.docker_run("logs", self.container)
            self.logs.append((output.stdout + output.stderr).decode(errors="replace"))
            # Remove this stopped instance, including one-off restored servers.
            self.docker_run("rm", self.container)
            self.container = None

    def start_failure(self):
        completed = self.compose(
            "run", "--rm", "--no-deps", "server", ok=False, timeout=30
        )
        require(completed.returncode != 0, "corrupt private root refuses startup")
        self.logs.append((completed.stdout + completed.stderr).decode(errors="replace"))
        require("startup_failed" in self.logs[-1], "structured private startup failure")

    def damage(self, path):
        self.compose(
            "run",
            "--rm",
            "--no-deps",
            "--entrypoint",
            "/bin/sh",
            "server",
            "-c",
            'dd if=/dev/zero of="$1" bs=1 count=1 conv=notrunc status=none',
            "synthetic-private-damage",
            path,
        )

    def digest(self, path):
        output = self.compose(
            "run",
            "--rm",
            "--no-deps",
            "--entrypoint",
            "sha256sum",
            "server",
            path,
        ).stdout.split()
        require(
            bool(output) and re.fullmatch(b"[0-9a-f]{64}", output[0]),
            "journal digest shape",
        )
        return output[0]

    def check_logs(self):
        phase("Check all private lifecycle logs for request secrets")
        require(
            self.logs
            and any("experimental_server_listening" in log for log in self.logs),
            "private startup evidence",
        )
        for log in self.logs:
            require(
                all(
                    form not in log
                    for secret in self.private
                    for form in [
                        secret,
                        json.dumps(secret)[1:-1],
                        json.dumps(secret, ensure_ascii=False)[1:-1],
                    ]
                ),
                "private lifecycle log redaction",
            )

    def cleanup(self):
        try:
            self.stop()
        finally:
            super().cleanup()


class FileAccountProbe(AccountProbe):
    """The server container receives a file path, never the master secret as ENV."""

    def __init__(self):
        super().__init__()
        self.compose_args[-1] = str(ROOT / "compose.accounts.file.yaml")
        self.key_file = "/var/lib/emilybase/master.key"
        self.private.append(self.key_file)
        self.env.pop("EMILYBASE_MASTER_KEY", None)

    def setup(self, build):
        self.docker_run("info", "--format", "{{.ServerVersion}}")
        self.compose("config", "--quiet")
        if build:
            phase("Build private-root image for file-based master key")
            self.compose("build", timeout=900)
        # Explicit fixture provisioning as UID 10001. The key travels only on
        # stdin; noclobber refuses an existing file in this disposable volume.
        self.compose(
            "run",
            "--rm",
            "--no-deps",
            "-T",
            "--entrypoint",
            "/bin/sh",
            "server",
            "-c",
            'set -C; umask 077; cat > "$1"',
            "synthetic-master-file",
            self.key_file,
            stdin_bytes=self.master.encode(),
        )

    def configuration(self):
        super().configuration()
        environment = self.inspect()["Config"]["Env"]
        require(
            "EMILYBASE_MASTER_KEY_FILE=" + self.key_file in environment
            and not any(
                entry.startswith("EMILYBASE_MASTER_KEY=") for entry in environment
            ),
            "file-mode container does not expose the master key in ENV",
        )
        mode = self.compose(
            "exec", "-T", "server", "stat", "-c", "%a:%u:%h:%s", self.key_file
        ).stdout.strip()
        require(mode == b"600:10001:1:64", "master file is private and singly linked")


class NativeProbe(AccountProbe):
    """Same HTTP/CLI lifecycle, without claiming any Docker configuration check."""

    def __init__(self, binary_dir):
        super().__init__()
        self.directory = tempfile.TemporaryDirectory(prefix="emilybase-private-probe-")
        self.data_directory = str(Path(self.directory.name) / "account-root")
        self.archive = str(Path(self.directory.name) / "account.backup")
        self.clone = str(Path(self.directory.name) / "account-copy")
        self.binary_dir = binary_dir.resolve()
        self.process = None
        self.output = None
        self.env.pop("EMILYBASE_MASTER_KEY_FILE", None)
        self.env.pop("EMILYBASE_DATA_DIR", None)
        self.env.pop("EMILYBASE_ACCOUNT_ROOT", None)
        self.env["EMILYBASE_LISTEN"] = f"127.0.0.1:{self.port}"

    def execute(self, args, ok=True, directory=None):
        environment = {**self.env}
        if directory is not None:
            environment["EMILYBASE_ACCOUNT_ROOT"] = directory
        try:
            completed = subprocess.run(
                args,
                env=environment,
                cwd=self.directory.name,
                capture_output=True,
                timeout=30,
                check=False,
            )
        except (OSError, subprocess.TimeoutExpired) as error:
            raise CheckFailed("native command failed or timed out") from error
        if ok:
            require(completed.returncode == 0, "native command returned failure")
        return completed

    def setup(self, build):
        for binary in ["emilybase", "emilybase-server"]:
            require(
                (self.binary_dir / binary).is_file(),
                "compile native Rust binaries first",
            )

    def cli(self, *args, ok=True):
        return self.execute([str(self.binary_dir / "emilybase"), *args], ok)

    def up(self, directory=None):
        require(self.process is None, "previous native process stopped")
        environment = {
            **self.env,
            "EMILYBASE_ACCOUNT_ROOT": directory or self.data_directory,
        }
        self.output = tempfile.TemporaryFile()
        self.process = subprocess.Popen(
            [str(self.binary_dir / "emilybase-server")],
            cwd=self.directory.name,
            env=environment,
            stdout=self.output,
            stderr=subprocess.STDOUT,
        )
        self.wait()

    def configuration(self):
        phase("Native preflight: container/cgroup checks are not executed")
        for directory in [self.data_directory, self.data_directory + "/private"]:
            require(
                Path(directory).stat().st_mode & 0o777 == 0o700,
                "native private directory mode",
            )

    def stop(self, hard=False):
        if self.process is not None:
            if hard:
                self.process.kill()
            else:
                self.process.terminate()
            try:
                code = self.process.wait(timeout=30)
                require(code == (-9 if hard else 0), "native shutdown status")
            finally:
                if self.process.poll() is None:
                    self.process.kill()
                    self.process.wait(timeout=10)
                self.output.seek(0)
                self.logs.append(self.output.read().decode(errors="replace"))
                self.output.close()
                self.output = None
                self.process = None

    def start_failure(self):
        completed = self.execute(
            [str(self.binary_dir / "emilybase-server")],
            ok=False,
            directory=self.data_directory,
        )
        require(
            completed.returncode != 0, "corrupt private root refuses native startup"
        )
        self.logs.append((completed.stdout + completed.stderr).decode(errors="replace"))
        require("startup_failed" in self.logs[-1], "structured native startup failure")

    def damage(self, path):
        with open(path, "r+b") as journal:
            journal.write(b"\0")
            journal.flush()
            os.fsync(journal.fileno())

    def digest(self, path):
        return hashlib.sha256(Path(path).read_bytes()).hexdigest()

    def cleanup(self):
        try:
            self.stop()
        finally:
            self.directory.cleanup()


class NativeFileProbe(NativeProbe):
    def __init__(self, binary_dir):
        super().__init__(binary_dir)
        self.key_file = Path(self.directory.name) / "master.key"
        self.private.append(str(self.key_file))
        self.env.pop("EMILYBASE_MASTER_KEY", None)
        self.env["EMILYBASE_MASTER_KEY_FILE"] = str(self.key_file)

    def setup(self, build):
        super().setup(build)
        descriptor = os.open(self.key_file, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, "wb") as file:
            file.write(self.master.encode() + b"\n")
            file.flush()
            os.fsync(file.fileno())

    def configuration(self):
        super().configuration()
        require(
            "EMILYBASE_MASTER_KEY" not in self.env, "native file mode has no key ENV"
        )
        metadata = self.key_file.stat()
        require(
            metadata.st_mode & 0o7777 == 0o600
            and metadata.st_nlink == 1
            and metadata.st_size == 65,
            "native master file transport and permissions",
        )


class Lifecycle:
    def __init__(self, probe):
        self.p = probe
        self.project = None
        self.login = "synthetic_user"
        self.password = "synthetic-password-" + secrets.token_hex(16)
        self.replacement = "synthetic-новый-界\0-" + secrets.token_hex(16)
        probe.private.extend([self.login, self.password, self.replacement])

    def auth(self, operation, payload, expected=200, key=None):
        identifier, service_key = self.project
        result = self.p.request(
            f"/v1/projects/{identifier}/auth/{operation}",
            key or service_key,
            payload,
            expected,
        )
        return result

    def sign(self, password):
        pair = self.auth("sign-in", {"login": self.login, "password": password})
        for field, prefix in [("access_token", "eba1_"), ("refresh_token", "ebr1_")]:
            value = pair[field]
            require(
                isinstance(value, str)
                and len(value) == 102
                and value.startswith(prefix),
                "session token shape",
            )
            self.p.private.append(value)
        require(
            pair["token_type"] == "Bearer" and pair["expires_at"].isdecimal(),
            "session wire metadata",
        )
        return pair

    def me(self, pair, expected=200):
        return self.auth("me", {"access_token": pair["access_token"]}, expected)

    def denied_pair(self, pair):
        self.me(pair, 401)
        self.auth("refresh", {"refresh_token": pair["refresh_token"]}, 401)

    def provision(self):
        phase("Initialize one empty private root without printing initial credentials")
        args = [
            "account-root-init",
            self.p.data_directory,
            "--name",
            "synthetic-private-container",
            "--reset-at",
            "0",
        ]
        created = self.p.cli(*args)
        require(
            b"projects=1 private_stores=1 tables=0 rows=0 accounts=0 session_families=0 reset_at=0"
            in created.stdout,
            "empty private root report",
        )
        require(
            self.p.cli("account-root-verify", self.p.data_directory).stdout
            == created.stdout,
            "initial root verifies exactly",
        )
        require(
            self.p.cli(*args, ok=False).returncode != 0,
            "initializer refuses existing private root",
        )
        self.p.up()
        self.p.configuration()
        projects = self.p.request("/v1/projects", self.p.master)
        require(
            isinstance(projects, list) and len(projects) == 1, "fixed private roster"
        )
        identifier = projects[0]["id"]
        require(
            bool(re.fullmatch("[0-9a-f]{32}", identifier)),
            "private project identifier shape",
        )
        self.p.private.append(identifier)
        require(
            identifier.encode() not in created.stdout,
            "initializer output omits project identity",
        )
        self.p.request("/v1/projects", self.p.master, {"name": "synthetic-denied"}, 405)
        rotated = self.p.request(
            f"/v1/projects/{identifier}/keys/rotate", self.p.master, {}
        )
        key = rotated["api_key"]
        require(
            bool(re.fullmatch("[0-9a-f]{64}", key))
            and rotated["project"]["key_epoch"] == 2,
            "first private service key rotation",
        )
        self.p.private.append(key)
        self.project = (identifier, key)
        user = self.auth("users", {"login": self.login, "password": self.password}, 201)
        require(
            user["credential_epoch"] == "1" and not user["disabled"],
            "first private user",
        )
        require(
            self.auth("users/list", {"limit": 1})
            == {"users": [user], "next_after": None},
            "bounded private user metadata",
        )
        pair = self.sign(self.password)
        require(self.me(pair) == user, "private session principal")
        self.p.request(
            f"/v1/projects/{identifier}/sql",
            pair["access_token"],
            {"sql": "SELECT * FROM t"},
            401,
        )
        self.auth("me", {"access_token": pair["access_token"]}, 401, self.p.master)
        self.p.sql(
            self.project,
            "CREATE TABLE t(id INT PRIMARY KEY,v TEXT);INSERT INTO t VALUES(1,'synthetic-source')",
        )
        return pair

    def table_schema(self):
        phase("Verify table schemas and kill after create/drop acknowledgements")
        identifier, key = self.project
        route = f"/v1/projects/{identifier}/tables"
        private_path = self.p.data_directory + f"/private/{identifier}/redo.wal"
        before = self.p.digest(private_path)
        tables = self.p.request(route, key)["tables"]
        require(
            len(tables) == 1 and tables[0]["name"] == "t",
            "initial typed table inventory",
        )
        description = self.p.request(route + "/schema", key, {"table": "t"})
        require(
            description["name"] == "t" and len(description["columns"]) == 2,
            "complete selected schema",
        )
        schema = {
            "name": "schema_probe",
            "primary_key": 0,
            "columns": [
                {"name": "id", "data_type": "integer", "nullable": False},
                {"name": "value", "data_type": "text", "nullable": True},
            ],
        }
        self.p.private.append("schema_probe")
        self.p.request(route + "/create", self.p.master, schema, 401)
        created = self.p.request(route + "/create", key, schema)
        require(
            created["transaction"].isdecimal() and created["table"]["id"].isdecimal(),
            "lossless schema acknowledgement IDs",
        )
        self.p.stop(hard=True)
        self.p.up()
        require(
            self.p.request(route + "/schema", key, {"table": "schema_probe"}) == schema,
            "created schema survives kill",
        )
        self.p.request(route + "/create", key, schema, 400)
        row_key = {"type": "integer", "value": "9223372036854775807"}
        point = {"table": "schema_probe", "key": row_key}
        original = [row_key, {"type": "text", "value": "synthetic-row-before"}]
        replacement = [row_key, {"type": "text", "value": "synthetic-row-after"}]
        self.p.private.extend(["synthetic-row-before", "synthetic-row-after"])
        for operation, payload, expected in [
            ("insert", {"table": "schema_probe", "row": original}, original),
            ("update", {**point, "row": replacement}, replacement),
            ("delete", point, None),
        ]:
            changed = self.p.request(route + "/rows/" + operation, key, payload)
            require(
                changed["key"] == row_key and changed["transaction"].isdecimal(),
                "lossless durable row acknowledgement",
            )
            self.p.stop(hard=True)
            self.p.up()
            require(
                self.p.request(route + "/rows/get", key, point) == {"row": expected},
                "acknowledged typed row state survives kill",
            )
            page = self.p.request(
                route + "/rows/page", key, {"table": "schema_probe", "limit": 1}
            )
            require(
                page == {"rows": [] if expected is None else [expected], "next": None},
                "bounded typed row page after restart",
            )
        dropped = self.p.request(route + "/drop", key, {"table": "schema_probe"})
        require(dropped["transaction"].isdecimal(), "durable drop acknowledgement")
        self.p.stop(hard=True)
        self.p.up()
        self.p.request(route + "/schema", key, {"table": "schema_probe"}, 400)
        require(
            len(self.p.request(route, key)["tables"]) == 1,
            "dropped schema remains absent",
        )
        require(
            self.p.digest(private_path) == before,
            "public schema work preserves private WAL",
        )

    def transfer(self):
        phase("Verify bounded table transfer and kill after import acknowledgement")
        identifier, key = self.project
        route = f"/v1/projects/{identifier}/tables/"
        private_path = self.p.data_directory + f"/private/{identifier}/redo.wal"
        private_before = self.p.digest(private_path)
        document = self.p.request(route + "export", key, {"table": "t"})
        require(
            document["version"] == 1 and len(document["rows"]) == 1,
            "complete typed table export",
        )
        document["schema"]["name"] = "transferred"
        self.p.request(route + "import", self.p.master, document, 401)
        imported = self.p.request(route + "import", key, document)
        require(
            imported["transfer"]["rows"] == 1, "durable table import acknowledgement"
        )
        self.p.stop(hard=True)
        self.p.up()
        require(
            self.p.request(route + "export", key, {"table": "transferred"}) == document,
            "acknowledged table survives kill",
        )
        self.p.request(route + "import", key, document, 400)
        require(
            self.p.digest(private_path) == private_before,
            "public table transfer preserves private WAL",
        )
        self.p.sql(self.project, "DROP TABLE transferred")

    def session_kill(self, pair):
        phase("Race two real refresh requests, then kill after the winning response")
        results, failures = [], []
        barrier = threading.Barrier(2)

        def refresh():
            try:
                barrier.wait(timeout=10)
                # Both status codes are valid; collect them without retrying.
                route = f"/v1/projects/{self.project[0]}/auth/refresh"
                body = {"refresh_token": pair["refresh_token"]}
                results.append(
                    self.p.request(
                        route,
                        self.project[1],
                        body,
                        expected=(200, 401),
                        return_status=True,
                    )
                )
            except (OSError, ValueError, CheckFailed, threading.BrokenBarrierError):
                failures.append(True)

        workers = [threading.Thread(target=refresh) for _ in range(2)]
        for worker in workers:
            worker.start()
        for worker in workers:
            worker.join(timeout=20)
        require(
            all(not worker.is_alive() for worker in workers) and not failures,
            "refresh race completed",
        )
        require(
            sorted(status for status, _ in results) == [200, 401],
            "exactly one durable refresh winner",
        )
        next_pair = next(result for status, result in results if status == 200)
        self.p.private.extend([next_pair["access_token"], next_pair["refresh_token"]])
        self.p.stop(hard=True)
        self.p.up()
        self.denied_pair(pair)
        self.me(next_pair)
        require(
            self.p.status(self.project)["rows"] == 1,
            "SQL acknowledgement survives refresh kill",
        )
        return next_pair

    def credentials(self, pair):
        phase("Kill after password change, disable and re-enable acknowledgements")
        changed = self.auth(
            "password",
            {
                "login": self.login,
                "current_password": self.password,
                "replacement_password": self.replacement,
            },
        )
        require(changed["credential_epoch"] == "2", "password epoch")
        self.p.stop(hard=True)
        self.p.up()
        self.denied_pair(pair)
        self.auth("sign-in", {"login": self.login, "password": self.password}, 401)
        pair = self.sign(self.replacement)
        disabled = self.auth("disabled", {"login": self.login, "disabled": True})
        require(
            disabled["disabled"] and disabled["credential_epoch"] == "3",
            "disabled epoch",
        )
        self.p.stop(hard=True)
        self.p.up()
        self.denied_pair(pair)
        self.auth("sign-in", {"login": self.login, "password": self.replacement}, 401)
        enabled = self.auth("disabled", {"login": self.login, "disabled": False})
        require(
            not enabled["disabled"] and enabled["credential_epoch"] == "4",
            "re-enabled epoch",
        )
        self.p.stop(hard=True)
        self.p.up()
        self.denied_pair(pair)
        return self.sign(self.replacement)

    def prune(self, active):
        phase("Bounded inactive-session cleanup, then kill after its response")
        require(
            self.auth("sessions/prune", {"limit": 128}) == {"removed": 2},
            "remove only two inactive families",
        )
        self.p.stop(hard=True)
        self.p.up()
        require(
            self.auth("sessions/prune", {"limit": 1}) == {"removed": 0},
            "cleanup remains durable after kill",
        )
        self.me(active)

    def restore(self, source_pair):
        phase("Offline compact, common-root backup, verify and independent restore")
        self.p.stop()
        for relative in [
            f"registry/{self.project[0]}/data",
            f"private/{self.project[0]}",
        ]:
            self.p.cli("compact", self.p.data_directory + "/" + relative)
        self.p.cli("account-root-verify", self.p.data_directory)
        self.p.cli("account-root-backup", self.p.data_directory, self.p.archive)
        self.p.cli("account-bundle-verify", self.p.archive)
        require(
            self.p.cli(
                "account-root-backup", self.p.data_directory, self.p.archive, ok=False
            ).returncode
            != 0,
            "private archive does not clobber",
        )
        self.p.cli(
            "account-bundle-restore",
            self.p.archive,
            self.p.clone,
            "--reset-at",
            str(int(time.time())),
        )
        require(
            self.p.cli(
                "account-bundle-restore",
                self.p.archive,
                self.p.clone,
                "--reset-at",
                "0",
                ok=False,
            ).returncode
            != 0,
            "private root restore does not clobber",
        )
        self.p.cli("account-root-verify", self.p.clone)
        self.p.up(self.p.clone)
        self.denied_pair(source_pair)
        require(
            self.p.status(self.project)["rows"] == 1,
            "clone public rows and scoped service key preserved",
        )
        clone_pair = self.sign(self.replacement)
        self.me(clone_pair)
        self.p.sql(self.project, "INSERT INTO t VALUES(2,'synthetic-copy-only')")
        self.auth("logout", {"refresh_token": clone_pair["refresh_token"]})
        self.p.stop(hard=True)
        self.p.up(self.p.clone)
        self.denied_pair(clone_pair)
        require(
            self.p.status(self.project)["rows"] == 2,
            "clone logout and SQL survive kill",
        )
        self.p.stop()
        self.p.up()
        self.me(source_pair)
        self.denied_pair(clone_pair)
        require(
            self.p.status(self.project)["rows"] == 1,
            "source remains independent with its original session",
        )
        self.p.stop()

    def corruption(self):
        phase("Private WAL corruption refuses complete-root startup without repair")
        path = f"{self.p.data_directory}/private/{self.project[0]}/redo.wal"
        before = self.p.digest(path)
        self.p.damage(path)
        damaged = self.p.digest(path)
        require(before != damaged, "private journal deliberately changed")
        self.p.start_failure()
        require(
            self.p.digest(path) == damaged,
            "startup does not overwrite corrupt private journal",
        )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--no-build", action="store_true", help="reuse emilybase:accounts-local"
    )
    parser.add_argument(
        "--native",
        action="store_true",
        help="run the same lifecycle on local Rust binaries; no Docker claim",
    )
    parser.add_argument("--binary-dir", type=Path, default=ROOT / "target/debug")
    parser.add_argument(
        "--master-file",
        action="store_true",
        help="read the master key from a private file",
    )
    args = parser.parse_args()
    if args.native:
        probe = (NativeFileProbe if args.master_file else NativeProbe)(args.binary_dir)
    else:
        probe = FileAccountProbe() if args.master_file else AccountProbe()
    failed = False
    try:
        probe.setup(not args.no_build)
        lifecycle = Lifecycle(probe)
        pair = lifecycle.provision()
        lifecycle.table_schema()
        lifecycle.transfer()
        pair = lifecycle.session_kill(pair)
        pair = lifecycle.credentials(pair)
        lifecycle.prune(pair)
        lifecycle.restore(pair)
        lifecycle.corruption()
        probe.check_logs()
        phase(
            "Native private-root preflight passed"
            if args.native
            else "All real private-root container checks passed"
        )
    except (
        CheckFailed,
        OSError,
        ValueError,
        KeyError,
        TypeError,
        KeyboardInterrupt,
    ) as error:
        if isinstance(error, CheckFailed):
            print(f"Private-root check failed: {error}", file=sys.stderr)
        else:
            print(
                "Private-root check failed; private responses withheld", file=sys.stderr
            )
        failed = True
    finally:
        try:
            probe.cleanup()
        except (CheckFailed, OSError):
            print("Private-root probe cleanup failed", file=sys.stderr)
            failed = True
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
