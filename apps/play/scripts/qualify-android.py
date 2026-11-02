"""Check native overview touch targets on an explicitly selected Android emulator."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import re
import shutil
import subprocess
import time
import xml.etree.ElementTree as ET


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--serial", required=True)
    parser.add_argument("--screenshots", type=Path)
    arguments = parser.parse_args()
    if not re.fullmatch(r"emulator-\d+", arguments.serial):
        parser.error("Select an emulator serial; this check does not operate personal phones.")
    executable = shutil.which("adb")
    if executable is None and os.environ.get("ANDROID_HOME"):
        executable = str(Path(os.environ["ANDROID_HOME"]) / "platform-tools/adb")
    if executable is None:
        parser.error("Set ANDROID_HOME or add Android platform-tools to PATH.")
    adb = [executable, "-s", arguments.serial]

    def run(*command: str) -> bytes:
        return subprocess.check_output([*adb, *command], timeout=30)

    def nodes() -> list[ET.Element]:
        run("shell", "uiautomator", "dump", "/sdcard/spellwake-qualification.xml")
        document = run("exec-out", "cat", "/sdcard/spellwake-qualification.xml")
        return list(ET.fromstring(document).iter("node"))

    def visible_button(title: str) -> tuple[int, int] | None:
        for node in nodes():
            label = node.get("text") or node.get("content-desc")
            if label != title or node.get("enabled") != "true":
                continue
            bounds = [int(value) for value in re.findall(r"\d+", node.get("bounds", ""))]
            if len(bounds) == 4 and bounds[2] > bounds[0] and bounds[3] > bounds[1]:
                return (bounds[0] + bounds[2]) // 2, (bounds[1] + bounds[3]) // 2
        return None

    def wait_for(title: str) -> tuple[int, int]:
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            location = visible_button(title)
            if location is not None:
                return location
            time.sleep(0.5)
        raise RuntimeError(f"Expected visible, enabled {title!r}; native touch navigation failed.")

    def capture(name: str) -> None:
        if arguments.screenshots is not None:
            arguments.screenshots.mkdir(parents=True, exist_ok=True)
            (arguments.screenshots / f"android-{name}.png").write_bytes(
                run("exec-out", "screencap", "-p")
            )

    rotation = run("shell", "settings", "get", "system", "user_rotation").decode().strip()
    automatic = run("shell", "settings", "get", "system", "accelerometer_rotation").decode().strip()
    run("shell", "settings", "put", "system", "accelerometer_rotation", "0")
    try:
        for direction, orientation in [("portrait", "0"), ("landscape", "1")]:
            run("shell", "settings", "put", "system", "user_rotation", orientation)
            time.sleep(1)
            point = wait_for("Table overview")
            capture(f"{direction}-board")
            run("shell", "input", "tap", str(point[0]), str(point[1]))
            point = wait_for("Return to your board")
            capture(f"{direction}-overview")
            run("shell", "input", "tap", str(point[0]), str(point[1]))
            wait_for("Table overview")
            print(f"PASS: {direction} overview and return via touch-target centers.", flush=True)
    finally:
        run("shell", "settings", "put", "system", "user_rotation", rotation)
        run("shell", "settings", "put", "system", "accelerometer_rotation", automatic)


if __name__ == "__main__":
    main()
