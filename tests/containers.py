#!/usr/bin/env python3
"""Disposable real-container checks; Python never implements database storage."""

import argparse
import json
import os
import re
import secrets
import socket
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MAX_RESPONSE = 4 * 1024 * 1024


class CheckFailed(Exception):
    pass


def require(condition, description):
    if not condition:
        raise CheckFailed(description)


def phase(description):
    print(description, flush=True)


def free_port():
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


class Probe:
    def __init__(self):
        self.project = "emilybase-probe-" + secrets.token_hex(8)
        self.master = secrets.token_hex(32)
        self.port = free_port()
        self.url = f"http://127.0.0.1:{self.port}"
        self.env = {
            **os.environ,
            "EMILYBASE_MASTER_KEY": self.master,
            "EMILYBASE_PORT": str(self.port),
        }
        self.docker = os.environ.get("EMILYBASE_DOCKER", "docker")
        self.compose_args = [
            "compose",
            "--project-name",
            self.project,
            "--file",
            str(ROOT / "compose.yaml"),
        ]
        self.private = [self.master]
        self.projects = []
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    def docker_run(self, *args, timeout=120, ok=True, env=None):
        try:
            completed = subprocess.run(
                [self.docker, *args],
                cwd=ROOT,
                env=env or self.env,
                capture_output=True,
                timeout=timeout,
                check=False,
            )
        except (OSError, subprocess.TimeoutExpired) as error:
            raise CheckFailed("container command failed or timed out") from error
        # Docker inspection and Compose expansion can contain credentials.
        # Never echo their output, command payloads, or environment on failure.
        if ok:
            require(completed.returncode == 0, "container command returned failure")
        return completed

    def compose(self, *args, **options):
        return self.docker_run(*self.compose_args, *args, **options)

    def cli(self, *args, ok=True):
        return self.compose(
            "run",
            "--rm",
            "--no-deps",
            "--entrypoint",
            "emilybase",
            "server",
            *args,
            ok=ok,
        )

    def request(self, route, key=None, payload=None, expected=200):
        headers = {"content-type": "application/json"}
        if key is not None:
            headers["authorization"] = "Bearer " + key
        body = None if payload is None else json.dumps(payload).encode()
        request = urllib.request.Request(
            self.url + route,
            data=body,
            headers=headers,
            method="GET" if body is None else "POST",
        )
        try:
            response = self.opener.open(request, timeout=15)
        except urllib.error.HTTPError as response_error:
            response = response_error
        with response:
            status = response.status
            raw = response.read(MAX_RESPONSE + 1)
        require(len(raw) <= MAX_RESPONSE, "test response exceeded bound")
        require(status == expected, "unexpected HTTP status")
        try:
            return json.loads(raw)
        except (ValueError, UnicodeError) as error:
            raise CheckFailed("invalid HTTP response format") from error

    def create(self, name):
        created = self.request(
            "/v1/projects", self.master, {"name": name}, expected=201
        )
        identifier = created["project"]["id"]
        key = created["api_key"]
        require(re.fullmatch("[0-9a-f]{32}", identifier), "project identifier shape")
        require(re.fullmatch("[0-9a-f]{64}", key), "project credential shape")
        self.private.extend([identifier, key])
        project = (identifier, key)
        self.projects.append(project)
        return project

    def sql(self, project, sql, parameters=None, expected=200):
        identifier, key = project
        self.private.append(sql)
        return self.request(
            f"/v1/projects/{identifier}/sql",
            key,
            {"sql": sql, "parameters": parameters or []},
            expected,
        )

    def status(self, project, expected=200):
        identifier, key = project
        return self.request(f"/v1/projects/{identifier}/status", key, expected=expected)

    def up(self, recreate=False):
        args = ["up", "--detach", "--no-build", "--wait", "--wait-timeout", "120"]
        if recreate:
            args.append("--force-recreate")
        self.compose(*args, timeout=160)
        require(self.request("/health") == {"status": "experimental"}, "liveness")

    def restored_server(self, directory, first, second, expected, before, sibling):
        phase(
            "Serve restored projects with preserved credentials and independent writes"
        )
        container = (
            self.compose(
                "run",
                "--detach",
                "--rm",
                "--service-ports",
                "--env",
                "EMILYBASE_DATA_DIR=" + directory,
                "server",
            )
            .stdout.decode()
            .strip()
        )
        require(
            bool(re.fullmatch("[0-9a-f]{64}", container)), "restored container identity"
        )
        try:
            deadline = time.monotonic() + 30
            while True:
                try:
                    health = self.request("/health")
                    require(health == {"status": "experimental"}, "restored liveness")
                    break
                except (OSError, CheckFailed):
                    require(time.monotonic() < deadline, "restored startup deadline")
                    time.sleep(0.1)
            require(
                self.status(first) == before, "restored first project scope and counts"
            )
            require(
                self.status(second) == sibling,
                "restored second project scope and counts",
            )
            require(
                self.sql(first, "SELECT * FROM t ORDER BY id")["results"][0]
                == expected,
                "restored container rows",
            )
            self.request(f"/v1/projects/{first[0]}/status", second[1], expected=401)
            changed = self.sql(
                first, "INSERT INTO t VALUES(99,'restored-registry-only')"
            )
            require(
                changed["transaction"] == before["transaction"] + 1,
                "restored container accepts independent commits",
            )
            logs = self.docker_run("logs", container).stdout.decode(errors="replace")
            require(
                all(secret not in logs for secret in self.private),
                "restored container log redaction",
            )
        finally:
            self.docker_run("stop", "--time", "30", container)

    def stop(self):
        self.compose("stop", "--timeout", "30")

    def inspect(self):
        identifier = self.compose("ps", "--quiet", "server").stdout.decode().strip()
        require(bool(identifier), "running server container")
        return json.loads(self.docker_run("inspect", identifier).stdout)[0]

    def setup(self, build):
        self.docker_run("info", "--format", "{{.ServerVersion}}")
        no_secret = {**self.env}
        no_secret.pop("EMILYBASE_MASTER_KEY", None)
        require(
            self.compose("config", "--quiet", env=no_secret, ok=False).returncode != 0,
            "Compose must reject a missing master key",
        )
        self.compose("config", "--quiet")
        if build:
            phase("Build original Rust server and CLI in the image")
            self.compose("build", timeout=900)
        self.up()

    def configuration(self):
        phase("Check non-root process, loopback port, private volume and limits")
        container = self.inspect()
        host = container["HostConfig"]
        require(container["Config"]["User"] == "10001:10001", "unprivileged user")
        require(host["ReadonlyRootfs"], "read-only image filesystem")
        require("ALL" in host["CapDrop"], "dropped capabilities")
        require(
            "no-new-privileges:true" in host["SecurityOpt"], "privilege restriction"
        )
        require(host["Memory"] == 512 * 1024 * 1024, "memory configuration")
        require(host["NanoCpus"] == 1_000_000_000, "CPU configuration")
        require(host["PidsLimit"] == 64, "process configuration")
        require(container["Config"]["StopTimeout"] == 30, "graceful stop configuration")
        ports = host["PortBindings"]["7000/tcp"]
        require(
            ports == [{"HostIp": "127.0.0.1", "HostPort": str(self.port)}],
            "loopback port",
        )
        require(
            self.compose("exec", "-T", "server", "id", "-u").stdout.strip() == b"10001",
            "runtime user",
        )
        require(
            self.compose(
                "exec",
                "-T",
                "server",
                "stat",
                "-c",
                "%a:%u",
                "/var/lib/emilybase/projects",
            ).stdout.strip()
            == b"700:10001",
            "private database directory",
        )
        denied = self.compose(
            "exec", "-T", "server", "touch", "/unwanted-write", ok=False
        )
        require(denied.returncode != 0, "image filesystem rejects writes")
        for filename, expected in [
            ("memory.max", b"536870912"),
            ("cpu.max", b"100000 100000"),
            ("pids.max", b"64"),
        ]:
            current = self.compose(
                "exec", "-T", "server", "cat", "/sys/fs/cgroup/" + filename
            )
            require(
                current.stdout.strip() == expected,
                "cgroup-v2 resource limit is applied",
            )

    def data_and_credentials(self):
        phase("Check isolated SQL, parameter literals, rollback and key rotation")
        first, second = self.create("container-first"), self.create("container-second")
        initial = self.sql(
            first,
            "CREATE TABLE t(id INTEGER PRIMARY KEY,label TEXT); INSERT INTO t VALUES (1,$1)",
            [
                {"type": "text", "value": "'); DROP TABLE t; -- synthetic"},
            ],
        )
        require(
            initial["transaction"] == 2 and initial["committed"],
            "initial committed SQL",
        )
        self.sql(
            second,
            "CREATE TABLE t(id INTEGER PRIMARY KEY,label TEXT); INSERT INTO t VALUES (9,'sibling')",
        )
        before = self.status(first)
        rolled = self.sql(
            first, "BEGIN; INSERT INTO t VALUES (2,'discarded'); ROLLBACK"
        )
        require(not rolled["committed"], "explicit rollback")
        self.sql(
            first,
            "INSERT INTO t VALUES (3,'discarded'); INSERT INTO t VALUES (1,'duplicate')",
            expected=400,
        )
        require(
            self.status(first) == before,
            "rejected and rolled-back scripts preserve status",
        )
        self.request(f"/v1/projects/{first[0]}/status", second[1], expected=401)
        self.request(f"/v1/projects/{first[0]}/status", self.master, expected=401)
        rotated = self.request(f"/v1/projects/{first[0]}/keys/rotate", self.master, {})
        key = rotated["api_key"]
        self.private.append(key)
        require(rotated["project"]["key_epoch"] == 2, "key rotation epoch")
        self.status(first, expected=401)
        first = (first[0], key)
        require(self.status(first) == before, "key rotation preserves data")
        return first, second

    def sdk(self, enabled):
        if not enabled:
            return
        phase("Check project SDK against the actual container server")
        environment = {
            **self.env,
            "EMILYBASE_TEST_URL": self.url,
            "EMILYBASE_TEST_MASTER_KEY": self.master,
        }
        try:
            completed = subprocess.run(
                ["npm", "run", "test:integration", "--prefix", "sdks/typescript"],
                cwd=ROOT,
                env=environment,
                capture_output=True,
                timeout=60,
                check=False,
            )
        except (OSError, subprocess.TimeoutExpired) as error:
            raise CheckFailed("container SDK checks could not complete") from error
        require(completed.returncode == 0, "container SDK checks failed")

    def backup_recreate(self, first, second):
        phase(
            "Stop, back up, verify, restore, compact and recreate with the same volume"
        )
        expected = self.sql(first, "SELECT * FROM t ORDER BY id")["results"][0]
        before = self.status(first)
        sibling = self.status(second)
        self.stop()
        source = f"/var/lib/emilybase/projects/{first[0]}/data"
        archive = "/var/lib/emilybase/synthetic.backup"
        restored = "/var/lib/emilybase/restored-data"
        saved = json.loads(self.cli("primary-index-save", source, "t").stdout)
        require(
            saved["entries"] == before["rows"]
            and saved["transaction"] == before["transaction"],
            "private source cache binds acknowledged rows",
        )
        self.cli("backup", source, archive)
        self.cli("backup-verify", archive)
        require(
            self.cli("backup", source, archive, ok=False).returncode != 0,
            "backup does not clobber",
        )
        self.cli("restore", archive, restored)
        require(
            json.loads(self.cli("primary-index-load", restored, "t").stdout) is None,
            "verified backup omits disposable cache files",
        )
        self.cli("primary-index-save", restored, "t")
        loaded = json.loads(self.cli("primary-index-load", restored, "t").stdout)
        require(loaded["entries"] == before["rows"], "restored private cache loads")
        require(
            self.cli("restore", archive, restored, ok=False).returncode != 0,
            "restore does not clobber",
        )
        result = json.loads(
            self.cli("sql", restored, "SELECT * FROM t ORDER BY id").stdout
        )
        require(result["results"][0] == expected, "restored rows and schema")
        index_info = json.loads(self.cli("primary-index-info", restored, "t").stdout)
        require(
            index_info["entries"] == before["rows"]
            and index_info["excluded_long_keys"] == 0,
            "restored primary tree covers every integer key",
        )
        require(
            result["transaction"] == before["transaction"],
            "restored transaction number",
        )
        key = json.dumps({"type": "integer", "value": 1})
        location = self.cli("row-location", source, "t", key).stdout.strip()
        require(
            self.cli("row-location", restored, "t", key).stdout.strip() == location,
            "restored row keeps its bound physical location",
        )
        resolved = json.loads(
            self.cli("row-resolve", restored, "t", key, location).stdout
        )
        require(
            resolved == expected["rows"][0], "physical location resolves the exact row"
        )
        forged = json.loads(location)
        forged["row"]["page_id"] = 18446744073709551615
        require(
            self.cli(
                "row-resolve", restored, "t", key, json.dumps(forged), ok=False
            ).returncode
            != 0,
            "forged row location is refused",
        )
        added = json.loads(
            self.cli("sql", restored, "INSERT INTO t VALUES (8,'restored-only')").stdout
        )
        require(
            added["transaction"] == before["transaction"] + 1,
            "restored database accepts new writes",
        )
        self.cli("compact", source)
        source_cache = json.loads(self.cli("primary-index-load", source, "t").stdout)
        require(source_cache["entries"] == before["rows"], "compaction preserves cache")
        require(
            self.cli("primary-index-load", restored, "t", ok=False).returncode != 0,
            "restored mutation retires the old optional cache",
        )
        index = "/var/lib/emilybase/standalone-index"
        self.cli("index-create", index)
        self.cli("index-insert", index, '{"type":"integer","value":7}', "10", "3")
        pointer = json.loads(
            self.cli("index-get", index, '{"type":"integer","value":7}').stdout
        )
        require(pointer == {"page": 10, "slot": 3}, "compiled standalone index CLI")
        self.cli("index-delete", index, '{"type":"integer","value":7}')
        self.cli("index-verify", index)
        registry = "/var/lib/emilybase/projects"
        registry_archive = "/var/lib/emilybase/registry.backup"
        registry_copy = "/var/lib/emilybase/restored-projects"
        self.cli("projects-backup", registry, registry_archive)
        self.cli("projects-backup-verify", registry_archive)
        self.cli("projects-restore", registry_archive, registry_copy)
        require(
            self.cli("projects-backup", registry, registry_archive, ok=False).returncode
            != 0,
            "registry archive does not clobber",
        )
        require(
            self.cli(
                "projects-restore", registry_archive, registry_copy, ok=False
            ).returncode
            != 0,
            "registry restore does not clobber",
        )
        for project, status in [(first, before), (second, sibling)]:
            copied_data = f"{registry_copy}/{project[0]}/data"
            restored_result = json.loads(
                self.cli("sql", copied_data, "SELECT * FROM t").stdout
            )
            require(
                restored_result["transaction"] == status["transaction"],
                "registry copy preserves transaction",
            )
            require(
                len(restored_result["results"][0]["rows"]) == status["rows"],
                "registry copy preserves isolated rows",
            )
        self.restored_server(registry_copy, first, second, expected, before, sibling)
        self.up(recreate=True)
        require(
            self.status(first) == before,
            "source status survives compaction and recreation",
        )
        require(
            self.sql(first, "SELECT * FROM t ORDER BY id")["results"][0] == expected,
            "source rows survive recreation",
        )
        require(self.status(second) == sibling, "sibling survives unchanged")

    def killed_writer(self, first, second):
        phase("Kill a live container writer; reopen every acknowledged whole script")
        initial = self.sql(
            first, "CREATE TABLE crash(id INTEGER PRIMARY KEY,value INTEGER)"
        )
        before = initial["transaction"]
        sibling = self.status(second)
        ready, stop = threading.Event(), threading.Event()
        acknowledged, errors = [], []

        def writer():
            for identifier in range(1, 33):
                if stop.is_set():
                    return
                try:
                    report = self.sql(
                        first,
                        "INSERT INTO crash VALUES ($1,$2)",
                        [
                            {"type": "integer", "value": identifier},
                            {"type": "integer", "value": identifier * 7},
                        ],
                    )
                    if (
                        not report["committed"]
                        or report["transaction"] != before + identifier
                    ):
                        errors.append("writer transaction sequence")
                        return
                    acknowledged.append(identifier)
                    if identifier == 5:
                        ready.set()
                    time.sleep(0.005)
                except (OSError, CheckFailed):
                    return  # A lost response is allowed only as an unknown outcome.

        worker = threading.Thread(target=writer, daemon=True)
        worker.start()
        try:
            require(ready.wait(15), "writer did not acknowledge its initial commits")
            self.compose("kill", "--signal", "SIGKILL", "server")
        finally:
            stop.set()
            worker.join(timeout=20)
        require(not worker.is_alive(), "writer terminated")
        require(not errors and len(acknowledged) >= 5, "writer acknowledged prefix")
        self.up()
        report = self.sql(first, "SELECT * FROM crash ORDER BY id")
        rows = report["results"][0]["rows"]
        recovered = [row[0]["value"] for row in rows]
        require(
            recovered == list(range(1, len(rows) + 1)),
            "recovered rows form a complete prefix",
        )
        require(len(rows) >= len(acknowledged), "all acknowledged rows survived")
        require(
            all(row[1]["value"] == row[0]["value"] * 7 for row in rows),
            "whole row values survived",
        )
        require(
            report["transaction"] == before + len(rows),
            "recovered transaction sequence",
        )
        require(
            self.status(second) == sibling, "killed writer leaves sibling unchanged"
        )
        next_id = len(rows) + 1
        new = self.sql(
            first,
            "INSERT INTO crash VALUES ($1,$2)",
            [
                {"type": "integer", "value": next_id},
                {"type": "integer", "value": next_id * 7},
            ],
        )
        require(
            new["transaction"] == before + next_id, "new commit after crash recovery"
        )

    def journal_damage(self, first, second):
        phase("Damage one disposable journal; verify fail-closed project isolation")
        before = self.status(first)
        self.stop()
        journal = f"/var/lib/emilybase/projects/{second[0]}/data/redo.wal"
        # Only this probe's validated random project path is a shell argument.
        self.compose(
            "run",
            "--rm",
            "--no-deps",
            "--entrypoint",
            "/bin/sh",
            "server",
            "-c",
            'dd if=/dev/zero of="$1" bs=1 count=1 conv=notrunc status=none',
            "synthetic-journal-damage",
            journal,
        )
        self.up()
        require(
            self.status(second, expected=503) == {"code": "storage_unavailable"},
            "damaged journal fails closed",
        )
        require(self.status(first) == before, "healthy sibling remains available")

    def check_method_labels(self):
        phase("Send private HTTP extension methods to accepted and denied requests")
        private_method = "SYNTHETIC_PRIVATE_METHOD_" + secrets.token_hex(16)
        self.private.append(private_method)
        for method, key, expected in [
            (private_method, self.master, 405),
            (self.master, self.master, 405),
            (private_method, "invalid", 401),
        ]:
            request = urllib.request.Request(
                self.url + "/v1/projects",
                headers={"authorization": "Bearer " + key},
                method=method,
            )
            try:
                response = self.opener.open(request, timeout=15)
            except urllib.error.HTTPError as response_error:
                response = response_error
            with response:
                require(response.status == expected, "extension method response")
                require(
                    len(response.read(MAX_RESPONSE + 1)) <= MAX_RESPONSE,
                    "extension method response bound",
                )

    def check_logs(self):
        phase("Check credential, project identifier, SQL and method redaction")
        logs = self.compose("logs", "--no-color", "server").stdout.decode(
            errors="replace"
        )
        require(
            all(secret not in logs for secret in self.private),
            "container logs disclose private request content",
        )
        require("experimental_server_listening" in logs, "structured startup log")
        require(logs.count('"method":"OTHER"') >= 3, "static method labels")

    def cleanup(self):
        # The project name is random and created solely by this probe. Its volume
        # contains synthetic data only; never use these flags on a real deployment.
        self.compose("down", "--volumes", "--remove-orphans", timeout=120)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--no-build",
        action="store_true",
        help="use an already built emilybase:local image",
    )
    parser.add_argument(
        "--sdk",
        action="store_true",
        help="also run the installed SDK integration suite",
    )
    args = parser.parse_args()
    probe = Probe()
    failed = False
    try:
        probe.setup(not args.no_build)
        probe.configuration()
        first, second = probe.data_and_credentials()
        probe.sdk(args.sdk)
        probe.backup_recreate(first, second)
        probe.killed_writer(first, second)
        probe.journal_damage(first, second)
        probe.check_method_labels()
        probe.check_logs()
        phase("All real-container checks passed")
    except (CheckFailed, OSError, ValueError, KeyError, KeyboardInterrupt) as error:
        # Do not render exception strings from HTTP peers, process output or JSON.
        if isinstance(error, CheckFailed):
            print(f"Container check failed: {error}", file=sys.stderr)
        else:
            print("Container check failed; private responses withheld", file=sys.stderr)
        failed = True
    finally:
        try:
            probe.cleanup()
        except CheckFailed:
            print(
                f"Cleanup failed for disposable project {probe.project}",
                file=sys.stderr,
            )
            failed = True
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
