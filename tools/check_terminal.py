#!/usr/bin/env python3
"""Exercise the real terminal binary under a PTY on macOS/Linux."""
import fcntl
import json
import os
from pathlib import Path
import select
import signal
import shutil
import struct
import sys
import tempfile
import termios
import time

ROOT = Path(__file__).resolve().parent.parent
BINARY = Path(os.environ.get('HALLS_BINARY', ROOT / 'target' / 'release' / 'halls')).resolve()


class Session:
    def __init__(self, arguments, binary=None, mute=True, cwd=None):
        executable = BINARY if binary is None else Path(binary).resolve()
        self.master, slave = os.openpty()
        self.original = termios.tcgetattr(self.master)
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 40, 120, 0, 0))
        self.pid = os.fork()
        if self.pid == 0:
            import resource
            resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
            os.setsid()
            fcntl.ioctl(slave, termios.TIOCSCTTY, 0)
            for fd in (0, 1, 2):
                os.dup2(slave, fd)
            os.close(self.master)
            if slave > 2:
                os.close(slave)
            os.environ['TERM'] = 'xterm-256color'
            os.environ['HALLS_MUTE'] = '1' if mute else '0'
            os.chdir(cwd or ROOT)
            os.execv(executable, [str(executable), *map(str, arguments)])
        os.close(slave)
        self.output = bytearray()
        self.done = False

    def read(self, seconds=0.25):
        start = len(self.output)
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            ready, _, _ = select.select([self.master], [], [], max(0, min(0.02, deadline - time.monotonic())))
            if ready:
                try:
                    chunk = os.read(self.master, 65536)
                except OSError:
                    break
                if not chunk:
                    break
                self.output.extend(chunk)
        return bytes(self.output[start:])

    def send(self, keys, seconds=0.25):
        os.write(self.master, keys)
        return self.read(seconds)

    def until(self, marker, timeout=20.0, start=None):
        start = len(self.output) if start is None else start
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            self.read(0.05)
            if marker in self.output[start:]:
                return bytes(self.output[start:])
        raise AssertionError(f'Missing {marker!r}: {bytes(self.output[-500:])!r}')

    def send_until(self, keys, marker):
        start = len(self.output)
        os.write(self.master, keys)
        return self.until(marker, start=start)

    def resize(self, columns, rows):
        fcntl.ioctl(self.master, termios.TIOCSWINSZ, struct.pack('HHHH', rows, columns, 0, 0))
        os.kill(self.pid, signal.SIGWINCH)
        return self.read()

    def finish(self):
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            self.read(0.03)
            pid, status = os.waitpid(self.pid, os.WNOHANG)
            if pid:
                self.done = True
                assert os.waitstatus_to_exitcode(status) == 0, bytes(self.output[-2000:])
                restored = termios.tcgetattr(self.master)
                assert restored[3] & (termios.ECHO | termios.ICANON) == self.original[3] & (termios.ECHO | termios.ICANON)
                assert b'\x1b[?1049l' in self.output and b'\x1b[?25h' in self.output
                return bytes(self.output)
        raise AssertionError('Process did not exit')

    def close(self):
        if not self.done:
            os.kill(self.pid, signal.SIGKILL)
            os.waitpid(self.pid, 0)
        os.close(self.master)


def main():
    if not BINARY.exists():
        raise SystemExit('First run cargo build --release --bin halls')
    session = Session([])
    try:
        session.until(b'NEW GAME')
        session.send(b'\r',0.05)
        first = session.until(b'WASD')
        assert '▀'.encode() in first
        assert b'WALK' in first and b'STAMINA' not in first
        assert b'[ ] vol' in first
        assert b'RUN' in session.send(b'z', 0.2)
        assert b'WALK' in session.send(b'z', 0.2)
        assert b'Shift' not in first or b'WASD' in first
        moved = session.send(b'w' * 8, 0.5)
        assert moved.count(b'\x1b[') > 30
        assert b'\x1b[2J' in session.resize(80, 26)
        assert b'Terminal too small' in session.resize(20, 8)
        assert b'WASD' in session.resize(120, 40)
        start = len(session.output)
        session.send(b'c \thv\x1b[15~\x1b[20~', 0.3)
        changed = bytes(session.output[start:])
        assert not any(marker in changed for marker in [b'CHALK', b'SKETCH', b'SAVED', b'LOADED', b'Arrow keys', b'CPU /'])
        session.send(b'\x1b')
        session.until(b'RESUME') if b'RESUME' not in session.output else None
        assert b'SAVE GAME' not in session.output
        session.send(b'q')
        session.finish()
    finally:
        session.close()
    print('PASS: RGB movement, resize, inactive hotkeys, terminal restoration')
    session = Session([])
    try:
        session.until(b'NEW GAME')
        started = time.monotonic()
        session.send(b'\r',0.001)
        session.until(b'WASD') if b'WASD' not in session.output else None
        assert b'LOADING' not in session.output and b'BUILDING SPACE' not in session.output
        assert time.monotonic()-started < 2.0
        changed = session.send(b'rrr', 0.5)
        assert b'GENERATING' not in changed and b'LOADING' not in changed and b'WASD' in changed, 'R restarted a live game'
        session.send(b'\x1b')
        session.send(b'q')
        session.finish()
    finally:
        session.close()
    print('PASS: direct entry without loading animation; R is ignored during live play')
    tools = Path(os.environ.get('HALLS_TOOLS_BINARY', ROOT / 'target/release/halls-tools')).resolve()
    if not tools.exists():
        raise AssertionError('Build author test fixtures: cargo build --release --features tools --bin halls-tools')
    session = Session(['play', '--dead'], binary=tools)
    try:
        session.until(b'YOU DIED')
        session.read(1.2)
        changed = session.send(b'fzwasdc \thv\x1b[15~\x1b[20~', 0.35)
        assert b'YOU DIED' in changed and b'WASD' not in changed and b'RESUME' not in changed
        session.send(b'\r', 0.15)
        session.until(b'WASD')
        session.send(b'\x1b')
        session.send(b'q')
        session.finish()
    finally:
        session.close()
    session = Session(['play', '--won'], binary=tools)
    try:
        session.until(b'ESCAPED')
        session.read(0.1)
        assert all(text in session.output for text in (b'YOU ESCAPED', b'EXIT FOUND', b'TIME', b'ENTER / ESC  MAIN MENU'))
        changed = session.send(b'rrrfzwasd\x1b[A\x1b[C', 0.3)
        assert b'WASD' not in changed and b'Name your victory proof' in changed
        assert b'rrrfzwasd.hproof' in changed
        assert b'GENERATING' not in changed and b'R      NEW GAME' not in session.output
        session.send(b'\x1b')
        session.until(b'NEW GAME') if b'NEW GAME' not in session.output else None
        session.send(b'q')
        session.finish()
    finally:
        session.close()
    session = Session(['play', '--dead'], binary=tools)
    try:
        session.until(b'YOU DIED')
        session.send(b'r', 0.15)
        session.until(b'WASD')
        session.send(b'\x1b')
        session.send(b'q')
        session.finish()
    finally:
        session.close()
    session = Session(['play', '--won'], binary=tools)
    try:
        session.until(b'ESCAPED')
        session.send(b'\x1b')
        session.until(b'NEW GAME') if b'NEW GAME' not in session.output else None
        assert b'RESUME' not in session.output, 'Victory Escape opened the pause menu'
        session.send(b'q')
        session.finish()
    finally:
        session.close()
    print('PASS: death retry, victory filename entry, export cancellation and menu return')
    # Emulate a Kitty-capable terminal and feed fragmented native key-up packets.
    with tempfile.TemporaryDirectory(prefix='halls-input-') as folder:
        path = Path(folder) / 'open.json'
        bits = [(1 << 64)-1] * ((17*17+63)//64)
        for z in range(1,16):
            for x in range(1,16):bits[(z*17+x)//64] &= ~(1 << ((z*17+x)%64))
        path.write_text(json.dumps({'version':2,'width':17,'height':17,'walls':bits,
                                    'spawn':{'x':17,'z':17,'yaw':0},'exit':[15,15]}))
        session = Session(['play', path], binary=tools)
        try:
            session.until(b'\x1b[?u')
            session.send(b'\x1b[?11u\x1b[?1;2c',0.02)
            first = session.until(b'WASD')
            assert b'\x1b[>11u' in session.output
            session.send(b'\x1b[119;1u',0.15)
            session.send(b'\x1b[1;1C',0.15)
            moved = session.send(b'\x1b[102;1u\x1b[102;1:3u',0.2)
            assert moved.count(b'\x1b[')>30
            session.send(b'\x1b[1;1:3C',0.05)
            moved = session.read(0.15)
            assert moved.count(b'\x1b[')>30, 'Walking stopped when another key was released'
            session.send(b'\x1b[119;1:',0.003)
            session.send(b'3u',0.15)
            paused = session.send(b'\x1b[27;1u')
            assert b'RESUME' in paused, 'Fragmented release blocked subsequent menu input'
            session.send(b'\x1b[113;1u')
            session.finish()
        finally:
            session.close()
    print('PASS: enhanced keyboard negotiation, overlapping movement/lamp keys, fragmented releases and pause')

    session = Session([])
    try:
        session.until(b'NEW GAME')
        session.send_until(b'\x1b[B\r', b'< 1x >')
        for key, scales in [(b'\x1b[C', [2, 4, 8, 16, 32, 64, 64]), (b'\x1b[D', [32, 16, 8, 4, 2, 1, 1])]:
            for scale in scales:
                screen = session.send_until(key, f'< {scale}x >'.encode())
                screen += session.read(0.05)
                assert (b'WARNING: may run slowly' in screen) == (scale >= 4)
        session.send_until(b'\x1b[C', b'< 2x >')
        session.send_until(b'\r', b'NEW GAME')
        session.send_until(b'\r', b'WASD')
        session.send_until(b'\x1b', b'RESUME')
        session.send_until(b'\x1b[B\r', b'< 2x >')
        session.send_until(b'\x1b[C', b'< 4x >')
        session.send_until(b'\x1b', b'RESUME')
        session.send_until(b'\r', b'WASD')
        session.send_until(b'\x1b', b'RESUME')
        session.send_until(b'\x1b[B\r', b'< 4x >')
        session.send_until(b'\x1b', b'RESUME')
        session.send_until(b'\x1b[B\x1b[B\r', b'NEW GAME')
        session.send_until(b'\x1b[B\r', b'< 4x >')
        session.send_until(b'\x1b', b'NEW GAME')
        session.send(b'q')
        session.finish()
    finally:
        session.close()
    print('PASS: 1x-64x settings, bounds, load warning, menu/pause persistence and 2x/4x gameplay')

    fixture = Path(os.environ.get('HALLS_PROOF_FIXTURE_BINARY', BINARY.parent / 'examples/proof_fixture'))
    if fixture.is_file():
        with tempfile.TemporaryDirectory(prefix='halls-export-') as folder:
            folder = Path(folder)
            executable = folder / 'halls-proof-test'
            shutil.copy2(fixture, executable)
            occupied = folder / 'existing.hproof'
            occupied.write_bytes(b'preserve this file')
            session = Session([folder / 'frames', '--prompt'], binary=executable)
            try:
                session.until(b'Name your victory proof')
                assert b'Use a filename, without a path' in session.send(b'../bad\r')
                session.send(b'\x7f' * 6)
                assert b'File exists. Choose another name.' in session.send(b'existing\r')
                session.send(b'\x7f' * 8)
                saved = session.send('proof-λ'.encode() + b'\r', 0.5)
                assert b'VICTORY PROOF SAVED' in saved and b'Saved beside halls-proof-test.' in saved
                assert occupied.read_bytes() == b'preserve this file'
                assert (folder / 'proof-λ.hproof').read_bytes().startswith(b'HLPF\x01\r\n\x1a')
                session.send(b'\r')
                session.finish()
            finally:
                session.close()
        print('PASS: proof naming, path rejection, existing-file retry, Unicode filename and save location')

    panic_binary = os.environ.get('HALLS_PANIC_BINARY')
    if panic_binary:
        session = Session([], binary=panic_binary)
        try:
            session.until(b'HALLS: fatal test')
            _, status = os.waitpid(session.pid, 0)
            session.done = True
            session.read(0.1)
            assert os.waitstatus_to_exitcode(status) != 0
            assert b'\x1b[?1049l' in session.output and b'\x1b[?25h' in session.output
            restored = termios.tcgetattr(session.master)
            assert restored[3] & (termios.ECHO | termios.ICANON) == session.original[3] & (termios.ECHO | termios.ICANON)
        finally:
            session.close()
        print('PASS: no-std fatal exit restores terminal modes, cursor and alternate screen')



if __name__ == '__main__':
    if os.name != 'posix':
        raise SystemExit('The PTY harness runs on macOS and Linux; Windows needs an interactive terminal check.')
    main()
