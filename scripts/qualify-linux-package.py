"""Exercise an actual packaged Tauri app through its native Linux WebDriver."""

from __future__ import annotations

import argparse
import base64
import http.client
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

    def read_command(
        self, method: str, path: str, data: dict[str, object] | None = None
    ) -> object:
        """Retry a dropped connection only for a read-only WebDriver probe."""
        for delay in (0.25, 0.5):
            try:
                return self.command(method, path, data)
            except http.client.RemoteDisconnected:
                time.sleep(delay)
        return self.command(method, path, data)

    def evaluate(self, script: str) -> object:
        return self.read_command("POST", "/execute/sync", {"script": script, "args": []})

    def screenshot(self, path: Path) -> None:
        encoded = self.read_command("GET", "/screenshot")
        if not isinstance(encoded, str):
            raise TypeError("WebDriver did not return an image")
        png = base64.b64decode(encoded, validate=True)
        if not png.startswith(b"\x89PNG\r\n\x1a\n") or len(png) < 10_000:
            raise AssertionError("Screenshot is missing or unexpectedly small")
        path.write_bytes(png)


def wait_for(check: Callable[[], bool], description: str, seconds: int = 45) -> None:
    """Poll a bounded readiness signal instead of assuming that process survival means ready."""
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if check():
            return
        time.sleep(0.25)
    raise AssertionError(f"Timed out waiting for {description}")


SNAPSHOT = """
const ready = document.querySelector('[data-frame], [data-entry-card]');
const styled = ready ? getComputedStyle(ready) : null;
return {
  runtime: document.documentElement.dataset.runtime ?? null,
  version: document.documentElement.dataset.nativeVersion ?? null,
  ready: ready?.getAttribute('data-frame') ?? (ready ? 'entry' : null),
  stylesApplied: Boolean(styled && styled.display !== 'inline' && document.styleSheets.length > 0),
  width: document.documentElement.clientWidth,
  height: document.documentElement.clientHeight,
  overflowX: document.documentElement.scrollWidth > document.documentElement.clientWidth + 1,
};
"""


def qualify(
    application: Path, output: Path, version: str, application_args: list[str]
) -> None:
    """Verify the bridge, the application's first screen, its styles and resize in the release binary."""
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
        # The application opens on its frame, or on the entry card when signed out.
        wait_for(
            lambda: (
                session.evaluate(
                    "return Boolean(document.querySelector('[data-frame], [data-entry-card]')) && document.documentElement.dataset.nativeVersion !== undefined;"
                )
                is True
            ),
            "packaged application readiness",
        )
        snapshot = session.evaluate(SNAPSHOT)
        if not isinstance(snapshot, dict):
            raise TypeError("Invalid application readiness snapshot")
        evidence["first"] = snapshot
        if snapshot.get("runtime") != "tauri" or snapshot.get("version") != version:
            raise AssertionError("Native bridge/version check failed")
        if snapshot.get("stylesApplied") is not True:
            raise AssertionError("Native content security policy blocked styles")
        if snapshot.get("overflowX") is True:
            raise AssertionError("First screen overflows the window sideways")
        session.screenshot(output / "first.png")
        session.command("POST", "/window/rect", {"width": 1024, "height": 768})
        wait_for(
            lambda: (
                session.evaluate(
                    "return document.documentElement.clientWidth <= 1024 && document.documentElement.clientWidth >= 900 && Boolean(document.querySelector('[data-frame], [data-entry-card]'));"
                )
                is True
            ),
            "window resize",
        )
        resized = session.evaluate(SNAPSHOT)
        if not isinstance(resized, dict) or resized.get("overflowX") is True:
            raise AssertionError("Resized screen overflows the window sideways")
        evidence["resized"] = resized
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


def qualify_with_transport_retry(
    application: Path, output: Path, version: str, application_args: list[str]
) -> None:
    """Repeat a disconnected package exercise with a fresh native session."""
    try:
        qualify(application, output, version, application_args)
    except http.client.RemoteDisconnected:
        # A click may already have been delivered. Discard the session rather
        # than repeating any individual command that changes application state.
        first_attempt = output.with_name(f"{output.name}-webdriver-disconnect")
        if first_attempt.exists():
            raise
        output.rename(first_attempt)
        qualify(application, output, version, application_args)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--application", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--extract-and-run", action="store_true")
    arguments = parser.parse_args()
    qualify_with_transport_retry(
        arguments.application,
        arguments.output,
        arguments.version,
        ["--appimage-extract-and-run"] if arguments.extract_and_run else [],
    )
