"""QA de teclado, ratón, resize y restauración en una terminal real simulada (POSIX)."""

import errno
import fcntl
import os
from pathlib import Path
import pty
import re
import select
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time


def screen_text(raw):
    # shortcut: proyecta los CSI usados por Ratatui; ampliar si la QA incorpora otros protocolos.
    rows = [[" "] * 160 for _ in range(64)]
    x = y = 0
    position = 0
    data = raw.decode("utf-8", errors="replace")
    for match in re.finditer(r"\x1b\[([0-9;?]*)([A-Za-z~])|$", data):
        for character in data[position:match.start()]:
            if character == "\n":
                y = min(63, y + 1)
            elif character == "\r":
                x = 0
            elif character.isprintable() and 0 <= x < 160 and 0 <= y < 64:
                rows[y][x] = character
                x += 1
        position = match.end()
        values, command = match.group(1), match.group(2)
        if values is None or values.startswith("?"):
            continue
        numbers = [int(value or 0) for value in values.split(";")]
        if command in ("H", "f"):
            y = max(0, (numbers[0] or 1) - 1)
            x = max(0, (numbers[1] if len(numbers) > 1 and numbers[1] else 1) - 1)
        elif command == "J" and numbers[0] == 2:
            rows = [[" "] * 160 for _ in range(64)]
        elif command == "K" and y < 64:
            rows[y][min(x, 160):] = [" "] * (160 - min(x, 160))
    return "\n".join("".join(row) for row in rows)


def stable_terminal_attributes(attributes):
    # PENDIN es estado transitorio del kernel al volver a modo canónico, no una preferencia.
    result = list(attributes)
    result[3] &= ~getattr(termios, "PENDIN", 0)
    return result


def check(binary):
    binary = str(Path(binary).resolve())
    master, slave = pty.openpty()
    status_reader, status_writer = os.pipe()
    release_reader, release_writer = os.pipe()
    transcript = bytearray()
    child = None

    def read_until(expected=None, seconds=0.3):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            if select.select([master], [], [], 0.03)[0]:
                try:
                    chunk = os.read(master, 65536)
                except OSError as error:
                    if error.errno == errno.EIO:
                        break
                    raise
                if not chunk:
                    break
                transcript.extend(chunk)
                if expected and expected.decode() in screen_text(transcript):
                    return
        if expected:
            raise AssertionError(f"No se mostró {expected!r}; estado={child.poll()}; salida={bytes(transcript[-2500:])!r}")

    def resize(width, height):
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", height, width, 0, 0))
        if child:
            os.killpg(child.pid, signal.SIGWINCH)

    try:
        with tempfile.TemporaryDirectory(prefix="cbrec-tui-pty-") as temporary:
            environment = os.environ.copy()
            environment.update(TERM="xterm-256color", CBREC_CONFIG_DIR=temporary, CBREC_IMAGE_PROTOCOL="text")
            environment.pop("CBREC_SESSION_COOKIE", None)
            environment.pop("CBREC_FFMPEG", None)
            resize(110, 36)

            def attach_terminal():
                os.setsid()
                fcntl.ioctl(0, termios.TIOCSCTTY, 0)

            child = subprocess.Popen(
                [sys.executable, "-c", """
import os, subprocess, sys
status_fd, release_fd = map(int, sys.argv[1:3])
os.write(status_fd, b"ready")
os.read(release_fd, 1)
result = subprocess.run(sys.argv[3:])
os.write(status_fd, str(result.returncode).encode())
os.read(release_fd, 1)
sys.exit(result.returncode)
""", str(status_writer), str(release_reader), binary, "tui"],
                stdin=slave, stdout=slave, stderr=slave, env=environment,
                preexec_fn=attach_terminal, pass_fds=(status_writer, release_reader),
            )
            os.close(status_writer)
            status_writer = None
            os.close(release_reader)
            release_reader = None
            assert select.select([status_reader], [], [], 3)[0], "La sesión PTY no inició"
            assert os.read(status_reader, 32) == b"ready"
            # La referencia corresponde a la sesión ya adjunta, antes de que cbrec cambie sus modos.
            before = termios.tcgetattr(slave)
            os.write(release_writer, b"1")
            read_until(b"Inicio", 5)
            os.write(master, b"/")
            read_until(seconds=0.15)
            os.write(master, b"\x7f" * 6 + b"qr")
            read_until(seconds=0.3)
            assert not select.select([status_reader], [], [], 0)[0], "q al editar debe escribir, no cerrar"
            os.write(master, b"\x1b")
            read_until(seconds=0.2)
            rows = screen_text(bytes(transcript)).splitlines()
            row, line = next((index, line) for index, line in enumerate(rows) if "g Grabaciones" in line)
            column = line.index("g Grabaciones")
            assert b"\x1b[?1006h" in transcript, "La captura de ratón debe estar activa"
            os.write(master, f"\x1b[<0;{column + 1};{row + 1}M\x1b[<0;{column + 1};{row + 1}m".encode())
            read_until(seconds=0.2)
            # Verificar el frame completo: el diff puede reutilizar letras de Inicio.
            resize(111, 36)
            read_until(b"No hay grabaciones activas", 2)
            resize(20, 8)
            read_until(seconds=0.2)
            assert not select.select([status_reader], [], [], 0)[0], "El resize no debe cerrar la TUI"
            resize(110, 36)
            os.write(master, b"h")
            read_until(seconds=0.3)
            # El diff de Ratatui puede omitir letras iguales; resize fuerza un frame completo.
            resize(112, 36)
            read_until(b"Inicio", 3)
            os.write(master, b"q")
            read_until(seconds=0.2)
            assert select.select([status_reader], [], [], 3)[0], "La TUI no terminó"
            assert os.read(status_reader, 32) == b"0", bytes(transcript[-2500:])
            # macOS revoca el PTY al salir su líder; el supervisor vive hasta comprobar termios.
            after = termios.tcgetattr(slave)
            assert stable_terminal_attributes(after) == stable_terminal_attributes(before), f"La TUI debe restaurar los atributos del terminal: antes={before!r}; después={after!r}"
            assert b"\x1b[?1006l" in transcript, "La TUI debe desactivar la captura de ratón"
            os.write(release_writer, b"1")
            assert child.wait(timeout=3) == 0
    finally:
        if child and child.poll() is None:
            os.killpg(child.pid, signal.SIGKILL)
            child.wait()
        for descriptor in (master, slave, status_reader, status_writer, release_reader, release_writer):
            if descriptor is None:
                continue
            try:
                os.close(descriptor)
            except OSError:
                pass
    print("TUI PTY: inicio, edición, clic, grabaciones, resize y restauración PASS")


if __name__ == "__main__":
    if os.name != "posix" or len(sys.argv) != 2:
        sys.exit("Uso en POSIX: python3 scripts/test_tui_pty.py <binario-cbrec>")
    assert "No hay grabaciones activas" in screen_text(b"\x1b[2;1HNo\x1b[2;4Hhay\x1b[2;8Hgrabaciones activas")
    assert "Inicio" not in screen_text(b"Inicio\x1b[1;1HSala  ")
    baseline = [0, 0, 0, termios.ECHO | termios.ICANON | termios.ISIG, 0, 0, []]
    pending = list(baseline)
    pending[3] |= getattr(termios, "PENDIN", 0)
    assert stable_terminal_attributes(pending) == stable_terminal_attributes(baseline)
    for flag in (termios.ECHO, termios.ICANON, termios.ISIG):
        changed = list(baseline)
        changed[3] &= ~flag
        assert stable_terminal_attributes(changed) != stable_terminal_attributes(baseline)
    check(sys.argv[1])
