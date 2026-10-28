"""Key provisioning must never overwrite a key set or write inside the repository."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import tempfile
import unittest

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric import ec

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('keygen', ROOT / 'tools/proof/keygen.py')
keygen = importlib.util.module_from_spec(spec)
spec.loader.exec_module(keygen)


class KeyTests(unittest.TestCase):
    def test_key_set_matches_and_cannot_be_overwritten(self):
        with tempfile.TemporaryDirectory() as tmp:
            folder = Path(tmp) / 'keys'
            keygen.generate(folder)
            saved = {p.name: p.read_bytes() for p in folder.iterdir()}
            self.assertEqual(set(saved), {'proof_keys.h', 'recipient.pem', 'trust.json'})
            with self.assertRaises(FileExistsError):
                keygen.generate(folder)
            self.assertEqual(saved, {p.name: p.read_bytes() for p in folder.iterdir()})
            trust = json.loads(saved['trust.json'])
            raw = re.search(rb'proof_signing_key\[104\]=\{([^}]+)\}', saved['proof_keys.h'])[1]
            material = bytes(int(n) for n in raw.split(b','))
            signer = ec.derive_private_key(int.from_bytes(material[72:], 'big'), ec.SECP256R1())
            point = signer.public_key().public_bytes(serialization.Encoding.X962, serialization.PublicFormat.UncompressedPoint)
            self.assertEqual(point.hex(), trust['signing_public'])
            self.assertEqual(point[1:], material[8:72])
            recipient = serialization.load_pem_private_key(saved['recipient.pem'], None)
            modulus = recipient.public_key().public_numbers().n.to_bytes(256, 'big')
            self.assertEqual(hashlib.sha256(point[1:] + modulus).digest()[:8].hex(), trust['key_id'])
            if os.name == 'posix':
                self.assertEqual(folder.stat().st_mode & 0o777, 0o700)
                for path in folder.iterdir():
                    self.assertEqual(path.stat().st_mode & 0o777, 0o600)

    def test_source_tree_destination_rejected(self):
        for path in (ROOT, ROOT / 'keys', ROOT / 'src/../keys'):
            with self.assertRaises(ValueError):
                keygen.generate(path)

    @unittest.skipUnless(os.name == 'posix', 'Symlink creation needs privileges on Windows')
    def test_symlink_into_source_tree_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            link = Path(tmp) / 'source'
            link.symlink_to(ROOT, target_is_directory=True)
            with self.assertRaises(ValueError):
                keygen.generate(link / 'keys')


if __name__ == '__main__':
    unittest.main()
