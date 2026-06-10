#!/usr/bin/env python3
"""End-to-end smoke test: drive the release binary in a pty.

Simulates: startup render, a mouse drag of the lens, a double-click to open
the settings panel, a click on a panel arrow, and 'q' to quit. Asserts the
app renders half-block pixels, opens the panel, and exits cleanly.
"""
import fcntl
import os
import pty
import select
import struct
import subprocess
import sys
import termios
import time

ROWS, COLS = 40, 120
BIN = os.path.join(os.path.dirname(__file__), "..", "target", "release", "glasstui")


def read_available(fd, duration):
    out = b""
    end = time.time() + duration
    while time.time() < end:
        r, _, _ = select.select([fd], [], [], 0.1)
        if r:
            try:
                out += os.read(fd, 65536)
            except OSError:
                break
    return out


def mouse(fd, col, row, press):
    seq = f"\x1b[<0;{col};{row}{'M' if press else 'm'}"
    os.write(fd, seq.encode())


def mouse_drag(fd, col, row):
    os.write(fd, f"\x1b[<32;{col};{row}M".encode())


def main():
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))
    env = dict(os.environ, TERM="xterm-256color")
    proc = subprocess.Popen(
        [BIN], stdin=slave, stdout=slave, stderr=slave, env=env, close_fds=True
    )
    os.close(slave)
    output = b""
    try:
        output += read_available(master, 1.5)
        assert proc.poll() is None, "app exited prematurely"
        assert "▀".encode() in output, "no half-block pixels rendered"
        assert b"\x1b[?1000h" in output or b"\x1b[?1006h" in output, (
            "mouse capture not enabled"
        )

        # Lens starts at pixel (0.5*W, 0.32*H) -> cell (60, ~12). Drag it.
        mouse(master, 61, 13, True)
        for col in range(62, 75, 3):
            mouse_drag(master, col, 14)
        output += read_available(master, 0.5)  # let a frame render mid-drag
        mouse(master, 74, 14, False)
        output += read_available(master, 0.5)
        assert b"dragging" in output, "drag state never rendered"

        # Double-click the lens at its new position to open settings.
        mouse(master, 74, 14, True)
        mouse(master, 74, 14, False)
        time.sleep(0.1)
        mouse(master, 74, 14, True)
        mouse(master, 74, 14, False)
        output += read_available(master, 0.7)
        assert b"Liquid Glass" in output, "settings panel did not open"
        assert b"Distortion" in output, "params not listed"

        # Click an increase arrow in the panel (panel centered: x=37, y=14;
        # row of first param = y+2 in 1-based terms, INC zone col ~ x+43).
        mouse(master, 37 + 43, 14 + 2, True)
        mouse(master, 37 + 43, 14 + 2, False)
        output += read_available(master, 0.4)

        os.write(master, b"\x1b")  # Esc closes settings
        time.sleep(0.2)
        os.write(master, b"q")  # quit
        for _ in range(30):
            if proc.poll() is not None:
                break
            output += read_available(master, 0.1)
        assert proc.poll() == 0, f"bad exit code: {proc.poll()}"
        # Terminal must be restored: alternate screen left, mouse released.
        assert b"\x1b[?1006l" in output or b"\x1b[?1000l" in output, (
            "mouse capture not released"
        )
        assert b"\x1b[?1049l" in output, "alternate screen not restored"
    finally:
        if proc.poll() is None:
            proc.kill()
        os.close(master)
    print("SMOKE TEST PASSED")


if __name__ == "__main__":
    sys.exit(main())
