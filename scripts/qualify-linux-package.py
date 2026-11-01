"""Exercise an actual packaged Tauri app through its native Linux WebDriver."""

from __future__ import annotations

import argparse
import base64
import json
import os
import subprocess
import time
import urllib.error
import urllib.request
from collections.abc import Callable
from pathlib import Path


class NativeSession:
    """A small W3C WebDriver client; no plugin or test server is added to the app."""

    def __init__(self) -> None:
        self.session_id = ""

    def request(
        self, method: str, path: str, data: dict[str, object] | None = None
    ) -> object:
        body = json.dumps(data).encode() if data is not None else None
        request = urllib.request.Request(
            f"http://127.0.0.1:4444{path}",
            data=body,
            method=method,
            headers={"Content-Type": "application/json"},
        )
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                envelope = json.load(response)
        except urllib.error.HTTPError as error:
            raise AssertionError(
                f"WebDriver {method} {path}: {error.read().decode()}"
            ) from error
        if not isinstance(envelope, dict) or "value" not in envelope:
            raise AssertionError("Invalid WebDriver response envelope")
        return envelope["value"]

    def command(
        self, method: str, path: str, data: dict[str, object] | None = None
    ) -> object:
        return self.request(method, f"/session/{self.session_id}{path}", data)

    def evaluate(self, script: str) -> object:
        return self.command("POST", "/execute/sync", {"script": script, "args": []})

    def screenshot(self, path: Path) -> None:
        encoded = self.command("GET", "/screenshot")
        if not isinstance(encoded, str):
            raise TypeError("WebDriver did not return an image")
        png = base64.b64decode(encoded, validate=True)
        if not png.startswith(b"\x89PNG\r\n\x1a\n") or len(png) < 10_000:
            raise AssertionError("Screenshot is missing or unexpectedly small")
        path.write_bytes(png)

    def click_overview(self) -> None:
        wait_for(
            lambda: (
                self.evaluate(
                    "return document.querySelector('.table-overview-toggle')?.disabled === false;"
                )
                is True
            ),
            "camera travel and navigation availability",
        )
        element = self.command(
            "POST",
            "/element",
            {"using": "css selector", "value": ".table-overview-toggle"},
        )
        if not isinstance(element, dict):
            raise TypeError("Overview button is missing")
        element_id = element.get("element-6066-11e4-a52e-4f735466cecf")
        if not isinstance(element_id, str):
            raise TypeError("Invalid WebDriver element")
        self.command("POST", f"/element/{element_id}/click", {})


def wait_for(check: Callable[[], bool], description: str, seconds: int = 45) -> None:
    """Poll a bounded readiness signal instead of assuming that process survival means ready."""
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if check():
            return
        time.sleep(0.25)
    raise AssertionError(f"Timed out waiting for {description}")


SNAPSHOT = """
const canvas = document.querySelector('#play-canvas');
const gl = canvas?.getContext('webgl2');
const overview = document.querySelector('.table-overview');
const toggle = document.querySelector('.table-overview-toggle');
const bounds = toggle?.getBoundingClientRect();
return {
  runtime: document.documentElement.dataset.runtime ?? null,
  version: document.documentElement.dataset.nativeVersion ?? null,
  status: document.querySelector('#play-status')?.textContent ?? null,
  width: canvas?.width ?? 0,
  height: canvas?.height ?? 0,
  webgl2: typeof WebGL2RenderingContext !== 'undefined' && gl instanceof WebGL2RenderingContext && !gl.isContextLost(),
  renderer: gl ? gl.getParameter(gl.RENDERER) : null,
  overview: document.querySelector('.table-overview-toggle')?.getAttribute('aria-pressed') ?? null,
  overviewPosition: overview ? getComputedStyle(overview).position : null,
  overviewStylesApplied: Boolean(overview?.querySelector('style')?.sheet),
  toggleBounds: bounds ? {left: bounds.left, top: bounds.top, width: bounds.width, height: bounds.height} : null,
};
"""


def qualify(
    application: Path, output: Path, version: str, application_args: list[str]
) -> None:
    """Verify bridge, renderer, view navigation and resize in the unmodified release binary."""
    output.mkdir(parents=True, exist_ok=True)
    environment = dict(os.environ)
    environment["XDG_DATA_HOME"] = str(output.resolve() / "profile")
    session = NativeSession()
    evidence: dict[str, object] = {
        "application": str(application),
        "arguments": application_args,
    }
    driver_log = (output / "webdriver.log").open("w")
    driver = subprocess.Popen(
        ["tauri-driver"], env=environment, stdout=driver_log, stderr=subprocess.STDOUT
    )
    try:

        def driver_ready() -> bool:
            if driver.poll() is not None:
                raise AssertionError("Native driver exited before becoming ready")
            try:
                session.request("GET", "/status")
                return True
            except urllib.error.URLError:
                return False

        wait_for(driver_ready, "native driver")
        value = session.request(
            "POST",
            "/session",
            {
                "capabilities": {
                    "alwaysMatch": {
                        "browserName": "wry",
                        "tauri:options": {
                            "application": str(application.resolve()),
                            "args": application_args,
                        },
                    }
                },
            },
        )
        if not isinstance(value, dict) or not isinstance(value.get("sessionId"), str):
            raise TypeError("Invalid native session response")
        session.session_id = value["sessionId"]
        session.command("POST", "/window/rect", {"width": 1440, "height": 960})
        wait_for(
            lambda: (
                session.evaluate(
                    "return document.querySelector('#play-status')?.textContent === 'Table preview ready';"
                )
                is True
            ),
            "packaged tabletop readiness",
        )
        snapshot = session.evaluate(SNAPSHOT)
        if not isinstance(snapshot, dict):
            raise TypeError("Invalid tabletop readiness snapshot")
        evidence["board"] = snapshot
        if snapshot.get("runtime") != "tauri" or snapshot.get("version") != version:
            raise AssertionError("Native bridge/version check failed")
        if snapshot.get("webgl2") is not True:
            raise AssertionError("WebGL2 context is unavailable or lost")
        if (
            snapshot.get("overviewPosition") != "absolute"
            or snapshot.get("overviewStylesApplied") is not True
        ):
            raise AssertionError(
                "Native content security policy blocked overview styles"
            )
        session.screenshot(output / "board.png")
        session.click_overview()
        wait_for(
            lambda: (
                session.evaluate(
                    "const toggle = document.querySelector('.table-overview-toggle'); return toggle?.getAttribute('aria-pressed') === 'true' && !toggle.disabled;"
                )
                is True
            ),
            "overview navigation",
        )
        session.screenshot(output / "overview.png")
        session.click_overview()
        wait_for(
            lambda: (
                session.evaluate(
                    "const toggle = document.querySelector('.table-overview-toggle'); return toggle?.getAttribute('aria-pressed') === 'false' && !toggle.disabled;"
                )
                is True
            ),
            "return to board",
        )
        session.command("POST", "/window/rect", {"width": 1024, "height": 768})
        wait_for(
            lambda: (
                session.evaluate(
                    "const c = document.querySelector('#play-canvas'); return c.width > 0 && c.clientWidth <= 1024 && c.clientWidth >= 900 && document.querySelector('.table-overview-toggle')?.disabled === false;"
                )
                is True
            ),
            "canvas resize",
        )
        evidence["resized"] = session.evaluate(SNAPSHOT)
        session.screenshot(output / "resized.png")
        evidence["result"] = "passed"
    except Exception as error:
        evidence["result"] = "failed"
        evidence["error"] = str(error)
        if session.session_id:
            try:
                evidence["failureSnapshot"] = session.evaluate(SNAPSHOT)
                session.screenshot(output / "failure.png")
            except (AssertionError, TypeError, ValueError, OSError) as capture_error:
                print(
                    f"Could not capture failure evidence: {capture_error}", flush=True
                )
        raise
    finally:
        (output / "result.json").write_text(json.dumps(evidence, indent=2) + "\n")
        if session.session_id:
            try:
                session.command("DELETE", "")
            except (AssertionError, TypeError, ValueError, OSError) as cleanup_error:
                print(f"Could not close native session: {cleanup_error}", flush=True)
        driver.terminate()
        try:
            driver.wait(timeout=10)
        except subprocess.TimeoutExpired:
            driver.kill()
            driver.wait()
        driver_log.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--application", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--extract-and-run", action="store_true")
    arguments = parser.parse_args()
    qualify(
        arguments.application,
        arguments.output,
        arguments.version,
        ["--appimage-extract-and-run"] if arguments.extract_and_run else [],
    )
