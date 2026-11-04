"""QA de la TUI real en Xvfb/Konsole: píxeles, navegación y limpieza, sin red."""
import ctypes as c
import json
import os
from pathlib import Path
import select
import shlex
import signal
import subprocess
import sys
import tempfile
import time


def check(binary, output):
    binary = Path(binary).resolve()
    output = Path(output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    reader, writer = os.pipe()
    server = subprocess.Popen(
        ["Xvfb", "-displayfd", str(writer), "-screen", "0", "1200x900x24", "-nolisten", "tcp"],
        pass_fds=(writer,), stdout=subprocess.DEVNULL, stderr=(output / "xvfb.log").open("w"),
    )
    os.close(writer)
    child = display = None
    x11 = c.CDLL("libX11.so.6")
    try:
        assert select.select([reader], [], [], 5)[0], "Xvfb no inició"
        address = ":" + os.read(reader, 64).decode().strip()
        os.close(reader)
        with tempfile.TemporaryDirectory(prefix="cbrec-graphics-") as temporary:
            env = os.environ.copy()
            for name in list(env):
                if name.startswith("KONSOLE_DBUS") or name in ("WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS", "CBREC_SESSION_COOKIE", "WINDOWID"):
                    env.pop(name, None)
            env.update(DISPLAY=address, QT_QPA_PLATFORM="xcb", XDG_CONFIG_HOME=temporary,
                       XDG_DATA_HOME=temporary, CBREC_IMAGE_PROTOCOL="auto")
            child = subprocess.Popen([
                "dbus-run-session", "--", "konsole", "--separate", "--builtin-profile",
                "--hide-menubar", "--hide-tabbar", "-e", "script", "-q", "-e", "-c",
                shlex.quote(str(binary)), str(output / "terminal.log"),
            ], env=env, stdout=(output / "konsole.log").open("w"), stderr=subprocess.STDOUT, start_new_session=True)
            x11.XOpenDisplay.argtypes = [c.c_char_p]
            x11.XOpenDisplay.restype = c.c_void_p
            display = x11.XOpenDisplay(address.encode())
            assert display, "No se pudo conectar al display aislado"
            x11.XDefaultRootWindow.argtypes = [c.c_void_p]
            x11.XDefaultRootWindow.restype = c.c_ulong
            root = x11.XDefaultRootWindow(display)
            x11.XQueryTree.argtypes = [c.c_void_p, c.c_ulong, c.POINTER(c.c_ulong), c.POINTER(c.c_ulong), c.POINTER(c.POINTER(c.c_ulong)), c.POINTER(c.c_uint)]
            x11.XFree.argtypes = [c.c_void_p]
            x11.XGetGeometry.argtypes = [c.c_void_p, c.c_ulong, c.POINTER(c.c_ulong), c.POINTER(c.c_int), c.POINTER(c.c_int), c.POINTER(c.c_uint), c.POINTER(c.c_uint), c.POINTER(c.c_uint), c.POINTER(c.c_uint)]
            deadline = time.monotonic() + 5
            window = None
            while not window and time.monotonic() < deadline:
                children = c.POINTER(c.c_ulong)()
                count = c.c_uint()
                returned_root, parent = c.c_ulong(), c.c_ulong()
                x11.XQueryTree(display, root, c.byref(returned_root), c.byref(parent), c.byref(children), c.byref(count))
                for index in range(count.value):
                    x, y = c.c_int(), c.c_int()
                    width, height, border, depth = c.c_uint(), c.c_uint(), c.c_uint(), c.c_uint()
                    x11.XGetGeometry(display, children[index], c.byref(returned_root), c.byref(x), c.byref(y),
                                     c.byref(width), c.byref(height), c.byref(border), c.byref(depth))
                    if width.value > 400 and height.value > 200:
                        window = children[index]
                if children:
                    x11.XFree(children)
                time.sleep(.1)
            assert window, "Konsole no abrió una ventana"
            x11.XResizeWindow.argtypes = [c.c_void_p, c.c_ulong, c.c_uint, c.c_uint]
            x11.XSetInputFocus.argtypes = [c.c_void_p, c.c_ulong, c.c_int, c.c_ulong]
            x11.XFlush.argtypes = [c.c_void_p]
            x11.XMapRaised.argtypes = [c.c_void_p, c.c_ulong]
            x11.XSync.argtypes = [c.c_void_p, c.c_int]
            x11.XResizeWindow(display, window, 1200, 900)
            x11.XMapRaised(display, window)
            x11.XSync(display, 0)
            x11.XSetInputFocus(display, window, 1, 0)
            x11.XFlush(display)
            xtest = c.CDLL("libXtst.so.6")
            xtest.XTestFakeKeyEvent.argtypes = [c.c_void_p, c.c_uint, c.c_int, c.c_ulong]
            xtest.XTestFakeButtonEvent.argtypes = [c.c_void_p, c.c_uint, c.c_int, c.c_ulong]
            xtest.XTestFakeMotionEvent.argtypes = [c.c_void_p, c.c_int, c.c_int, c.c_int, c.c_ulong]
            xtest.XTestFakeMotionEvent(display, -1, 1199, 899, 0)
            x11.XFlush(display)
            x11.XKeysymToKeycode.argtypes = [c.c_void_p, c.c_ulong]
            x11.XKeysymToKeycode.restype = c.c_uint

            def key(symbol):
                code = x11.XKeysymToKeycode(display, symbol)
                xtest.XTestFakeKeyEvent(display, code, 1, 0)
                xtest.XTestFakeKeyEvent(display, code, 0, 0)
                x11.XFlush(display)
                time.sleep(.5)

            def capture(name):
                path = output / (name + ".png")
                subprocess.run(["ffmpeg", "-v", "error", "-f", "x11grab", "-video_size", "1200x900", "-i", address,
                                "-frames:v", "1", "-threads", "1", "-y", str(path)], check=True, capture_output=True, timeout=10)
                return subprocess.run(["ffmpeg", "-v", "error", "-i", str(path), "-pix_fmt", "rgb24", "-f", "rawvideo", "-"],
                                      check=True, capture_output=True, timeout=10).stdout

            def assert_source(screen, second=False):
                source = bytes(component for y in range(270) for x in range(480)
                               for component in ([20, 240 if x % 8 < 4 else 20, y * 255 // 270] if second
                                                 else [240 if x % 8 < 4 else 20, x * 255 // 480, y * 255 // 270]))
                return assert_bitmap(screen, source)

            def assert_bitmap(screen, source):
                start = screen.find(source[:480 * 3])
                assert start >= 0, "No aparece la imagen original a resolución nativa"
                for y in range(270):
                    assert screen[start + y * 1200 * 3:start + y * 1200 * 3 + 480 * 3] == source[y * 480 * 3:(y + 1) * 480 * 3], "La imagen perdió píxeles o quedó cubierta"
                return start

            time.sleep(2)
            home = capture("home")
            xtest.XTestFakeMotionEvent(display, -1, 350, 315, 0)
            xtest.XTestFakeButtonEvent(display, 1, 1, 0)
            xtest.XTestFakeButtonEvent(display, 1, 0, 0)
            xtest.XTestFakeMotionEvent(display, -1, 1199, 899, 0)
            x11.XFlush(display)
            time.sleep(.5)
            assert_source(capture("image"))
            key(0xff54)  # Down
            second = capture("second")
            start = assert_source(second, second=True)
            key(ord("="))
            zoomed = capture("zoom")
            bitmap = b"".join(zoomed[start + y * 1200 * 3:start + y * 1200 * 3 + 480 * 3] for y in range(270))
            assert bitmap[:480 * 3] != second[start:start + 480 * 3], "El zoom no modificó la imagen"
            key(ord("f"))
            assert_bitmap(capture("expanded"), bitmap)
            x11.XResizeWindow(display, window, 1000, 760)
            x11.XFlush(display)
            time.sleep(2)
            assert_bitmap(capture("resized"), bitmap)
            x11.XResizeWindow(display, window, 1200, 900)
            x11.XFlush(display)
            time.sleep(2)
            key(ord("h"))
            restored = capture("home-restored")
            assert home[100 * 1200 * 3:600 * 1200 * 3] == restored[100 * 1200 * 3:600 * 1200 * 3], "Quedaron gráficos al volver a Inicio"
            key(ord("a"))
            wizard = capture("session-browser")
            first_row = bytes(component for x in range(480) for component in [240 if x % 8 < 4 else 20, x * 255 // 480, 0])
            assert first_row not in wizard, "La imagen nativa debe ocultarse bajo el asistente"
            key(ord("2"))
            key(0xff1b)  # Escape cancela el asistente sin cerrar la TUI.
            assert_source(capture("session-cancelled"))
            key(ord("q"))
            assert child.wait(timeout=10) == 0
            transcript = (output / "terminal.log").read_bytes()
            assert b"COMMAND_EXIT_CODE=\"0\"" in transcript
            assert b"\x1b[?1006l" in transcript and b"\x1b[?1049l" in transcript
            # Los portales de esta sesión D-Bus pueden seguir escribiendo su caché temporal.
            try:
                os.killpg(child.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            time.sleep(.2)
            result = {"status": "PASS", "source_pixels_verified": 480 * 270 * 2,
                      "home_restored": True, "session_overlay": True, "mouse_card": True, "resize": True, "mouse_disabled_on_exit": True,
                      "no_color": env.get("NO_COLOR", "")}
            (output / "result.json").write_text(json.dumps(result, indent=2))
            print(json.dumps(result))
    finally:
        if child:
            try:
                os.killpg(child.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
        if child and child.poll() is None:
            child.terminate()
            child.wait(timeout=5)
        if display:
            x11.XCloseDisplay.argtypes = [c.c_void_p]
            x11.XCloseDisplay(display)
        server.terminate()
        server.wait(timeout=5)


if __name__ == "__main__":
    check(sys.argv[1], sys.argv[2])
