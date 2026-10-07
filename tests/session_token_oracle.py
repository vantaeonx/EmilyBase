"""Independent standard-library SHA-256/layout oracle; synthetic fixtures only."""

import argparse
import hashlib
import json
from pathlib import Path


def vectors():
    project = bytes([0x11]) * 16
    incarnation = bytes([0x22]) * 16
    family = bytes([0x33]) * 16
    secret = bytes([0x44]) * 32
    result = []
    for kind, tag, prefix in [("access", 1, "eba1_"), ("refresh", 2, "ebr1_")]:
        preimage = (
            b"EmilyBaseSessionToken-v1\0"
            + bytes([tag])
            + project
            + incarnation
            + family
            + secret
        )
        digest = hashlib.sha256(preimage).digest()
        record = (
            b"EBSK\0\0\0\0"
            + (1).to_bytes(2, "little")
            + bytes([tag, 0])
            + project
            + incarnation
            + family
            + digest
        )
        text = prefix + family.hex() + "." + secret.hex()
        assert len(preimage) == 106
        assert len(record) == 92
        assert len(text) == 102
        result.append(
            {
                "kind": kind,
                "preimage_bytes": len(preimage),
                "sha256": digest.hex(),
                "record_hex": record.hex(),
                "synthetic_token": text,
            }
        )
    return {"oracle_version": 1, "implementation": "Python hashlib", "vectors": result}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.write_text(json.dumps(vectors(), indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
