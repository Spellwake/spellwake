"""Give Claude compiler feedback after Rust source or manifest edits."""

import json
from pathlib import Path
import subprocess
import sys


def main() -> int:
    """Check the edited package, including independently rooted workspaces."""
    event = json.load(sys.stdin)
    raw_path = event.get("tool_input", {}).get("file_path")
    if not raw_path:
        return 0
    root = Path(__file__).resolve().parents[2]
    path = Path(raw_path)
    if not path.is_absolute():
        path = Path(event.get("cwd", str(root))) / path
    path = path.resolve()
    if not path.is_relative_to(root):
        return 0
    if path.suffix != ".rs" and path.name not in {"Cargo.toml", "Cargo.lock"}:
        return 0
    for directory in path.parents:
        manifest = directory / "Cargo.toml"
        if manifest.is_file():
            try:
                result = subprocess.run(
                    ["cargo", "check", "--locked", "--manifest-path", str(manifest)],
                    cwd=root,
                    capture_output=True,
                    text=True,
                    timeout=110,
                    check=False,
                )
            except (OSError, subprocess.TimeoutExpired) as error:
                print(f"Rust edit check could not finish: {error}", file=sys.stderr)
                return 2
            if result.returncode:
                print(result.stdout + result.stderr, file=sys.stderr)
                return 2
            return 0
        if directory == root:
            break
    return 0


if __name__ == "__main__":
    sys.exit(main())
