"""Export the pinned flat engine vocabulary from the checked-in identity fixture."""
import argparse
import hashlib
import json
from pathlib import Path

SOURCE = Path(__file__).resolve().parents[2] / "crates/content/tests/fixtures/creature-vocabulary.input.json"
EXPECTED_SHA256 = "dd16c29df30b69a72ea9021f0e60429a8537914e9e85187d36559e42ce59cd3c"

def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    fixture = json.loads(SOURCE.read_bytes())
    flat = {
        "rules_revision": fixture["source"]["rules_revision"],
        "rules_sha256": fixture["source"]["sha256"],
        "members": [entry["canonical_label"] for entry in fixture["entries"]],
        "id": fixture["identity_contract"]["sha256"],
    }
    data = (json.dumps(flat, ensure_ascii=False, indent=2) + "\n").encode()
    if hashlib.sha256(data).hexdigest() != EXPECTED_SHA256:
        raise SystemExit("The derived Rust CI vocabulary differs from the pinned qualification input.")
    args.output.write_bytes(data)

if __name__ == "__main__":
    main()
