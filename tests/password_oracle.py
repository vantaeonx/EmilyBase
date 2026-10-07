"""Regenerate synthetic raw-hash vectors with the independent C Argon2 oracle.

Optional test tooling only. Install argon2-cffi==25.1.0 in an isolated environment;
this dependency is not needed to build, run or test the Rust workspace.
"""

import argparse
import importlib.metadata
import json
from pathlib import Path

from argon2.low_level import Type, hash_secret_raw

ORACLE_VERSION = "25.1.0"
SALT = b"0123456789abcdef"
PARAMETERS = {
    "memory_cost": 19456,
    "time_cost": 2,
    "parallelism": 1,
    "hash_len": 32,
    "type": Type.ID,
    "version": 19,
}


def vectors():
    installed = importlib.metadata.version("argon2-cffi")
    if installed != ORACLE_VERSION:
        raise RuntimeError(f"oracle requires argon2-cffi=={ORACLE_VERSION}")
    inputs = [
        ("ascii", b"synthetic-password", {"utf8": "synthetic-password"}),
        (
            "unicode",
            "синтетический\0пароль".encode(),
            {"utf8": "синтетический\0пароль"},
        ),
        ("maximum", b"x" * 1024, {"repeat_byte_hex": "78", "count": 1024}),
    ]
    return {
        "oracle": f"argon2-cffi {installed}; independent C implementation",
        "purpose": "synthetic interoperability vectors, not credentials",
        "salt_hex": SALT.hex(),
        "memory_kib": PARAMETERS["memory_cost"],
        "iterations": PARAMETERS["time_cost"],
        "parallelism": PARAMETERS["parallelism"],
        "algorithm": "argon2id",
        "version": PARAMETERS["version"],
        "hash_bytes": PARAMETERS["hash_len"],
        "vectors": [
            {
                "name": name,
                "input": description,
                "password_bytes": len(password),
                "digest_hex": hash_secret_raw(password, SALT, **PARAMETERS).hex(),
            }
            for name, password, description in inputs
        ],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path)
    arguments = parser.parse_args()
    text = json.dumps(vectors(), ensure_ascii=False, indent=2) + "\n"
    if arguments.output:
        arguments.output.write_text(text, encoding="utf-8")
    else:
        print(text, end="")


if __name__ == "__main__":
    main()
