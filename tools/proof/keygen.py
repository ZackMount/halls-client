#!/usr/bin/env python3
"""Create an independent organizer key set outside the source repository."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import struct

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric import ec, rsa

ROOT = Path(__file__).resolve().parents[2]


def generate(directory):
    directory = directory.resolve()
    if directory == ROOT or ROOT in directory.parents:
        raise ValueError("Key directory must be outside the repository")
    directory.mkdir(mode=0o700, parents=True, exist_ok=False)
    signer = ec.generate_private_key(ec.SECP256R1())
    recipient = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    signing = signer.private_numbers()
    encryption = recipient.public_key().public_numbers()
    x = signing.public_numbers.x.to_bytes(32, "big")
    y = signing.public_numbers.y.to_bytes(32, "big")
    modulus = encryption.n.to_bytes(256, "big")
    key_id = hashlib.sha256(x + y + modulus).digest()[:8]
    private = struct.pack("<II", 0x32534345, 32) + x + y + signing.private_value.to_bytes(32, "big")
    public = struct.pack("<6I", 0x31415352, 2048, 3, 256, 0, 0) + encryption.e.to_bytes(3, "big") + modulus
    header = "/* Client signing material; keep outside source control. */\n"
    for name, value in [("proof_signing_key", private), ("proof_recipient_key", public), ("proof_key_id", key_id)]:
        header += f"static const unsigned char {name}[{len(value)}]={{" + ",".join(map(str, value)) + "};\n"
    trust = {"key_id": key_id.hex(), "signing_public": (b"\x04" + x + y).hex()}
    files = {
        "proof_keys.h": header.encode(),
        "recipient.pem": recipient.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8, serialization.NoEncryption()),
        "trust.json": (json.dumps(trust, indent=2) + "\n").encode(),
    }
    for name, data in files.items():
        with os.fdopen(os.open(directory / name, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), "wb") as output:
            output.write(data)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path, help="New directory; existing keys are never overwritten")
    args = parser.parse_args()
    generate(args.directory)
    print("Created proof_keys.h, recipient.pem and trust.json. No private values printed.")
