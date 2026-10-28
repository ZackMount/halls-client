#!/usr/bin/env python3
"""Verify native crypto probes and a completed recording with independent Python crypto."""
import argparse
import json
from pathlib import Path
import struct

from cryptography.exceptions import InvalidSignature, InvalidTag
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec, padding, rsa, utils
from cryptography.hazmat.primitives.ciphers.aead import AESGCM


def decrypt(data, private, trust):
    assert len(data) > 308 and data[:8] == b'HLPF\x01\r\n\x1a'
    assert data[8:16] == bytes.fromhex(trust['key_id'])
    plain, packed = struct.unpack_from('<II', data, 28)
    assert 64 < plain <= 256 * 1024 * 1024 and packed == len(data) - 308
    key = private.decrypt(data[36:292], padding.OAEP(mgf=padding.MGF1(hashes.SHA256()), algorithm=hashes.SHA256(), label=None))
    assert len(key) == 32
    return AESGCM(key).decrypt(data[16:28], data[308:] + data[292:308], data[:292]), plain


def verify(public, raw, signature):
    assert len(signature) == 64
    r, s = int.from_bytes(signature[:32], 'big'), int.from_bytes(signature[32:], 'big')
    public.verify(utils.encode_dss_signature(r, s), raw, ec.ECDSA(hashes.SHA256()))


def decompress(data, size):
    out = bytearray()
    at = 0
    while len(out) < size:
        flags = data[at]
        at += 1
        for bit in range(8):
            if len(out) == size:
                assert flags >> bit == 0
                break
            if flags & (1 << bit):
                token = struct.unpack_from('<H', data, at)[0]
                at += 2
                distance, length = (token >> 5) + 1, (token & 31) + 3
                assert distance <= len(out) and len(out) + length <= size
                for _ in range(length):
                    out.append(out[-distance])
            else:
                out.append(data[at])
                at += 1
    assert at == len(data)
    return bytes(out)


def frames(raw):
    assert raw[:4] == b'HLR5'
    build, width, height, entities, flags, words, count = struct.unpack_from('<IHHBBHI', raw, 4)
    assert (build, width, height, entities, words) == (0x01000001, 256, 256, 5, 99)
    assert flags & 128, 'Fixture must carry the developer flag'
    at = 88 + entities * 8 + 8192
    previous = [0] * words
    decoded = bytearray()
    for _ in range(count):
        for start in range(0, words, 8):
            mask = raw[at]
            at += 1
            assert mask >> min(8, words - start) == 0
            for bit in range(min(8, words - start)):
                if mask & (1 << bit):
                    delta = 0
                    for shift in range(0, 35, 7):
                        value = raw[at]
                        at += 1
                        assert shift != 28 or value <= 15
                        delta |= (value & 127) << shift
                        if value < 128:
                            assert shift == 0 or value != 0
                            break
                    else:
                        raise AssertionError('Oversized delta')
                    assert delta != 0
                    previous[start + bit] ^= delta
        decoded.extend(struct.pack('<99I', *previous))
    assert at == len(raw)
    return bytes(decoded)


def rejected(action, exceptions):
    try:
        action()
    except exceptions:
        return
    raise AssertionError('Tampered proof was accepted')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--keys', type=Path, required=True)
    parser.add_argument('--fixture', type=Path, required=True)
    parser.add_argument('--probe', type=Path, required=True)
    args = parser.parse_args()
    trust = json.loads((args.keys / 'trust.json').read_text())
    private = serialization.load_pem_private_key((args.keys / 'recipient.pem').read_bytes(), None)
    public = ec.EllipticCurvePublicKey.from_encoded_point(ec.SECP256R1(), bytes.fromhex(trust['signing_public']))
    sizes = [1, 15, 16, 17, 4095, 4096, 4097, 131101, 2097161]
    for size in sizes:
        raw = bytes(((i * 73) ^ (i // 13) ^ (i * i // 193)) & 255 for i in range(size))
        data = (args.probe / f'{size}.enc').read_bytes()
        packed, plain = decrypt(data, private, trust)
        assert packed == raw and plain == size + 64
        signature = (args.probe / f'{size}.sig').read_bytes()
        verify(public, raw, signature)
        rejected(lambda: verify(public, raw + b'x', signature), InvalidSignature)
    data = (args.fixture / 'fixture.hproof').read_bytes()
    packed, plain = decrypt(data, private, trust)
    signed = decompress(packed, plain)
    raw, signature = signed[:-64], signed[-64:]
    verify(public, raw, signature)
    assert raw == (args.fixture / 'fixture.raw').read_bytes()
    decoded = frames(raw)
    assert decoded == (args.fixture / 'fixture.recording.frames').read_bytes()
    core = b''.join(decoded[at:at + 336] for at in range(0, len(decoded), 396))
    assert core == (args.fixture / 'fixture.frames').read_bytes()
    assert decompress((args.fixture / 'fixture.raw.hlc').read_bytes(), len(raw)) == raw
    for offset in [0, 8, 16, 28, 32, 36, 291, 292, 307, 308, len(data) - 1]:
        altered = bytearray(data)
        altered[offset] ^= 1
        rejected(lambda: decrypt(bytes(altered), private, trust), (AssertionError, ValueError, InvalidTag))
    for altered in (data[:-1], data + b'x'):
        rejected(lambda: decrypt(altered, private, trust), AssertionError)
    wrong_key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    rejected(lambda: decrypt(data, wrong_key, trust), ValueError)
    wrong_signer = ec.generate_private_key(ec.SECP256R1()).public_key()
    rejected(lambda: verify(wrong_signer, raw, signature), InvalidSignature)
    rejected(lambda: verify(public, raw, bytes(64)), InvalidSignature)
    print(f'PASS: {len(sizes)} native crypto sizes; {len(decoded) // 396} complete ticks; signature, GCM, key mismatch and tampering checks')


if __name__ == '__main__':
    main()
