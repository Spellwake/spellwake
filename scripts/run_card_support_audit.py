"""Replay the catalogue and its game receipts on one recorded Rust source tree."""
import argparse
import hashlib
import json
import os
import re
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
CRATES = ("engine", "card_programs", "contracts", "content", "oracle_compiler")
TESTS = ("ordinary_", "nonbasic_", "paid_source_boosts", "sol_ring_and_llanowar",
         "named_ordinary_permanents", "clause_support_probe", "loses_its_only_target", "its_only_target_is_exiled", "every_current_binding", "real_rebound_spells", "compiled_program_inventory")
OUTPUTS = {"SPELLWAKE_CARD_AUDIT_OUTPUT": "ordinary-cards.tsv",
           "SPELLWAKE_SPELL_AUDIT_OUTPUT": "ordinary-spells.tsv",
           "SPELLWAKE_CLAUSE_AUDIT_OUTPUT": "clauses.tsv",
           "SPELLWAKE_PROGRAM_AUDIT_OUTPUT": "programs.jsonl"}


QUALIFICATIONS = Path(__file__).with_name("card-game-qualifications.json")


def named_evidence(games, log):
    """Require an observed successful named test for every recorded claim."""
    passed = {name.rsplit("::", 1)[-1] for name in
              re.findall(r"^test ([\w:]+) \.\.\. ok$", log, re.MULTILINE)}
    if any(t not in passed for game in games for t in game["tests"]):
        missing = sorted({t for game in games for t in game["tests"]} - passed)
        raise ValueError("Missing named gameplay results: " + ", ".join(missing))
    if len({game["oracle_id"] for game in games}) != len(games):
        raise ValueError("Duplicate named card qualification")
    return games


def sha(path):
    with path.open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


def rust_sources():
    paths = [ROOT / name for name in ("Cargo.lock", "Cargo.toml", "rust-toolchain.toml")]
    for crate in CRATES:
        directory = ROOT / "crates" / crate
        paths.extend(sorted(directory.rglob("*.rs")))
        paths.append(directory / "Cargo.toml")
    return {str(path.relative_to(ROOT)): sha(path) for path in paths}


def verify(directory, check_source=True):
    receipt = json.loads((directory/"run-receipt.json").read_bytes())
    for name, digest in receipt["artifacts"].items():
        if sha(directory/name) != digest:
            raise ValueError("Evidence changed: " + name)
    if check_source and rust_sources() != receipt["rust_sources"]:
        raise ValueError("Rust source revision changed since the recorded live run")
    return receipt


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--verify", action="store_true")
    parser.add_argument("--compare-existing", action="store_true",
                        help="Require byte-identical CLI and per-card outputs before sealing the run")
    args = parser.parse_args()
    directory = args.directory.resolve()
    if args.verify:
        verify(directory)
        print("Verified the local Rust sources and sealed evidence")
        return
    source = rust_sources()
    games = json.loads(QUALIFICATIONS.read_text())
    filters = list(dict.fromkeys([*TESTS, *(t for game in games for t in game["tests"])]))
    env = os.environ.copy()
    inputs = {key: Path(env["SPELLWAKE_"+key]).resolve() for key in
              ("CATALOGUE_METADATA", "CATALOGUE_INPUT", "CREATURE_VOCABULARY_INPUT")}
    hashes = {key: sha(path) for key,path in inputs.items()}
    with tempfile.TemporaryDirectory(dir=directory,prefix=".live-game-evidence-") as folder:
        staging = Path(folder)
        env.update({key: str(staging/value) for key,value in OUTPUTS.items()})
        with (staging/"after.json").open("wb") as output, (staging/"qualifying-processor.log").open("wb") as log:
            subprocess.run(["cargo","run","--offline","--locked","-p","spellwake-card-programs","--",
                inputs["CATALOGUE_METADATA"],inputs["CATALOGUE_INPUT"],"--creature-vocabulary-input",
                inputs["CREATURE_VOCABULARY_INPUT"],"--profile","keyword-gameplay-current"],
                cwd=ROOT,env=env,stdout=output,stderr=log,check=True)
        with (staging/"qualifying-games.log").open("wb") as log:
            try:
                subprocess.run(["cargo","test","--offline","--locked","-p","spellwake-card-programs",
                    "--lib","--",*filters,"--include-ignored","--nocapture","--test-threads=4"],
                    cwd=ROOT,env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
                subprocess.run(["cargo","test","--offline","--locked","-p","spellwake-card-programs",
                    "--test","batch_combat","--","--include-ignored","--nocapture","--test-threads=2"],
                    cwd=ROOT,env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
            except subprocess.CalledProcessError:
                log.flush()
                shutil.copy2(log.name,directory/"failed-qualifying-games.log")
                raise
        if source != rust_sources() or any(sha(inputs[k])!=digest for k,digest in hashes.items()):
            raise RuntimeError("Source files changed while collecting evidence")
        names = ["after.json", *OUTPUTS.values(), "qualifying-processor.log", "qualifying-games.log"]
        named = named_evidence(games, (staging/"qualifying-games.log").read_text())
        (staging/"named-games.json").write_text(json.dumps(named, indent=2,ensure_ascii=False)+"\n")
        names.append("named-games.json")
        artifacts = {name:sha(staging/name) for name in names}
        if args.compare_existing:
            for name in ["after.json", *OUTPUTS.values()]:
                if artifacts[name] != sha(directory/name):
                    raise RuntimeError("New run differs from published evidence: "+name)
        receipt = {"profile":"keyword-gameplay-current","rustc":subprocess.check_output(["rustc","-V"],text=True,cwd=ROOT).strip(),
                   "inputs":hashes,"rust_sources":source,"test_filters":filters,"integration_tests":["batch_combat"],"artifacts":artifacts}
        (staging/"run-receipt.json").write_text(json.dumps(receipt,indent=2)+"\n")
        for name in names+["run-receipt.json"]:
            (staging/name).replace(directory/name)
    verify(directory)
    print("Sealed live processor and game evidence")


if __name__ == "__main__":
    main()
