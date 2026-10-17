"""End-to-end tests for Nix App Center, driven through GTK's Broadway backend.

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
DISPLAY = os.environ.get("NIX_APP_CENTER_E2E_DISPLAY", ":7")
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

    def __exit__(self, exc_type, *_):
        try:
            self._browser.close()
            self._pw.stop()
        except Exception:
            if exc_type is None:
                raise

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
    """Throwaway XDG dirs + a fake NixOS layout, then broadwayd + nix-app-center."""

    def __init__(self, binary):
        self.tmp = Path(tempfile.mkdtemp(prefix="nix-app-center-e2e-"))
        self.data = self.tmp / "data"
        self.config = self.tmp / "nixos"
        self.config.mkdir(parents=True)
        (self.config / "configuration.nix").write_text(
            "{ pkgs, ... }:\n{\n  imports = [\n    ./hardware-configuration.nix\n  ];\n}\n"
        )
        # Fake programs: they record how Nix App Center calls them, so the actions can
        # be checked without a real terminal, root rights or a rebuild.
        self.bin = self.tmp / "bin"
        self.bin.mkdir()
        # pkexec gets "<env> PATH=<system dirs> cmd…": the fake drops the first
        # two so the other fakes, not the real tools, answer.
        fakes = {
            "xdg-terminal-exec": 'echo "$@" >> "{log}/terminal.log"',
            "pkexec": 'echo "$@" >> "{log}/pkexec.log"; shift 2; exec "$@"',
            "nixos-rebuild": 'echo "$@" >> "{log}/rebuild.log"; echo "building the system configuration..." >&2; '
            + "".join(
                f"echo '@nix {line}' >&2; sleep 0.4; "
                for line in [
                    '{{"action":"start","id":1,"level":0,"parent":0,"text":"","type":103}}',
                    '{{"action":"result","fields":[1,8,0,0],"id":1,"type":105}}',
                    '{{"action":"result","fields":[3,8,0,0],"id":1,"type":105}}',
                    '{{"action":"result","fields":[5,8,0,0],"id":1,"type":105}}',
                    '{{"action":"result","fields":[7,8,0,0],"id":1,"type":105}}',
                ]
            )
            + 'echo "activating the configuration..." >&2',
        }
        for name, body in fakes.items():
            fake = self.bin / name
            fake.write_text("#!/bin/sh\n" + body.format(log=self.tmp) + "\n")
            fake.chmod(0o755)
        cache = Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
        self.env = dict(
            os.environ,
            GDK_BACKEND="broadway",
            BROADWAY_DISPLAY=DISPLAY,
            LANGUAGE=os.environ.get("NIX_APP_CENTER_E2E_LANG", "en"),
            LANG="en_US.UTF-8",
            XDG_DATA_HOME=str(self.data),
            XDG_CONFIG_HOME=str(self.tmp / "config"),
            # Reuse the real cache: AppStream download and nix search results.
            XDG_CACHE_HOME=str(cache),
            NIX_APP_CENTER_CONFIG_DIR=str(self.config),
            PATH=f"{self.bin}:{os.environ['PATH']}",
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
        self.log = open(self.tmp / "nix-app-center.log", "w")
        # Private bus: a running Nix App Center elsewhere would swallow the launch.
        self.app = subprocess.Popen(["dbus-run-session", "--", binary], env=self.env,
                                    stdout=self.log, stderr=self.log, start_new_session=True)

    def kept(self):
        path = self.data / "nix-app-center" / "kept.json"
        return [e["attr"] for e in json.loads(path.read_text())["kept"]] if path.exists() else []

    def logged(self, name):
        path = self.tmp / f"{name}.log"
        return path.read_text() if path.exists() else ""

    def tried(self):
        path = self.data / "nix-app-center" / "tried.json"
        return path.read_text() if path.exists() else ""

    def close(self):
        os.killpg(self.app.pid, signal.SIGTERM)
        self.broadway.terminate()
        self.app.wait(timeout=10)
        self.broadway.wait(timeout=10)
        self.log.close()


# Window geometry at 1100×760 (Broadway draws the window at its default size).
TOUR_SKIP = "click:196,659"
SEARCH_TOGGLE = "click:47,48"
BACK = "click:47,48"
FIRST_RESULT = "click:600,170"
TAB_INSTALLED = "click:443,48"
# Installed tab: Remove buttons of the first kept and first tried rows.
KEPT_REMOVE = "click:846,159"
TRIED_REMOVE = "click:846,304"
TAB_UPDATES = "click:566,48"
TAB_STORAGE = "click:690,48"
# Details page of a CLI tool ("Try in a Shell" primary): menu arrow and entries.
MENU = "click:895,145"
MENU_KEEP_TOGGLE = "click:894,221"
MENU_RUN = "click:834,259"
MENU_COPY = "click:834,297"
MENU_ADVANCED = "click:834,335"
VERSION_TOP = "click:504,248"
# Install As… is hidden once the app has its own block: Apply moves up.
MENU_APPLY = "click:834,378"
# Version section of the details page.
VERSION_CHOOSE = "click:494,669"
# Search results with an option only, and its dialog.
FIRST_OPTION = "click:500,187"
OPTION_SET = "click:504,586"
PRIMARY = "click:820,145"


# Advanced options editor.
EDITOR_SYSTEM_TAB = "click:566,147"
EDITOR_TEXT = "click:500,300"
EDITOR_SAVE = "click:783,147"
ATTACH_ACCEPT = "click:764,148"


def scenario(env, s):
    """search → details → run → keep → attach → installed → remove."""
    s.do("wait:8000")
    s.shot("welcome")
    s.do(TOUR_SKIP)
    settings = env.tmp / "config" / "nix-app-center" / "settings.json"
    wait_for(lambda: '"onboarded": true' in settings.read_text(), 10, "tour remembered")
    s.do("wait:800")
    s.shot("home")

    s.do(SEARCH_TOGGLE)
    s.do("type:hello")
    s.do("key:Enter")
    s.do("wait:8000")
    s.shot("search-hello")

    s.do(FIRST_RESULT)
    s.do("wait:15000")
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
    home = env.config / "nix-app-center-home.nix"
    site = env.config / "configuration.nix"
    assert "hello" in home.read_text(), "nix-app-center-home.nix must list hello"
    assert "home.packages" not in home.read_text(), "NixOS without home-manager has no home.packages"
    assert "nix-app-center-home.nix" not in site.read_text(), "configuration.nix must stay untouched before consent"

    s.do(ATTACH_ACCEPT)
    wait_for(lambda: "./nix-app-center-home.nix" in site.read_text(), 10, "import line in configuration.nix")
    assert (env.config / "configuration.nix.nix-app-center-bak").exists(), "a backup must be written first"
    s.shot("attached")

    actions(env, s)

    s.do(BACK)
    s.do(TAB_INSTALLED)
    s.do("wait:5000")
    s.shot("installed")

    s.do(KEPT_REMOVE)
    wait_for(lambda: "hello" not in env.kept(), 10, "hello removed from kept.json")
    assert "hello" not in home.read_text(), "nix-app-center-home.nix must drop hello"
    s.shot("removed")

    # The banner pushed the list down.
    s.do(TRIED_REMOVE)
    wait_for(lambda: '"hello"' not in env.tried(), 180, "hello removed from tried.json")
    s.do("wait:1000")
    s.shot("tried-removed")

    s.do(TAB_UPDATES)
    s.do("wait:60000")
    s.shot("updates")

    s.do(TAB_STORAGE)
    s.do("wait:30000")
    s.shot("storage")


def actions(env, s):
    """Every button of a details page, checked through what it really runs."""
    # Primary of a command line tool: a terminal with the package.
    s.do(PRIMARY)
    wait_for(lambda: "hello" in env.logged("terminal"), 20, "terminal opened")
    call = env.logged("terminal")
    assert "shell" in call and "#hello" in call, f"nix shell expected, got {call}"
    assert "NIXPKGS_ALLOW_UNFREE=1" in call, "unfree packages must work in a shell"
    assert "INSECURE" not in call, "insecure packages stay refused"
    s.shot("action-shell")

    s.do(MENU)
    s.do(MENU_COPY)
    s.do("wait:500")
    s.shot("action-copy")

    # Advanced options: a system block lands in nix-app-center-system.nix.
    s.do(MENU)
    s.do(MENU_ADVANCED)
    s.do("wait:1000")
    s.do(EDITOR_SYSTEM_TAB)
    s.shot("action-editor-empty-system")
    s.do(EDITOR_TEXT)
    s.do("type:networking.firewall.allowedTCPPorts = [ 8080 ];")
    s.do("wait:2000")
    s.shot("action-editor-typed")
    s.do(EDITOR_SAVE)
    system = env.config / "nix-app-center-system.nix"
    wait_for(lambda: system.exists() and "8080" in system.read_text(), 10, "system block written")

    # Apply: elevated through pkexec, never through a password prompt in a tty.
    s.do(MENU)
    s.do(MENU_APPLY)
    wait_for(lambda: "nixos-rebuild" in env.logged("pkexec"), 20, "apply through pkexec")
    s.shot("action-apply-progress")
    assert "switch" in env.logged("rebuild"), "apply must switch"
    assert "--log-format internal-json" in env.logged("rebuild"), "apply must ask for progress"
    s.do("wait:1000")
    s.shot("action-apply")
    s.do("key:Escape")
    s.do("wait:500")

    home = env.config / "nix-app-center-home.nix"
    manifest = env.data / "nix-app-center" / "kept.json"
    s.do(VERSION_CHOOSE)
    s.do("wait:8000")
    s.shot("action-versions")
    s.do(VERSION_TOP)
    wait_for(lambda: '"pin"' in manifest.read_text(), 180, "hello held in kept.json")
    wait_for(lambda: "builtins.fetchTarball" in home.read_text(), 10, "held revision imported")
    assert ".hello" in home.read_text(), "hello must come from the held revision"
    s.do("wait:1500")
    s.shot("action-hold")
    s.do(VERSION_CHOOSE)
    s.do("wait:8000")
    s.do(VERSION_TOP)
    wait_for(lambda: '"pin"' not in manifest.read_text(), 10, "hold released")
    assert "fetchTarball" not in home.read_text(), "a release drops the held import"

    # Options: found by search, set in the system file.
    s.do(BACK)
    s.do("wait:800")
    s.do(SEARCH_TOGGLE)
    s.do("key:Control+a")
    s.do("type:services.printing.enable")
    s.do("key:Enter")
    s.do("wait:15000")
    s.shot("option-search")
    s.do(FIRST_OPTION)
    s.do("wait:2000")
    s.shot("option-dialog")
    s.do(OPTION_SET)
    wait_for(lambda: "services.printing.enable = true;" in system.read_text(), 10, "option set")
    s.do("key:Escape")


def main():
    serve = "--serve" in sys.argv
    args = [a for a in sys.argv[1:] if a != "--serve"]
    binary = args[0] if args else str(ROOT / "target/debug/nix-app-center")
    out = Path(os.environ.get("NIX_APP_CENTER_E2E_OUT", ROOT / "target/e2e"))
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
        print(f"e2e: app log in {env.tmp / 'nix-app-center.log'}")


if __name__ == "__main__":
    main()
