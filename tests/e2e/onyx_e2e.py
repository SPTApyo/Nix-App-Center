"""End-to-end tests for Onyx, driven through GTK's Broadway backend.

Broadway renders the real GTK app into a web page, which Playwright drives
with real mouse and keyboard events. Assertions check the real side effects
(manifest, generated Nix files, patched configuration) in a throwaway
environment, and every step saves a screenshot for visual review.

Run with tests/e2e/run.sh.
"""
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from playwright.sync_api import sync_playwright

ROOT = Path(__file__).resolve().parents[2]
DISPLAY = os.environ.get("ONYX_E2E_DISPLAY", ":7")
PORT = 8080 + int(DISPLAY.lstrip(":"))
URL = f"http://127.0.0.1:{PORT}/"
VIEWPORT = {"width": 1100, "height": 760}


class Session:
    """A Playwright page connected to the Broadway display."""

    def __init__(self, out_dir=None):
        self.out_dir = Path(out_dir) if out_dir else None
        self._pw = sync_playwright().start()
        self._browser = self._pw.chromium.launch()
        self.page = self._browser.new_page(viewport=VIEWPORT)
        self.page.goto(URL)
        self.page.wait_for_timeout(1500)
        self.step = 0

    @classmethod
    def attach(cls):
        return cls()

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self._browser.close()
        self._pw.stop()

    def do(self, cmd):
        kind, _, arg = cmd.partition(":")
        mouse, keyboard = self.page.mouse, self.page.keyboard
        if kind == "click":
            x, y = map(int, arg.split(","))
            mouse.click(x, y)
        elif kind == "type":
            keyboard.type(arg, delay=40)
        elif kind == "key":
            keyboard.press(arg)
        elif kind == "wait":
            self.page.wait_for_timeout(int(arg))
        elif kind == "scroll":
            dy, pos = arg.split("@")
            x, y = map(int, pos.split(","))
            mouse.move(x, y)
            for _ in range(abs(int(dy)) // 100):
                mouse.wheel(0, 100 if int(dy) > 0 else -100)
                self.page.wait_for_timeout(60)
        else:
            raise ValueError(f"unknown command {cmd!r}")
        self.page.wait_for_timeout(300)

    def shot(self, name):
        self.page.wait_for_timeout(500)
        path = Path(name) if self.out_dir is None else self.out_dir / f"{self.step:02}-{name}.png"
        self.step += 1
        self.page.screenshot(path=str(path))
        return path


def wait_for(predicate, timeout, what):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.5)
    raise AssertionError(f"timed out waiting for {what}")


class Environment:
    """Throwaway XDG dirs + a fake NixOS layout, then broadwayd + onyx."""

    def __init__(self, binary):
        self.tmp = Path(tempfile.mkdtemp(prefix="onyx-e2e-"))
        self.data = self.tmp / "data"
        self.config = self.tmp / "nixos"
        self.config.mkdir(parents=True)
        (self.config / "configuration.nix").write_text(
            "{ pkgs, ... }:\n{\n  imports = [\n    ./hardware-configuration.nix\n  ];\n}\n"
        )
        cache = Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
        self.env = dict(
            os.environ,
            GDK_BACKEND="broadway",
            BROADWAY_DISPLAY=DISPLAY,
            LANGUAGE=os.environ.get("ONYX_E2E_LANG", "en"),
            LANG="en_US.UTF-8",
            XDG_DATA_HOME=str(self.data),
            # Reuse the real cache: AppStream download and nix search results.
            XDG_CACHE_HOME=str(cache),
            ONYX_CONFIG_DIR=str(self.config),
        )
        # Child processes (terminals, apps started by Run) must never reach the
        # real desktop session.
        for var in ("WAYLAND_DISPLAY", "DISPLAY"):
            self.env.pop(var, None)
        broadwayd = shutil.which("gtk4-broadwayd", path=self.env["PATH"])
        if broadwayd is None:
            sys.exit("gtk4-broadwayd not found in PATH")
        self.broadway = subprocess.Popen([broadwayd, DISPLAY], env=self.env,
                                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        time.sleep(1)
        if self.broadway.poll() is not None:
            sys.exit(f"gtk4-broadwayd {DISPLAY} did not start (display already in use?)")
        self.log = open(self.tmp / "onyx.log", "w")
        # Private bus: a running Onyx elsewhere would swallow the launch.
        self.app = subprocess.Popen(["dbus-run-session", "--", binary], env=self.env,
                                    stdout=self.log, stderr=self.log, start_new_session=True)

    def kept(self):
        path = self.data / "onyx" / "kept.json"
        return [e["attr"] for e in json.loads(path.read_text())["kept"]] if path.exists() else []

    def tried(self):
        path = self.data / "onyx" / "tried.json"
        return path.read_text() if path.exists() else ""

    def close(self):
        os.killpg(self.app.pid, signal.SIGTERM)
        self.broadway.terminate()
        self.app.wait(timeout=10)
        self.broadway.wait(timeout=10)
        self.log.close()


# Window geometry at 1100×760 (Broadway draws the window at its default size).
SEARCH_TOGGLE = "click:47,48"
BACK = "click:47,48"
FIRST_RESULT = "click:400,230"
TAB_INSTALLED = "click:505,48"
# Installed tab: Remove buttons of the first kept and first tried rows.
KEPT_REMOVE = "click:846,159"
TRIED_REMOVE = "click:846,304"
TAB_UPDATES = "click:627,48"
# Details page of a CLI tool ("Try in a Shell" primary): menu arrow and entries.
MENU = "click:895,145"
MENU_KEEP_TOGGLE = "click:894,221"
MENU_RUN = "click:834,259"
ATTACH_ACCEPT = "click:764,148"


def scenario(env, s):
    """search → details → run → keep → attach → installed → remove."""
    s.do("wait:8000")
    s.shot("home")

    s.do(SEARCH_TOGGLE)
    s.do("type:hello")
    s.do("key:Enter")
    s.do("wait:8000")
    s.shot("search-hello")

    s.do(FIRST_RESULT)
    s.do("wait:6000")
    s.shot("details-hello")

    s.do(MENU)
    s.shot("actions-menu")
    s.do(MENU_RUN)
    wait_for(lambda: '"hello"' in env.tried(), 120, "hello in tried.json")
    s.shot("after-run")

    s.do(MENU)
    s.do(MENU_KEEP_TOGGLE)
    wait_for(lambda: "hello" in env.kept(), 10, "hello in kept.json")
    s.do("wait:1000")
    s.shot("attach-dialog")
    home = env.config / "onyx-home.nix"
    site = env.config / "configuration.nix"
    assert "hello" in home.read_text(), "onyx-home.nix must list hello"
    assert "home.packages" not in home.read_text(), "NixOS without home-manager has no home.packages"
    assert "onyx-home.nix" not in site.read_text(), "configuration.nix must stay untouched before consent"

    s.do(ATTACH_ACCEPT)
    wait_for(lambda: "./onyx-home.nix" in site.read_text(), 10, "import line in configuration.nix")
    assert (env.config / "configuration.nix.onyx-bak").exists(), "a backup must be written first"
    s.shot("attached")

    s.do(BACK)
    s.do(TAB_INSTALLED)
    s.do("wait:5000")
    s.shot("installed")

    s.do(KEPT_REMOVE)
    wait_for(lambda: "hello" not in env.kept(), 10, "hello removed from kept.json")
    assert "hello" not in home.read_text(), "onyx-home.nix must drop hello"
    s.shot("removed")

    # The banner pushed the list down.
    s.do(TRIED_REMOVE)
    wait_for(lambda: '"hello"' not in env.tried(), 180, "hello removed from tried.json")
    s.do("wait:1000")
    s.shot("tried-removed")

    s.do(TAB_UPDATES)
    s.do("wait:60000")
    s.shot("updates")


def main():
    serve = "--serve" in sys.argv
    args = [a for a in sys.argv[1:] if a != "--serve"]
    binary = args[0] if args else str(ROOT / "target/debug/onyx")
    out = Path(os.environ.get("ONYX_E2E_OUT", ROOT / "target/e2e"))
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    env = Environment(binary)
    if serve:
        # Keep the isolated app running for manual exploration (session.py).
        print(f"e2e: serving {URL}, data in {env.tmp}", flush=True)
        signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
        try:
            env.app.wait()
        finally:
            env.close()
        return
    try:
        with Session(out) as s:
            scenario(env, s)
        print(f"e2e: OK, screenshots in {out}")
    finally:
        env.close()
        print(f"e2e: app log in {env.tmp / 'onyx.log'}")


if __name__ == "__main__":
    main()
