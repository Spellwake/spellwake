"""Qualify only the owned local Supabase gateway; never print its generated keys."""
from pathlib import Path
import json
import fcntl
import os
import subprocess
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
BASE = "http://127.0.0.1:55431"


class NoRedirect(urllib.request.HTTPRedirectHandler):
    """A local fixture must never forward its generated credential elsewhere."""

    def redirect_request(self, request: urllib.request.Request, fp: object, code: int,
                         message: str, headers: object, new_url: str) -> None:
        return None


def qualify() -> None:
    """Use the caller-created local status record, guarded before any mutation."""
    config = (ROOT / "supabase/config.toml").read_text()
    if 'project_id = "spellwake-application-local"' not in config:
        raise RuntimeError("Refusing an unrecognized local project")
    state = json.loads((ROOT / "target/local-supabase-status.json").read_text())
    if state.get("API_URL") != BASE or not state.get("SECRET_KEY", "").startswith("sb_secret_"):
        raise RuntimeError("Refusing a nonlocal or unsupported credential target")
    opener = urllib.request.build_opener(NoRedirect())
    headers = {"apikey": state["SECRET_KEY"], "Content-Type": "application/json"}
    request = urllib.request.Request(BASE + "/storage/v1/bucket/spellwake-evidence", headers=headers)
    try:
        with opener.open(request, timeout=30) as response:
            bucket = json.load(response)
    except urllib.error.HTTPError as error:
        failure = json.loads(error.read(1048577))
        if error.code not in (400, 404) or failure.get("code") != "NoSuchBucket" or str(failure.get("statusCode")) != "404":
            raise RuntimeError("Local bucket lookup failed") from None
        request = urllib.request.Request(BASE + "/storage/v1/bucket", headers=headers, method="POST", data=json.dumps({
            "id": "spellwake-evidence", "name": "spellwake-evidence", "public": False,
            "file_size_limit": 2147483648, "allowed_mime_types": ["application/json", "application/gzip"],
        }).encode())
        with opener.open(request, timeout=30):
            pass
        with opener.open(urllib.request.Request(BASE + "/storage/v1/bucket/spellwake-evidence", headers=headers), timeout=30) as response:
            bucket = json.load(response)
    if bucket.get("id") != "spellwake-evidence" or bucket.get("public") is not False:
        raise RuntimeError("Refusing an unqualified local bucket")
    environment = os.environ.copy()
    environment.update({"SPELLWAKE_SUPABASE_URL": BASE, "SPELLWAKE_SUPABASE_SECRET_KEY": state["SECRET_KEY"],
                        "SPELLWAKE_PRICE_BUCKET": "spellwake-evidence", "SPELLWAKE_SUPABASE_PUBLISHABLE_KEY": state["PUBLISHABLE_KEY"], "SPELLWAKE_PRICE_STAGE": str(ROOT / "target/local-price-stage")})
    subprocess.run(["cargo", "test", "--locked", "--manifest-path", str(ROOT / "Cargo.toml"), "--lib",
                    "isolated_supabase_storage_and_source_rpc_interoperate", "--", "--ignored", "--nocapture"], env=environment, check=True)


def main() -> None:
    """Serialize this project's fixture cleanup across independent reviewers."""
    with (ROOT / "target/local-storage-test.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        qualify()


if __name__ == "__main__":
    main()
