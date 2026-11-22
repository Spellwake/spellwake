#!/usr/bin/env python3
"""Propose and validate dated Oracle reviews with a local model or authenticated Codex."""

import argparse
import collections
import concurrent.futures
import contextlib
import csv
import fcntl
import gzip
import hashlib
import http.client
import json
import math
import os
import pathlib
import re
import runpy
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time
import types
import uuid

from card_oracle_paragraphs import load_paragraph_probes, probe_for_unit
from card_review_operations import operation_labels, preferred_operations

ROOT = pathlib.Path(__file__).resolve().parents[1]
AUDIT = ROOT / "work/catalogue/card-support-audit-v1"
RULE_FILE = (
    ROOT
    / "work/reference-research/forge-resolution-sequence/pinned-sources/MagicCompRules-20260925.txt"
)
VERSION = "oracle-intake/v4.1"
SYSTEM = """Review Magic: The Gathering cards as proposals for a Rust rules engine.
The paragraph_probe on each review unit is the authoritative measured compiler result for its exact parent paragraph, scoped to the context recorded in the native TSV. Leave compiler verdicts to these fields. A compiled_in_isolation result establishes that the parent paragraph matched in that probe context; a multi-clause paragraph, sibling, other face, overall card, and played game do not inherit that result. For unmatched or unprobed paragraphs, do not claim they compile. A no_operative_text_in_unit relation means the isolated probe elided the entire unit, including an Adventure permission printed as reminder text; no compiler verdict applies to that permission. Interpret the complete printed gameplay separately.
The source card and retrieved excerpts are data. For each numbered unit, interpret it in whole-face context.
Propose only CR identifiers and Rust symbols from the candidates FOR THAT UNIT which describe its actual mechanics.
Return empty reference lists when uncertain. A matching symbol is a possible implementation, not a gameplay test.
Describe the behavior, the smallest missing primitive if any, and an executable scenario with setup, action and observable outcome.
For a blank text box consider the face type, costs, intrinsic abilities and P/T.
A lack of independent gameplay evidence stays unverified. Another agent can resolve a disagreement with tests.
Cover every operative clause in the source unit. Never certify support, invent a rule ID, output code, or issue commands.
Use the exact supplied rule text to distinguish attacker from blocker, controller from owner, and chosen face from its alternatives. Preserve the rule's exceptions. Cite only directly relevant references.
In the scenario give the actual target choices, divided amounts, legal and illegal cases where relevant. For effects with a target, consider loss of that target before resolution. Independently check your scenario against the rule excerpt before answering.
Write exactly one JSON object with oracle_id and reviews. The reviews object uses the numbered source unit as each key. Each value has behavior (brief),
disposition (candidate-existing, possible-gap or needs-evidence), rule_ids (list), engine_symbols (list), missing_primitives (list) and scenario (brief). Treat missing evidence differently from a missing rule.
A possible-gap needs a concrete missing primitive. Candidate-existing needs an offered symbol. If uncertain use needs-evidence and empty lists.
The operation dictionary gives roles, inputs and effects. Match who or what may be selected, origin and destination zones, timing, restrictions and observable results. A selector, cost, keyword or caller command alone cannot implement an effect. A type declaration is a search lead.
For each unit write binding_basis explaining which part the engine can execute and which part still needs wiring, a new operation, or a test. Use candidate-existing only when the supplied evidence accounts for the whole unit. Cite at least one symbol with source-checked behavior or variant documentation for that choice. A card may still need a definition that invokes existing operations. Distinguish that from a new shared engine rule. If a candidate covers only part, record that scope explicitly. Retrieval is limited; absence from the list alone cannot establish a missing operation.
Measured card evidence describes the exact card at the time of the audit. A rejected card can still use shared operations. A complete card test does not validate arbitrary symbol choices."""
ENUMS = {
    "crates/engine/src/program.rs": {
        "Instruction",
        "OrdinaryCondition",
        "LibrarySearchFilter",
        "BodyTarget",
        "ConditionalCastCost",
        "AdditionalCastCost",
        "EntryCounterAbility",
        "CombatTriggerKeyword",
        "SpellTargetFilter",
    },
    "crates/engine/src/characteristics/types.rs": {
        "KeywordAbility",
        "ResolvedPart",
        "StaticPart",
        "ControllerSelector",
    },
    "crates/engine/src/game.rs": {"GameCommand", "RuleFrame"},
    "crates/engine/src/activated.rs": {"SourceCost", "ActivationRestriction"},
    "crates/engine/src/mana.rs": {"ManaOutput", "CastResource"},
    "crates/engine/src/land.rs": {"EntryTap"},
}
OPERATION_ROLES = {
    "Instruction": "resolving effect",
    "OrdinaryCondition": "event condition",
    "LibrarySearchFilter": "library search filter",
    "BodyTarget": "target contract",
    "SpellTargetFilter": "spell target filter",
    "ConditionalCastCost": "casting cost condition",
    "AdditionalCastCost": "additional casting cost",
    "EntryCounterAbility": "entry counter ability",
    "CombatTriggerKeyword": "combat trigger",
    "KeywordAbility": "keyword characteristic",
    "ResolvedPart": "continuous effect from resolution",
    "StaticPart": "continuous effect from a static ability",
    "ControllerSelector": "controller filter",
    "GameCommand": "game command",
    "RuleFrame": "game execution frame",
    "SourceCost": "cost paid by the ability source",
    "ActivationRestriction": "activation restriction",
    "ManaOutput": "mana production",
    "CastResource": "casting resource",
    "EntryTap": "tapped status on battlefield entry",
}
HASH = re.compile(r"[0-9a-f]{64}\Z")
RULE = re.compile(r"^([1-9]\d\d\.\d+[a-z]?)\.?\s+(.+)$")
STOP = {
    "a",
    "an",
    "the",
    "to",
    "and",
    "or",
    "if",
    "of",
    "for",
    "in",
    "it",
    "its",
    "this",
    "that",
    "is",
    "are",
    "as",
    "be",
    "may",
    "with",
    "you",
    "your",
    "on",
    "from",
    "at",
    "one",
    "two",
    "three",
    "four",
    "each",
    "then",
    "their",
    "they",
    "has",
    "have",
    "by",
    "during",
    "whenever",
    "when",
    "until",
    "those",
    "than",
    "more",
    "any",
    "so",
    "into",
    "put",
    "instead",
    "can",
}
ALIASES = {
    "enters": "enters battlefield Enters",
    "dies": "dies graveyard battlefield",
    "draw": "draw drawcontroller",
    "life": "life adjustcontrollerlife",
    "add": "mana manaoutput",
    "flying": "flying evasion",
    "tapped": "tap tapped",
    "counter": "counter counterspell countering",
    "create": "createtokens token",
    "gets": "power toughness modifypt",
    "discard": "discard discarded",
    "target": "target targeting",
    "land": "land playland",
    "control": "control controller",
}
STATUSES = ("candidate-existing", "possible-gap", "needs-evidence")


def dumps(value):
    return json.dumps(
        value,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
        allow_nan=False,
    )


def digest(data):
    return hashlib.sha256(data).hexdigest()


def sha_file(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def uuid_string(value):
    return isinstance(value, str) and str(uuid.UUID(value)) == value


def load_queue(path):
    cards = {}
    with path.open(encoding="utf-8") as file:
        for line_no, line in enumerate(file, 1):
            try:
                card = json.loads(line)
                oid = card["oracle_id"]
                require(
                    uuid_string(oid) and oid not in cards, "invalid/duplicate Oracle ID"
                )
                require(
                    uuid_string(card["printing_id"])
                    and HASH.fullmatch(card["source_line_sha256"])
                    and HASH.fullmatch(card["catalogue_sha256"]),
                    "missing source identity",
                )
                require(
                    isinstance(card["name"], str)
                    and card["name"]
                    and isinstance(card["faces"], list)
                    and card["faces"],
                    "missing name/faces",
                )
                for index, face in enumerate(card["faces"]):
                    require(
                        face["face_index"] == index
                        and face["name"]
                        and face["type_line"]
                        and isinstance(face.get("oracle_text", ""), str),
                        "invalid face index or face data",
                    )
                cards[oid] = card
            except (KeyError, TypeError, ValueError) as err:
                raise ValueError(f"{path}:{line_no}: {err}") from err
    require(cards, "empty input")
    return cards


def units(card):
    """Source coordinates are half-open Unicode code-point offsets within a face.
    Every paragraph is retained verbatim. Within it, split prose only outside quoted and reminder text.
    Delimiters remain in their source slice so punctuation, repeated lines and whitespace round-trip.
    """
    result = []
    for face in card["faces"]:
        raw = face.get("oracle_text", "")
        offset = 0
        paragraph = 0
        for full_line in raw.splitlines(keepends=True):
            line = full_line.rstrip("\r\n\x0b\x0c\x1c\x1d\x1e\x85\u2028\u2029")
            if not line.strip():
                offset += len(full_line)
                continue
            begin, depth, quoted = 0, 0, False
            for i, ch in enumerate(line):
                if ch in '"“”':
                    quoted = not quoted
                elif not quoted and ch in "([":
                    depth += 1
                elif not quoted and ch in ")]":
                    depth = max(0, depth - 1)
                boundary = line.startswith(", then ", i) or (
                    ch in ".;"
                    and i + 1 < len(line)
                    and line[i + 1].isspace()
                    and line[i + 1 :].strip()
                )
                if boundary and not quoted and not depth:
                    result.append(
                        (
                            face["face_index"],
                            paragraph,
                            offset + begin,
                            offset + i + 1,
                            line[begin : i + 1],
                        )
                    )
                    begin = i + 1
            if begin < len(line):
                result.append(
                    (
                        face["face_index"],
                        paragraph,
                        offset + begin,
                        offset + len(line),
                        line[begin:],
                    )
                )
            offset += len(full_line)
            paragraph += 1
        if not paragraph:
            result.append((face["face_index"], 0, 0, len(raw), raw))
    return [
        {"unit": i, "face": f, "paragraph": p, "start": s, "end": e, "text": t}
        for i, (f, p, s, e, t) in enumerate(result)
    ]


def tokens(text):
    split = re.sub(r"([a-z])([A-Z])", r"\1 \2", text)
    return {w for w in re.findall(r"[a-z][a-z0-9]*", split.lower()) if w not in STOP}


class Index:
    def __init__(self, rows):
        self.rows = rows
        self.postings = collections.defaultdict(list)
        self.norm = {}
        for key, row in rows.items():
            words = tokens(row["text"])
            self.norm[key] = 1 + math.log1p(len(words)) / 5
            for word in words:
                self.postings[word].append(key)

    def lookup(self, text, count):
        weights = collections.Counter()
        query = tokens(text)
        expanded = query | {
            w for q in query for w in ALIASES.get(q, "").lower().split()
        }
        for term in sorted(expanded):
            postings = self.postings.get(term, ())
            weight = (2 if term in query else 1) * math.log1p(
                len(self.rows) / (1 + len(postings))
            )
            for key in postings:
                weights[key] += weight / self.norm[key]
        return sorted(weights, key=lambda k: (-weights[k], k))[:count]


def declaration_row(
    lines: list[str], number: int, enum: str, variant: str, file: str
) -> dict:
    """Preserve the variant's typed signature and immediately preceding Rust documentation."""
    doc_start = number
    while doc_start and lines[doc_start - 1].strip().startswith("///"):
        doc_start -= 1
    docs = " ".join(line.strip()[3:].strip() for line in lines[doc_start:number])
    signature = [lines[number].strip()]
    if not signature[0].endswith(","):
        for line in lines[number + 1 :]:
            if (
                line.startswith("    ")
                and line.strip()
                and not line.lstrip().startswith("//")
            ):
                signature.append(line.strip())
            if line in ("    },", "    ),"):
                break
            if line.startswith("}"):
                raise ValueError(f"Unterminated variant {enum}::{variant} in {file}")
    key = f"{enum}::{variant}"
    role = OPERATION_ROLES[enum]
    return {
        "text": " ".join((key, role, docs, *signature)),
        "file": file,
        "line": number + 1,
        "contract": {
            "role": role,
            "signature": " ".join(signature).rstrip(","),
            "documentation": docs,
            "evidence": "documented declaration" if docs else "declaration only",
            "sources": [{"file": file, "line": number + 1}],
        },
    }


def semantic_source_paths() -> set[str]:
    files = set(ENUMS) | {
        "scripts/card_review_operations.py",
        "scripts/card_oracle_paragraphs.py",
    }
    for entry in operation_labels(ROOT).values():
        files.update(source["file"] for source in entry["sources"])
    return files


def indices(rule_path):
    rules = {}
    for n, text in enumerate(rule_path.read_text(encoding="utf-8-sig").split("\n"), 1):
        m = RULE.match(text.strip())
        if m:
            rules[m[1]] = {"text": m[2], "file": str(rule_path), "line": n}
    require(len(rules) > 1000, "invalid dated rulebook")
    return Index(rules), operation_index()


def operation_index() -> Index:
    """Build the searchable operation dictionary with validated source references."""
    mechanics = {}
    for name, enums in ENUMS.items():
        enum = None
        lines = (ROOT / name).read_text().splitlines()
        for number, text in enumerate(lines):
            decl = re.match(r"^pub enum (\w+)\s*\{", text)
            if decl:
                enum = decl[1] if decl[1] in enums else None
            elif enum and text.startswith("}"):
                enum = None
            elif enum and (
                match := re.match(r"^    ([A-Z][A-Za-z0-9_]+)\s*[,({]", text)
            ):
                key = f"{enum}::{match[1]}"
                mechanics[key] = declaration_row(lines, number, enum, match[1], name)
    for key, label in operation_labels(ROOT).items():
        if key not in mechanics:
            mechanics[key] = {**label["sources"][0], "contract": {}}
        row = mechanics[key]
        row["contract"].update(label, evidence="source-checked behavior")
        row["text"] = f"{key} {label['label']} {row['contract'].get('role', '')}"
    require(len(mechanics) > 100, "engine mechanics index incomplete")
    return Index(mechanics)


def candidates(card, unit, rule_index, engine_index, top):
    face = card["faces"][unit["face"]]
    text = unit["text"].strip() or face["type_line"]
    primary = text.split(" (", 1)[0].strip().rstrip(".")
    rules = rule_index.lookup(text, top)
    mechanics = engine_index.lookup(text, top)
    if primary != text and len(primary.split()) <= 4:
        rules = list(dict.fromkeys(rule_index.lookup(primary, min(3, top)) + rules))[
            :top
        ]
        mechanics = list(
            dict.fromkeys(engine_index.lookup(primary, min(2, top)) + mechanics)
        )[:top]
    # Pin dated excerpts for structural decisions that lexical retrieval easily overlooks.
    # Other rules still come from the general vocabulary index.
    pinned = []
    if re.search(r"\bflying\b", text, re.IGNORECASE):
        pinned.extend(["702.9b", "702.17b"])
    if (
        re.search(r"\bdivid(?:e|ed)\b|\bdistribut(?:e|ed)\b", text, re.IGNORECASE)
        and "target" in text.lower()
    ):
        pinned.extend(["601.2d", "115.3"])
    if "target" in text.lower():
        pinned.append("608.2b")
    if card["layout"] == "split":
        pinned.extend(["709.3", "709.3a", "709.3b"])
    if text.lower().startswith("tap target"):
        pinned.append("701.26a")
    if "draw a card" in text.lower():
        pinned.append("121.1")
    preferred = preferred_operations(text)
    require(
        all(key in engine_index.rows for key in preferred),
        "unknown preferred operation",
    )
    mechanics = list(dict.fromkeys([*preferred, *mechanics]))[:top]
    return {
        "rules": list(
            dict.fromkeys([k for k in pinned if k in rule_index.rows] + rules)
        )[: max(top, len(pinned))],
        "mechanics": mechanics,
        "documented": [
            key
            for key in mechanics
            if engine_index.rows[key]["contract"]["evidence"] != "declaration only"
        ],
    }


def schema(batch, card, choices):
    def form(fields):
        return {
            "type": "object",
            "additionalProperties": False,
            "required": list(fields),
            "properties": fields,
        }

    def refs(values):
        item = {"type": "string", "enum": values} if values else {"type": "string"}
        return {"type": "array", "items": item, "maxItems": min(4, len(values))}

    fields = {}
    for unit in batch:
        offered = choices[unit["unit"]]
        fields[str(unit["unit"])] = form(
            {
                "behavior": {"type": "string"},
                "binding_basis": {"type": "string"},
                "disposition": {
                    "type": "string",
                    "enum": [
                        state
                        for state in STATUSES
                        if state != "candidate-existing" or offered["documented"]
                    ],
                },
                "rule_ids": refs(offered["rules"]),
                "engine_symbols": refs(offered["mechanics"]),
                "missing_primitives": {
                    "type": "array",
                    "items": {"type": "string"},
                    "maxItems": 4,
                },
                "scenario": {"type": "string"},
            }
        )
    return form(
        {
            "oracle_id": {"type": "string", "enum": [card["oracle_id"]]},
            "reviews": form(fields),
        }
    )


def validate_reply(reply, card, batch, choices):
    require(
        isinstance(reply, dict)
        and set(reply) == {"oracle_id", "reviews"}
        and reply["oracle_id"] == card["oracle_id"],
        "wrong Oracle ID or unexpected fields",
    )
    results = reply["reviews"]
    expected = {u["unit"] for u in batch}
    require(
        isinstance(results, list) and len(results) == len(expected),
        "wrong review count",
    )
    for item in results:
        require(
            isinstance(item, dict)
            and set(item)
            == {
                "unit",
                "behavior",
                "binding_basis",
                "disposition",
                "rule_ids",
                "engine_symbols",
                "missing_primitives",
                "scenario",
            },
            "invalid review fields",
        )
        index = item["unit"]
        require(
            type(index) is int and index in expected, "duplicate/missing/wrong unit"
        )
        expected.remove(index)
        for key in ("behavior", "binding_basis", "scenario"):
            require(
                isinstance(item[key], str) and 8 <= len(item[key].strip()) <= 4000,
                "empty/excessive " + key,
            )
        require(item["disposition"] in STATUSES, "unknown disposition")
        for key, source in [
            ("rule_ids", "rules"),
            ("engine_symbols", "mechanics"),
            ("missing_primitives", None),
        ]:
            values = item[key]
            require(
                isinstance(values, list)
                and len(values) <= 4
                and all(
                    isinstance(v, str) and 0 < len(v.strip()) <= 450 for v in values
                )
                and len(set(values)) == len(values),
                "invalid " + key,
            )
            if source:
                require(
                    set(values) <= set(choices[index][source]),
                    f"unoffered {key} for unit {index}",
                )
        if item["disposition"] == "possible-gap":
            require(item["missing_primitives"], "gap without a missing primitive")
        if item["disposition"] == "candidate-existing":
            require(
                set(item["engine_symbols"]).intersection(choices[index]["documented"]),
                "candidate-existing needs a documented operation",
            )
            require(
                not item["missing_primitives"],
                "candidate-existing with missing operations",
            )
    require(not expected, "unreviewed source unit")
    return sorted(results, key=lambda v: v["unit"])


def api(port, path, payload=None, timeout=240):
    # Fixed loopback socket, no environment proxies, redirects, model pulling or remote endpoint.
    try:
        with contextlib.closing(
            http.client.HTTPConnection("127.0.0.1", port, timeout=timeout)
        ) as conn:
            conn.request(
                "GET" if payload is None else "POST",
                path,
                None if payload is None else dumps(payload).encode(),
                {"Content-Type": "application/json"},
            )
            rep = conn.getresponse()
            body = rep.read(12_000_001)
            if rep.status != 200 or len(body) > 12_000_000:
                raise ProviderUnavailable(
                    f"Ollama {path} HTTP {rep.status}; inference service unavailable"
                )
            return json.loads(body)
    except (OSError, http.client.HTTPException) as error:
        raise ProviderUnavailable(
            f"Ollama {path} connection failed: {error}"
        ) from error


class ProviderUnavailable(RuntimeError):
    """The requested provider could not complete the request."""


def codex_env():
    # Retain the existing ChatGPT login location while excluding API credentials and provider overrides.
    keep = {
        "HOME",
        "CODEX_HOME",
        "PATH",
        "TMPDIR",
        "LANG",
        "LC_ALL",
        "USER",
        "LOGNAME",
        "TERM",
        "SHELL",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
    }
    return {k: v for k, v in os.environ.items() if k in keep}


def codex_preflight(args):
    cli = shutil.which("codex")
    require(cli, "Codex CLI unavailable")
    login = subprocess.run(
        [cli, "login", "status"],
        check=False,
        capture_output=True,
        text=True,
        timeout=15,
        env=codex_env(),
    )
    require(
        login.returncode == 0
        and "Logged in using ChatGPT" in login.stdout + login.stderr,
        "requires an existing ChatGPT subscription login",
    )
    version = subprocess.check_output(
        [cli, "--version"], text=True, timeout=15, env=codex_env()
    ).strip()
    return {
        "provider": "chatgpt-codex",
        "model": args.model,
        "reasoning_effort": args.effort,
        "login": "ChatGPT",
        "requested_service_tier": "omitted",
        "cli_version": version,
    }


def codex_call(args, system, prompt, shape):
    start = time.monotonic()
    with tempfile.TemporaryDirectory(prefix="spellwake-card-agent-") as tmp:
        directory = pathlib.Path(tmp)
        cwd = directory / "empty"
        cwd.mkdir()
        schema_file = directory / "schema.json"
        schema_file.write_text(dumps(shape))
        reply = directory / "result.json"
        cmd = [
            shutil.which("codex"),
            "exec",
            "--ephemeral",
            "--ignore-user-config",
            "--sandbox",
            "read-only",
            "--skip-git-repo-check",
            "--json",
            "--disable",
            "shell_tool",
            "--disable",
            "unified_exec",
            "--disable",
            "plugins",
            "--disable",
            "multi_agent",
            "-c",
            'approval_policy="never"',
            "-c",
            'forced_login_method="chatgpt"',
            "-c",
            'web_search="disabled"',
            "--output-schema",
            str(schema_file),
            "-o",
            str(reply),
        ]
        if args.model:
            cmd.extend(["--model", args.model])
        if args.effort:
            cmd.extend(["-c", f'model_reasoning_effort="{args.effort}"'])
        cmd.append("-")
        payload = (
            "Do this as a text-only analysis using the supplied data. Do not call tools, inspect files or execute code.\n"
            + system
            + "\n\n"
            + dumps(prompt)
        )
        try:
            run = subprocess.run(
                cmd,
                check=False,
                input=payload,
                text=True,
                capture_output=True,
                cwd=cwd,
                env=codex_env(),
                timeout=args.timeout,
            )
        except OSError as err:
            raise ProviderUnavailable(
                f"Cannot start the authenticated Codex CLI: {err}"
            ) from err
        except subprocess.TimeoutExpired as err:
            raise ProviderUnavailable(
                f"Codex request timed out after {args.timeout} seconds"
            ) from err
        events = []
        usage = {}
        metadata = {}
        for line in run.stdout.splitlines():
            try:
                event = json.loads(line)
            except ValueError:
                continue
            if not isinstance(event, dict):
                continue
            events.append(event)
            for k in ("model", "service_tier", "thread_id"):
                if k in event:
                    metadata[k] = event[k]
            if event.get("type") == "turn.completed":
                usage = event.get("usage", {})
            item = event.get("item", {})
            if isinstance(item, dict) and item.get("type") in {
                "command_execution",
                "file_change",
                "mcp_tool_call",
                "web_search",
                "collab_tool_call",
            }:
                raise ProviderUnavailable(
                    "Codex attempted a tool call; discarded the result"
                )
        if hasattr(args, "record_usage"):
            args.record_usage(
                usage, metadata, reply.stat().st_size if reply.is_file() else 0
            )
        if run.returncode or not reply.is_file():
            messages = []
            for event in events:
                error = event.get("error", event.get("message"))
                if isinstance(error, dict):
                    error = error.get("message")
                if isinstance(error, str):
                    messages.append(error)
            detail = next(
                (x for x in reversed(messages) if x.strip()),
                run.stderr.strip().splitlines()[-1]
                if run.stderr.strip()
                else "no final response",
            )
            raise ProviderUnavailable(f"Codex exit {run.returncode}: {detail[:1200]}")
        require(reply.stat().st_size <= 262144, "response exceeded 256 KiB")
        return {
            "usage": usage,
            "provider_metadata": metadata,
            "done": True,
            "done_reason": "stop",
            "message": {"content": reply.read_text()},
            "total_duration": int((time.monotonic() - start) * 1e9),
            "prompt_eval_count": usage.get("input_tokens", 0),
            "eval_count": usage.get("output_tokens", 0),
        }


def generated(args, system, prompt, shape, budget):
    if args.provider == "chatgpt-codex":
        return codex_call(args, system, prompt, shape)
    return api(
        args.port,
        "/api/chat",
        {
            "model": args.model,
            "stream": False,
            "think": False,
            "keep_alive": "2m",
            "options": {
                "num_ctx": args.context,
                "num_predict": budget,
                "temperature": 0,
                "seed": 12345,
            },
            "format": shape,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": dumps(prompt)},
            ],
        },
        args.timeout,
    )


def preflight(args):
    if args.provider == "chatgpt-codex":
        return codex_preflight(args)
    models = api(args.port, "/api/tags")["models"]
    model = next((m for m in models if m["name"] == args.model), None)
    require(
        model is not None
        and HASH.fullmatch(model["digest"])
        and ":cloud" not in args.model.lower(),
        "use an exact installed local model tag",
    )
    detail = api(args.port, "/api/show", {"model": args.model})
    require(
        not (
            detail.get("remote_model")
            or detail.get("remote_host")
            or model.get("remote_model")
            or model.get("remote_host")
        ),
        "model delegates inference remotely",
    )
    return {
        "model": model["name"],
        "model_digest": model["digest"],
        "model_bytes": model["size"],
        "ollama": api(args.port, "/api/version")["version"],
    }


def prompt_for(card, batch, choices, indexes):
    contexts = []
    operations = {}
    for unit in batch:
        offered = choices[unit["unit"]]
        for key in offered["mechanics"]:
            operations[key] = indexes[1].rows[key]["contract"]
        contexts.append(
            {
                "unit": unit["unit"],
                "face": unit["face"],
                "source": unit["text"],
                "paragraph_probe": probe_for_unit(
                    card, unit, indexes[1].paragraph_probes
                ),
                "rules": {
                    k: indexes[0].rows[k]["text"][:1800] for k in offered["rules"]
                },
                "engine_symbols": offered["mechanics"],
            }
        )
    return {
        "oracle_id": card["oracle_id"],
        "layout": card["layout"],
        "faces": card["faces"],
        "review_units": contexts,
        "operations": operations,
        "measured_card_evidence": indexes[1].card_evidence[card["oracle_id"]],
    }


def infer(args, card, batch, choices, indexes):
    prompt = prompt_for(card, batch, choices, indexes)
    result = generated(
        args, SYSTEM, prompt, schema(batch, card, choices), args.output_tokens
    )
    require(
        result.get("done") is True and result.get("done_reason") != "length",
        "incomplete model reply",
    )
    require(
        args.provider == "chatgpt-codex"
        or result.get("prompt_eval_count", 0) + result.get("eval_count", 0)
        < args.context,
        "context capacity reached",
    )
    content = result["message"].get("content", "").strip()
    require(
        content,
        f"model produced empty structured output (reason={result.get('done_reason')}, evaluated={result.get('eval_count')})",
    )
    metrics = {
        k: result.get(k, 0)
        for k in (
            "prompt_eval_count",
            "eval_count",
            "total_duration",
            "load_duration",
            "eval_duration",
        )
    }
    if args.provider == "chatgpt-codex":
        metrics["cli_usage"] = result["usage"]
        metrics["provider_metadata"] = result["provider_metadata"]
    return decode_reply(
        json.loads(content, object_pairs_hook=unique_keys), batch
    ), metrics


def decode_reply(data, batch):
    require(
        isinstance(data, dict)
        and set(data) == {"reviews", "oracle_id"}
        and isinstance(data["reviews"], dict)
        and set(data["reviews"]) == {str(u["unit"]) for u in batch},
        "wrong anchored unit keys",
    )
    data["reviews"] = [
        dict(item, unit=int(key)) for key, item in data["reviews"].items()
    ]
    return data


def store(db, source, config):
    db.execute(
        "CREATE TABLE IF NOT EXISTS manifest (id INTEGER PRIMARY KEY CHECK(id=1), value TEXT NOT NULL)"
    )
    db.execute(
        "CREATE TABLE IF NOT EXISTS proposals (oracle_id TEXT PRIMARY KEY, value TEXT NOT NULL)"
    )
    db.execute(
        "CREATE TABLE IF NOT EXISTS failures (oracle_id TEXT PRIMARY KEY, attempts INTEGER NOT NULL, last_error TEXT NOT NULL)"
    )
    db.execute(
        "CREATE TABLE IF NOT EXISTS provider_stops (time_utc TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, reason TEXT NOT NULL)"
    )
    data = {**source, **config}
    db.execute("INSERT OR IGNORE INTO manifest VALUES (1,?)", (dumps(data),))
    prior = json.loads(
        db.execute("SELECT value FROM manifest WHERE id=1").fetchone()[0]
    )
    require(
        prior.keys() == data.keys()
        and all(prior[k] == v for k, v in data.items() if k != "harness_sha256"),
        "manifest changed; choose a fresh --db",
    )
    db.commit()


def links(keys, index):
    return [
        {"id": key, "file": index.rows[key]["file"], "line": index.rows[key]["line"]}
        for key in keys
    ]


AUDIT_INPUTS = (
    "cards.csv",
    "clauses.tsv",
    "run-receipt.json",
    "after.json",
    "programs.jsonl",
    "ordinary-cards.tsv",
    "ordinary-spells.tsv",
    "qualifying-games.log",
)
PRINTED_FIELDS = (
    "name",
    "type_line",
    "mana_cost",
    "oracle_text",
    "power",
    "toughness",
    "loyalty",
    "defense",
    "color_indicator",
)


def measured_card_evidence(args, cards, source):
    """Join the accepted, dated engine run to the immutable review input by exact card and face identity."""
    folder = args.evidence_dir
    receipt = json.loads((folder / "run-receipt.json").read_text())
    inputs = source["audit_input_sha256"]
    for name in (
        "clauses.tsv",
        "after.json",
        "ordinary-cards.tsv",
        "ordinary-spells.tsv",
        "programs.jsonl",
        "qualifying-games.log",
    ):
        require(
            receipt["artifacts"].get(name) == inputs[name],
            f"native evidence digest mismatch: {name}",
        )
    registry = json.loads((folder / "after.json").read_text())["data"]
    catalogue = receipt["inputs"]["CATALOGUE_INPUT"]
    require(
        registry["sourceSha256"] == catalogue, "registry uses a different catalogue"
    )
    bindings = {r["oracleId"]: r for r in registry["cards"] if r["required"]}
    require(
        len(bindings) == registry["requiredRecords"]
        and bindings.keys() == cards.keys(),
        "registry/queue membership mismatch",
    )
    games = {}
    for name in ("ordinary-cards.tsv", "ordinary-spells.tsv"):
        with (folder / name).open(newline="", encoding="utf-8") as stream:
            for row_number, row in enumerate(csv.DictReader(stream, delimiter="\t"), 2):
                oid = row["oracle_id"]
                require(
                    oid in cards and row["name"] == cards[oid]["name"],
                    f"game evidence card mismatch: {name} row {row_number}",
                )
                if name == "ordinary-cards.tsv":
                    if row["status"] not in ("verified", "partial"):
                        continue
                    require(
                        row["source_printing_id"] == cards[oid]["printing_id"]
                        and row["catalogue_sha256"] == catalogue
                        and row["semantic_sha256"]
                        == bindings[oid]["binding"].get("semanticSha256"),
                        f"game evidence printing or semantic digest mismatch: {oid}",
                    )
                    status, detail = row["status"], row["evidence_or_reason"]
                else:
                    status, detail = "verified", row["evidence"]
                games[oid] = {
                    "file": name,
                    "row": row_number,
                    "scope": status,
                    "observed": detail,
                }
    # Sol Ring's complete named game is a separate fixture in the sealed native log.
    sol_test = "sol_ring_and_llanowar_elves_pay_their_prices_and_produce_printed_mana"
    with (folder / "qualifying-games.log").open() as stream:
        sol_line = next(
            (
                n
                for n, line in enumerate(stream, 1)
                if sol_test in line and "... ok" in line
            ),
            None,
        )
    named_games = {}
    if "named-games.json" in receipt["artifacts"]:
        named_path = folder / "named-games.json"
        require(sha_file(named_path) == receipt["artifacts"]["named-games.json"], "named games digest mismatch")
        log = (folder / "qualifying-games.log").read_text(encoding="utf-8")
        passed = {name.rsplit("::", 1)[-1] for name in re.findall(r"^test ([\w:]+) \.\.\. ok$", log, re.MULTILINE)}
        for game in json.loads(named_path.read_text(encoding="utf-8")):
            oid = game["oracle_id"]
            require(oid in cards and oid not in named_games and game["name"] == cards[oid]["name"], "named game identity mismatch")
            printed = [{k: face[k] for k in PRINTED_FIELDS if k in face} for face in cards[oid]["faces"]]
            require(digest(json.dumps(printed, ensure_ascii=False).encode()) == game["printed_faces_sha256"], "named game printed faces mismatch")
            require(game["tests"] and set(game["tests"]) <= passed and game["scope"] in ("complete", "partial"), "named game tests not verified")
            named_games[oid] = {"file": "named-games.json", "scope": "verified" if game["scope"] == "complete" else "partial", "observed": game["evidence"], "tests": game["tests"]}
    programs = {}
    with (folder / "programs.jsonl").open(encoding="utf-8") as stream:
        for line_number, line in enumerate(stream, 1):
            program = json.loads(line)
            oid = program["oracle_id"]
            require(
                oid in cards
                and oid not in programs
                and program["name"] == cards[oid]["name"],
                f"program source mismatch at {line_number}",
            )
            compact = {"file": "programs.jsonl", "row": line_number}
            for field in ("kind", "keywords", "mana", "activated", "triggered"):
                value = program.get(field)
                if not value:
                    continue
                # Rust debug strings can be long. Preserve the complete record's location and
                # mark every clipped excerpt so it cannot be taken as a complete program.
                if len(dumps(value)) > 450:
                    compact[field] = {"excerpt": dumps(value)[:445], "truncated": True}
                else:
                    compact[field] = value
            programs[oid] = compact
    evidence = {}
    with (folder / "cards.csv").open(newline="", encoding="utf-8") as stream:
        for row_number, row in enumerate(csv.DictReader(stream), 2):
            oid = row["oracle_id"]
            require(
                oid in cards and oid not in evidence,
                "duplicate or unknown coverage row",
            )
            card = cards[oid]
            expected = [
                {k: face[k] for k in PRINTED_FIELDS if k in face}
                for face in card["faces"]
            ]
            require(
                row["name"] == card["name"]
                and row["layout"] == card["layout"]
                and int(row["catalogue_line"]) == card["catalogue_line"]
                and card["catalogue_sha256"] == catalogue
                and json.loads(row["complete_printed_faces"]) == expected,
                f"coverage source identity mismatch: {oid}",
            )
            binding = bindings[oid]["binding"]
            present = oid in programs
            require(
                row["current_processor"] == ("Executable" if present else "Blocked")
                and present == (binding["status"] != "blocked")
                and row["semantic_sha256"] == binding.get("semanticSha256", "")
                and row["compiled_instructions"]
                == (f"programs.jsonl line {programs[oid]['row']}" if present else ""),
                f"coverage/processor mismatch: {oid}",
            )
            result = {
                "coverage_row": row_number,
                **{
                    k: row[k]
                    for k in (
                        "support",
                        "current_processor",
                        "behavior_evidence",
                        "verified_shared_abilities",
                        "remaining_work_families",
                        "earlier_profile_scopes",
                        "current_processor_diagnostic",
                    )
                },
            }
            require(
                result["support"]
                in ("Fully supported", "Partially supported", "Unsupported"),
                f"unknown measured support level: {oid}",
            )
            if not present:
                # Each isolated paragraph now carries its own sealed source-row context.
                result.pop("verified_shared_abilities", None)
                if result["behavior_evidence"].startswith(
                    "Reminder-stripped Oracle paragraphs compile"
                ):
                    result.pop("behavior_evidence")
            if oid in games:
                result["game_evidence"] = games[oid]
            if (
                row["name"] == "Sol Ring"
                and present
                and expected[0]["oracle_text"] == "{T}: Add {C}{C}."
                and sol_line
            ):
                result["game_evidence"] = {
                    "file": "qualifying-games.log",
                    "line": sol_line,
                    "scope": "verified",
                    "test": sol_test,
                }
            if oid in named_games:
                require(present, f"named game has no executable program: {oid}")
                result["game_evidence"] = named_games[oid]
            named_verified = result.get("game_evidence", {}).get("scope") == "verified"
            require(
                (result["support"] == "Fully supported") == named_verified,
                f"whole-card rating and named game evidence differ: {oid}",
            )
            if present:
                result["program"] = programs[oid]
            evidence[oid] = result
    require(evidence.keys() == cards.keys(), "incomplete measured evidence coverage")
    return types.MappingProxyType(evidence)


def legacy_api(args, cleanup):
    """Load the retained first-100 validator and Rust declarations exclusively for offline reads."""
    require(
        args.action in ("status", "validate", "export"),
        "legacy databases are read-only",
    )
    snapshots = {
        "agent-harness-a541d25f514c.py.gz": "a541d25f514cb3d489a91e55ae8d89628a6389ca83d10db6c4c8d06ada98b639",
        "agent-v3-engine-reference.json.gz": "b8cc5d7768570300690f4374ab2a8d1ffb0b01ca52bf1e9bfef3a64477347bd5",
    }
    payloads = []
    for name, expected in snapshots.items():
        path = next(
            (p for p in (args.db.parent / name, AUDIT / name) if p.is_file()), None
        )
        require(path, "missing historical review source: " + name)
        with gzip.open(path, "rb") as f:
            data = f.read(2_000_001)
        require(
            len(data) <= 2_000_000 and digest(data) == expected,
            "historical review source digest mismatch: " + name,
        )
        payloads.append(data)
    folder = pathlib.Path(
        cleanup.enter_context(
            tempfile.TemporaryDirectory(prefix="spellwake-v3-offline-")
        )
    )
    script = folder / "scripts" / "card-support-agent.py"
    script.parent.mkdir()
    script.write_bytes(payloads[0])
    for relative, text in json.loads(payloads[1]).items():
        target = folder / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)
    # The SHA-pinned archived source is repository-authored. Its CLI entry point never runs.
    return runpy.run_path(str(script), run_name="spellwake_offline_v3")


def archived_v4_api(args, cleanup):
    """Load only the digest-pinned historical v4 validator and its exact source tree."""
    require(
        args.action in ("validate", "export", "status"),
        "historical v4 databases are read-only",
    )
    archive = "v4-review-sources.json.gz"
    path = next(
        (
            p
            for p in (args.db.parent / archive, AUDIT / "label-rerun-v4" / archive)
            if p.is_file()
        ),
        None,
    )
    require(path, "missing historical v4 source snapshot")
    with gzip.open(path, "rb") as stream:
        data = stream.read(2_000_001)
    require(
        len(data) <= 2_000_000
        and digest(data)
        == "95465e1857097f41abf3d99794ac5436c5ae9cc4eaafd17ff6ee46bb01e7ab3d",
        "historical v4 source digest mismatch",
    )
    folder = pathlib.Path(
        cleanup.enter_context(
            tempfile.TemporaryDirectory(prefix="spellwake-v4-offline-")
        )
    )
    for relative, text in json.loads(data).items():
        target = folder / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text, encoding="utf-8")
    # The old script directly imports its sibling operation catalogue. Temporarily
    # expose the pinned sibling, even if a different live module is already loaded.
    module_name = "card_review_operations"
    previous = sys.modules.get(module_name)
    module = types.ModuleType(module_name)
    module.__dict__.update(
        runpy.run_path(str(folder / "scripts" / "card_review_operations.py"))
    )
    sys.modules[module_name] = module
    try:
        return runpy.run_path(
            str(folder / "scripts" / "card-support-agent.py"),
            run_name="spellwake_offline_v4",
        )
    finally:
        if previous is None:
            del sys.modules[module_name]
        else:
            sys.modules[module_name] = previous


def frozen_source(args):
    return {
        "version": VERSION,
        "queue_sha256": sha_file(args.input),
        "rules_sha256": sha_file(args.rules),
        "semantic_source_sha256": {
            name: sha_file(ROOT / name)
            for name in sorted(
                set(semantic_source_paths()) | {"scripts/card-support-agent.py"}
            )
        },
        "audit_input_sha256": {
            name: sha_file(args.evidence_dir / name) for name in AUDIT_INPUTS
        },
        "harness_sha256": sha_file(pathlib.Path(__file__)),
        "prompt_sha256": digest(SYSTEM.encode()),
    }


def review_one(args, card, indexes):
    source_units = units(card)
    choices = {u["unit"]: candidates(card, u, *indexes, args.top) for u in source_units}
    answers = []
    metrics = []
    for pos in range(0, len(source_units), args.chunk):
        batch = source_units[pos : pos + args.chunk]
        data, perf = infer(args, card, batch, choices, indexes)
        answers.extend(validate_reply(data, card, batch, choices))
        metrics.append(perf)
    return make_record(card, source_units, answers, metrics, indexes)


def make_record(card, source_units, answers, metrics, indexes):
    return {
        "oracle_id": card["oracle_id"],
        "name": card["name"],
        "input_sha256": digest(dumps(card).encode()),
        "source": card,
        "evidence": "unverified-model-proposal",
        "units": source_units,
        "reviews": answers,
        "paragraph_probes": [
            probe_for_unit(card, unit, indexes[1].paragraph_probes)
            for unit in source_units
        ],
        "references": [
            {
                "unit": v["unit"],
                "rules": links(v["rule_ids"], indexes[0]),
                "mechanics": links(v["engine_symbols"], indexes[1]),
            }
            for v in answers
        ],
        "generation": metrics,
    }


# One complete source card is committed only after every required unit has passed.
# A long card spans multiple calls. Other cards can share each bounded request.
BATCH_SYSTEM = (
    SYSTEM.replace(
        "Write exactly one JSON object with oracle_id and reviews.",
        "Return a JSON object keyed by the exact supplied Oracle IDs. Each value is a separate object with oracle_id and reviews.",
    )
    + "\nKeep each behavior under 550 characters and each scenario under 950. Never transfer rule or engine candidates between units or cards."
)


def packet_plan(cards, max_cards, max_units):
    group = []
    count = 0
    for card in cards:
        blocks = units(card)
        for pos in range(0, len(blocks), max_units):
            block = blocks[pos : pos + max_units]
            if group and (count + len(block) > max_units or len(group) >= max_cards):
                yield group
                group = []
                count = 0
            group.append((card, block))
            count += len(block)
    if group:
        yield group


def prepared_packet(args, group, indexes):
    choices = {}
    fields = {}
    prompts = []
    for card, block in group:
        oid = card["oracle_id"]
        choices[oid] = {
            u["unit"]: candidates(card, u, *indexes, args.top) for u in block
        }
        prompts.append(prompt_for(card, block, choices[oid], indexes))
        fields[oid] = schema(block, card, choices[oid])
    shape = {
        "type": "object",
        "additionalProperties": False,
        "required": list(fields),
        "properties": fields,
    }
    prompt = {"cards": prompts}
    membership = {c["oracle_id"]: [u["unit"] for u in b] for c, b in group}
    return choices, fields, shape, prompt, membership


def packet_inference(args, db, group, indexes):
    choices, fields, shape, prompt, membership = prepared_packet(args, group, indexes)
    packet_hash = digest(
        dumps({"system": BATCH_SYSTEM, "prompt": prompt, "schema": shape}).encode()
    )
    with db:
        request = db.execute(
            "INSERT INTO calls(members,state,metadata) VALUES (?,'started',?)",
            (dumps(membership), dumps({"packet_sha256": packet_hash})),
        ).lastrowid

    def record_usage(usage, metadata, output_bytes):
        with db:
            db.execute(
                "UPDATE calls SET usage=?,metadata=?,output_bytes=? WHERE id=?",
                (
                    dumps(usage),
                    dumps({**metadata, "packet_sha256": packet_hash}),
                    output_bytes,
                    request,
                ),
            )

    args.record_usage = record_usage
    try:
        started = time.monotonic()
        result = generated(args, BATCH_SYSTEM, prompt, shape, args.output_tokens)
        content = result["message"]["content"]
        require(
            len(content.encode())
            <= max(8192, 12288 * sum(map(len, membership.values()))),
            "batch response exceeds bounded size",
        )
        decoded = json.loads(content, object_pairs_hook=unique_keys)
        require(
            isinstance(decoded, dict) and set(decoded) == set(fields),
            "batch response Oracle ID membership mismatch",
        )
        # All card/unit coordinates must be exact before any proposal can enter the database.
        answers = {}
        errors = {}
        for card, block in group:
            oid = card["oracle_id"]
            try:
                answers[oid] = validate_reply(
                    decode_reply(decoded[oid], block), card, block, choices[oid]
                )
            except (ValueError, TypeError, KeyError) as err:
                errors[oid] = str(err)
        outcome = "partial" if errors else "complete"
        with db:
            db.execute(
                "UPDATE calls SET state=?,error=? WHERE id=?",
                (outcome, dumps(errors) if errors else None, request),
            )
        receipt = {
            "call_id": request,
            "shared_with": list(fields),
            "total_duration": result["total_duration"],
            "prompt_eval_count": result["prompt_eval_count"],
            "eval_count": result["eval_count"],
        }
        return answers, errors, receipt
    except (ProviderUnavailable, ValueError, KeyError, TypeError) as err:
        with db:
            db.execute(
                "UPDATE calls SET state='stopped',error=? WHERE id=?",
                (str(err)[:1200], request),
            )
        raise
    finally:
        with db:
            db.execute(
                "UPDATE calls SET seconds=? WHERE id=?",
                (round(time.monotonic() - started, 3), request),
            )
        del args.record_usage


def unique_keys(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate JSON object key")
        result[key] = value
    return result


def scheduled_packets(args, db, groups, indexes, bad):
    """Only the supervisor schedules work. An in-flight request always keeps its receipt."""
    workers = getattr(args, "parallel", 1)
    errors = (ProviderUnavailable, ValueError, TypeError, KeyError)
    if workers == 1:
        for group in groups:
            if getattr(args, "stop_dispatch", False):
                return
            group = [(c, b) for c, b in group if c["oracle_id"] not in bad]
            if not group:
                continue
            try:
                result = packet_inference(args, db, group, indexes)
            except errors as err:
                yield group, None, err
            else:
                yield group, result, None
        return

    def submit_packet(group):
        options = argparse.Namespace(**vars(args))
        with contextlib.closing(sqlite3.connect(args.db, timeout=60)) as connection:
            return packet_inference(options, connection, group, indexes)

    # Each parallel request contains only whole cards. No two packets may own a shared ID.
    seen = set()
    with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as pool:
        pending = {}
        while True:
            while len(pending) < workers and not getattr(args, "stop_dispatch", False):
                group = next(groups, None)
                if group is None:
                    break
                ids = {c["oracle_id"] for c, b in group}
                require(not seen.intersection(ids), "parallel packets overlap")
                seen.update(ids)
                pending[pool.submit(submit_packet, group)] = group
            if not pending:
                return
            ready, _ = concurrent.futures.wait(
                pending, return_when=concurrent.futures.FIRST_COMPLETED
            )
            for future in ready:
                group = pending.pop(future)
                try:
                    result = future.result()
                except errors as err:
                    yield group, None, err
                else:
                    yield group, result, None


def run_batches(args, db, cards, indexes, source):
    db.execute("""CREATE TABLE IF NOT EXISTS calls (
        id INTEGER PRIMARY KEY, started_utc TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
        members TEXT NOT NULL, state TEXT NOT NULL, usage TEXT, metadata TEXT, output_bytes INTEGER,
        seconds REAL, error TEXT)""")
    complete = {row[0] for row in db.execute("SELECT oracle_id FROM proposals")}
    rejected = (
        set()
        if args.retry_failed
        else {row[0] for row in db.execute("SELECT oracle_id FROM failures")}
    )
    ids = [
        oid
        for oid in dict.fromkeys(args.card_id or cards)
        if oid not in complete and oid not in rejected
    ]
    if args.limit:
        ids = ids[: args.limit]
    fragments = {}
    measurements = {}
    bad = set()
    accepted = 0
    halted = None
    start = time.monotonic()
    consecutive_bad = 0
    if getattr(args, "parallel", 1) > 1:
        require(
            all(len(units(cards[i])) <= args.units_per_call for i in ids),
            "parallel batches require whole cards per packet; use one worker or increase --units-per-call",
        )
    groups = packet_plan(
        (cards[i] for i in ids), args.cards_per_call, args.units_per_call
    )
    for group, outcome, error in scheduled_packets(args, db, groups, indexes, bad):
        if error is not None:
            halted = halted or str(error)[:1200]
            args.stop_dispatch = True
            continue
        answers, errors, receipt = outcome
        for oid, error in errors.items():
            bad.add(oid)
            with db:
                db.execute(
                    "INSERT INTO failures VALUES (?,1,?) ON CONFLICT(oracle_id) DO UPDATE SET attempts=attempts+1,last_error=excluded.last_error",
                    (oid, error[:1000]),
                )
        for oid, answer in answers.items():
            results = fragments.setdefault(oid, [])
            results.extend(answer)
            metrics = measurements.setdefault(oid, [])
            metrics.append(receipt)
            slots = units(cards[oid])
            if len(results) < len(slots):
                continue
            record = make_record(cards[oid], slots, results, metrics, indexes)
            record["producer_sha256"] = source["harness_sha256"]
            check_record(cards[oid], record, indexes, args.top)
            with db:
                db.execute("INSERT INTO proposals VALUES (?,?)", (oid, dumps(record)))
                db.execute("DELETE FROM failures WHERE oracle_id=?", (oid,))
            accepted += 1
            del fragments[oid], measurements[oid]
        print(
            dumps(
                {
                    "call": receipt["call_id"],
                    "cards_completed": accepted,
                    "rejected": len(bad),
                    "source_units_submitted": sum(len(b) for c, b in group),
                    "elapsed_seconds": round(time.monotonic() - start, 2),
                }
            ),
            flush=True,
        )
        consecutive_bad = consecutive_bad + 1 if errors and not answers else 0
        if consecutive_bad >= 3:
            halted = "Three consecutive batches with no valid proposals"
            args.stop_dispatch = True
    if halted:
        with db:
            db.execute("INSERT INTO provider_stops(reason) VALUES (?)", (halted,))
    print(
        dumps(
            {
                "accepted_proposals": accepted,
                "failed_cards": len(bad),
                "provider_stop": halted,
                "elapsed_seconds": round(time.monotonic() - start, 2),
                "qualified_by_agent": 0,
            }
        ),
        flush=True,
    )
    if bad or halted:
        sys.exit(2)


def check_record(card, record, indexes, top):
    expected = units(card)
    require(
        record["evidence"] == "unverified-model-proposal"
        and record["source"] == card
        and record["name"] == card["name"]
        and record["input_sha256"] == digest(dumps(card).encode()),
        "source identity mismatch",
    )
    require(record["units"] == expected, "source coordinate/coverage mismatch")
    require(
        record.get("paragraph_probes")
        == [
            probe_for_unit(card, unit, indexes[1].paragraph_probes) for unit in expected
        ],
        "scoped paragraph probe differs from sealed native evidence",
    )
    choices = {u["unit"]: candidates(card, u, *indexes, top) for u in expected}
    reviews = validate_reply(
        {"oracle_id": record["oracle_id"], "reviews": record["reviews"]},
        card,
        expected,
        choices,
    )
    refs = [
        {
            "unit": v["unit"],
            "rules": links(v["rule_ids"], indexes[0]),
            "mechanics": links(v["engine_symbols"], indexes[1]),
        }
        for v in reviews
    ]
    require(refs == record["references"], "invalid source links")
    return len(reviews)


# Independent fixed answers are held outside the prompt. These controls cover two cards;
# passing them establishes only this regression set, never corpus-wide accuracy.
CONTROLS = (
    (
        "ground_blocks_crow",
        "Player A attacks with Storm Crow. B declares an otherwise unrestricted creature with neither flying nor reach as its blocker. Is that block legal?",
        False,
    ),
    (
        "crow_blocks_ground",
        "A attacks with an otherwise unrestricted ground Soldier. B declares Storm Crow as its blocker. Is that block legal?",
        True,
    ),
    (
        "reach_blocks_crow",
        "A attacks with Storm Crow. B declares an otherwise unrestricted creature with reach as its blocker. Is that block legal?",
        True,
    ),
    (
        "one_target_two_damage",
        "A casts Fire choosing one legal target, announcing 2 damage to it. Is this allocation legal?",
        True,
    ),
    (
        "two_targets_zero_share",
        "A casts Fire choosing two legal targets and announcing 2 damage to the first and 0 to the second. Is this allocation legal?",
        False,
    ),
    (
        "two_targets_equal",
        "A casts Fire with two distinct legal targets and announces one damage to each. Is this legal?",
        True,
    ),
    (
        "ice_target_departed",
        "A casts Ice as its sole chosen half. Its only target has left the battlefield before resolution. Does A draw from Ice?",
        False,
    ),
    (
        "ice_already_tapped",
        "A casts Ice on a permanent already tapped that stays a legal target. Does A draw on resolution?",
        True,
    ),
    (
        "fire_also_draws",
        "A casts Fire as its sole chosen half and deals its damage. Does the Ice half also let A draw a card?",
        False,
    ),
)


def controls(args):
    identity = preflight(args)
    index, _ = indices(args.rules)
    rules = {
        k: index.rows[k]["text"]
        for k in (
            "702.9b",
            "702.17b",
            "601.2d",
            "608.2b",
            "701.26a",
            "709.3",
            "709.3a",
            "709.3b",
        )
    }
    cards = load_queue(args.input)
    sources = [
        cards[i]
        for i in (
            "000d5588-5a4c-434e-988d-396632ade42c",
            "ae92942b-919c-4ea9-b693-85fcef765d5a",
        )
    ]
    questions = {k: q for k, q, _ in CONTROLS}
    expected = {k: v for k, _, v in CONTROLS}
    shape = {
        "type": "object",
        "additionalProperties": False,
        "required": list(questions),
        "properties": {k: {"type": "boolean"} for k in questions},
    }
    message = {
        "cards": [c["faces"] for c in sources],
        "rule_excerpts": rules,
        "questions": questions,
    }
    answer = generated(
        args,
        "Apply the supplied Magic rules to each concrete scenario. Return only the JSON boolean answer for each named question. True means yes and false means no.",
        message,
        shape,
        600,
    )
    require(
        answer.get("done") and answer.get("done_reason") != "length",
        "incomplete control reply",
    )
    observed = json.loads(answer["message"]["content"])
    require(
        isinstance(observed, dict)
        and set(observed) == set(expected)
        and all(type(v) is bool for v in observed.values()),
        "invalid control reply",
    )
    failed = [k for k in expected if expected[k] != observed[k]]
    print(
        dumps(
            {
                "version": VERSION,
                **identity,
                "rules_sha256": sha_file(args.rules),
                "queue_sha256": sha_file(args.input),
                "checks": [
                    {
                        "id": k,
                        "question": q,
                        "expected": v,
                        "observed": observed[k],
                        "passed": v == observed[k],
                    }
                    for k, q, v in CONTROLS
                ],
                "passed": len(CONTROLS) - len(failed),
                "total": len(CONTROLS),
                "qualified_cards": 0,
                "usage": answer.get("usage"),
                "provider_metadata": answer.get("provider_metadata"),
                "elapsed_seconds": round(answer.get("total_duration", 0) / 1e9, 2),
            }
        ),
        flush=True,
    )
    return 2 if failed else 0


def call_status(db):
    tables = {
        r[0] for r in db.execute("SELECT name FROM sqlite_master WHERE type='table'")
    }
    if "calls" not in tables:
        return {}
    states = collections.Counter()
    usage = collections.Counter()
    metered = 0
    duration = 0.0
    for state, seconds, raw in db.execute("SELECT state,seconds,usage FROM calls"):
        states[state] += 1
        duration += seconds or 0
        if raw:
            metrics = json.loads(raw)
            if metrics:
                metered += 1
            usage.update({k: v for k, v in metrics.items() if type(v) is int})
    return {
        "provider_attempts": sum(states.values()),
        "attempt_states": dict(states),
        "attempts_with_usage": metered,
        "cli_usage": dict(usage),
        "provider_seconds": round(duration, 3),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "action",
        choices=[
            "preflight",
            "controls",
            "preview",
            "run",
            "status",
            "validate",
            "export",
        ],
    )
    parser.add_argument(
        "--input", type=pathlib.Path, default=AUDIT / "ai-review-input.jsonl"
    )
    parser.add_argument(
        "--db", type=pathlib.Path, default=AUDIT / "agent-intake-v4.1.sqlite3"
    )
    parser.add_argument("--rules", type=pathlib.Path, default=RULE_FILE)
    parser.add_argument(
        "--evidence-dir",
        type=pathlib.Path,
        default=AUDIT,
        help="directory containing the measured coverage audit and sealed native evidence",
    )
    parser.add_argument(
        "--provider", choices=["ollama", "chatgpt-codex"], default="ollama"
    )
    parser.add_argument(
        "--effort",
        choices=["low", "medium", "high", "xhigh"],
        help="subscription reasoning effort (ChatGPT default: high)",
    )
    parser.add_argument(
        "--parallel",
        type=int,
        default=1,
        choices=[1, 2, 3],
        help="bounded simultaneous disjoint subscription requests under a single persistence supervisor",
    )
    parser.add_argument(
        "--cards-per-call",
        type=int,
        default=1,
        help="pack this many distinct cards into one authenticated subscription call",
    )
    parser.add_argument(
        "--units-per-call",
        type=int,
        default=12,
        help="maximum reviewed source units in a multi-card inference request",
    )
    parser.add_argument(
        "--manifest",
        type=pathlib.Path,
        help="ordered list of authorized cards in an ID-pinned sample",
    )
    parser.add_argument(
        "--model",
        help="exact Ollama model tag or ChatGPT subscription model (default: gpt-6.2-sol)",
    )
    parser.add_argument("--port", type=int, default=11434)
    batch = parser.add_mutually_exclusive_group()
    batch.add_argument("--limit", type=int, help="maximum cards attempted in this run")
    batch.add_argument(
        "--all",
        action="store_true",
        help="review every outstanding card, with a checkpoint after each card",
    )
    parser.add_argument(
        "--card-id", action="append", default=[], help="limit to a repeatable Oracle ID"
    )
    parser.add_argument(
        "--chunk", type=int, default=2, help="source units per inference request"
    )
    parser.add_argument(
        "--top",
        type=int,
        default=5,
        help="maximum retrieved candidates of each type per unit",
    )
    parser.add_argument("--context", type=int, default=6144)
    parser.add_argument("--output-tokens", type=int, default=1000)
    parser.add_argument("--timeout", type=int, default=180)
    parser.add_argument(
        "--retry-failed",
        action="store_true",
        help="include prior rejected attempts on this run",
    )
    args = parser.parse_args()
    if args.provider == "chatgpt-codex" and args.action in (
        "run",
        "controls",
        "preflight",
        "preview",
    ):
        args.model = args.model or "gpt-6.2-sol"
        args.effort = args.effort or "high"
    require(
        1 <= args.port <= 65535
        and 1 <= args.chunk <= 8
        and 1 <= args.top <= 12
        and 128 <= args.output_tokens < args.context
        and 1024 <= args.context <= 32768
        and args.timeout > 0,
        "invalid resource limit",
    )
    require(
        1 <= args.cards_per_call <= 12 and 1 <= args.units_per_call <= 20,
        "invalid batch bounds",
    )
    if args.parallel > 1:
        require(
            args.cards_per_call > 1, "parallelism requires multi-card ChatGPT packets"
        )
    if args.cards_per_call > 1:
        require(
            args.action == "preview"
            or (args.provider == "chatgpt-codex" and args.model and args.effort),
            "multi-card batches require explicit ChatGPT model and reasoning effort",
        )
    if args.action == "controls":
        sys.exit(controls(args))
    if args.action == "preflight":
        print(dumps(preflight(args)))
        return
    if args.action == "preview":
        require(
            (args.manifest or args.card_id) and not args.all,
            "offline preview requires explicit selected cards",
        )
    elif args.action == "run":
        require(
            args.all or (args.limit and args.limit > 0),
            "choose --all or a positive --limit",
        )
        args.db.parent.mkdir(parents=True, exist_ok=True)
    else:
        require(args.db.is_file(), "no existing intake database")
    state = sqlite3.connect(
        ":memory:"
        if args.action == "preview"
        else (
            args.db if args.action == "run" else args.db.resolve().as_uri() + "?mode=ro"
        ),
        uri=args.action != "run",
    )
    with contextlib.closing(state) as db, contextlib.ExitStack() as cleanup:
        if args.action == "run":
            lock = cleanup.enter_context(
                args.db.with_suffix(args.db.suffix + ".lock").open("a+b")
            )
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        cards = load_queue(args.input)
        if args.manifest:
            sample = json.loads(args.manifest.read_text())
            designated = [row["oracle_id"] for row in sample["cards"]]
            require(
                sample["catalogue_queue_sha256"] == sha_file(args.input)
                and len(set(designated)) == len(designated)
                and set(designated) <= cards.keys(),
                "sample identity mismatch",
            )
            require(set(args.card_id) <= set(designated), "requested ID outside sample")
            args.card_id = args.card_id or designated
        require(set(args.card_id) <= cards.keys(), "unknown requested Oracle ID")
        prior_manifest = None
        if db.execute(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='manifest'"
        ).fetchone():
            saved = db.execute("SELECT value FROM manifest WHERE id=1").fetchone()
            prior_manifest = json.loads(saved[0]) if saved else None
        legacy = None
        if prior_manifest:
            if prior_manifest.get("version") == "oracle-intake/v4":
                legacy = archived_v4_api(args, cleanup)
            elif prior_manifest.get("version") == "oracle-intake/v3":
                legacy = legacy_api(args, cleanup)
            else:
                require(
                    prior_manifest.get("version") == VERSION,
                    "unsupported historical review version",
                )
        if legacy:
            source = legacy["frozen_source"](args)
            indexes = legacy["indices"](args.rules)
            if prior_manifest["version"] == "oracle-intake/v4":
                indexes[1].card_evidence = legacy["measured_card_evidence"](
                    args, cards, source
                )
            validate_record = legacy["check_record"]
        else:
            source = frozen_source(args)
            indexes = indices(args.rules)
            indexes[1].card_evidence = measured_card_evidence(args, cards, source)
            indexes[1].paragraph_probes = load_paragraph_probes(
                cards, args.evidence_dir / "clauses.tsv"
            )
            require(
                source == frozen_source(args),
                "evidence or source changed while building the review index",
            )
            validate_record = check_record
        if args.action == "preview":
            selected = [cards[oid] for oid in dict.fromkeys(args.card_id)]
            if args.limit:
                selected = selected[: args.limit]
            if args.cards_per_call > 1:
                for group in packet_plan(
                    selected, args.cards_per_call, args.units_per_call
                ):
                    _, _, shape, prompt, membership = prepared_packet(
                        args, group, indexes
                    )
                    packet = {"system": BATCH_SYSTEM, "prompt": prompt, "schema": shape}
                    print(
                        dumps(
                            {
                                "version": VERSION,
                                "source": source,
                                "members": membership,
                                "packet_sha256": digest(dumps(packet).encode()),
                                **packet,
                            }
                        )
                    )
            else:
                for card in selected:
                    slots = units(card)
                    oid = card["oracle_id"]
                    for pos in range(0, len(slots), args.chunk):
                        block = slots[pos : pos + args.chunk]
                        choices = {
                            u["unit"]: candidates(card, u, *indexes, args.top)
                            for u in block
                        }
                        packet = {
                            "system": SYSTEM,
                            "prompt": prompt_for(card, block, choices, indexes),
                            "schema": schema(block, card, choices),
                        }
                        print(
                            dumps(
                                {
                                    "version": VERSION,
                                    "source": source,
                                    "members": {oid: [u["unit"] for u in block]},
                                    "packet_sha256": digest(dumps(packet).encode()),
                                    **packet,
                                }
                            )
                        )
            return
        if args.action == "run":
            identity = preflight(args)
            config = {
                **identity,
                "chunk": args.chunk,
                "top": args.top,
                "num_ctx": args.context,
                "num_predict": args.output_tokens,
            }
            if args.cards_per_call > 1:
                config.update(
                    cards_per_call=args.cards_per_call,
                    units_per_call=args.units_per_call,
                    batch_prompt_sha256=digest(BATCH_SYSTEM.encode()),
                )
                if args.manifest:
                    config["sample_sha256"] = sha_file(args.manifest)
            store(db, source, config)
        else:
            manifest = json.loads(
                db.execute("SELECT value FROM manifest WHERE id=1").fetchone()[0]
            )
            require(
                all(
                    manifest.get(k) == v
                    for k, v in source.items()
                    if k != "harness_sha256"
                ),
                "source, rulebook, implementation or harness digest changed",
            )
            args.top = manifest["top"]
        if args.action == "status":
            complete = db.execute("SELECT COUNT(*) FROM proposals").fetchone()[0]
            failed = db.execute("SELECT COUNT(*) FROM failures").fetchone()[0]
            print(
                dumps(
                    {
                        "cards": len(cards),
                        "proposals": complete,
                        "failed": failed,
                        "unreviewed": len(cards) - complete,
                        "qualified_by_agent": 0,
                        "manifest": manifest,
                        **call_status(db),
                    }
                )
            )
            return
        if args.action in ("validate", "export"):
            checked = 0
            clauses = 0
            for oid, raw in db.execute(
                "SELECT oracle_id,value FROM proposals ORDER BY oracle_id"
            ):
                require(oid in cards, "unknown stored Oracle ID")
                record = json.loads(raw)
                require(record["oracle_id"] == oid, "database key mismatch")
                clauses += validate_record(cards[oid], record, indexes, args.top)
                checked += 1
                if args.action == "export":
                    print(
                        dumps(
                            {
                                "version": manifest["version"],
                                "manifest": manifest,
                                "proposal": record,
                            }
                        )
                    )
            if args.action == "validate":
                print(
                    dumps(
                        {
                            "validated_proposals": checked,
                            "validated_source_units": clauses,
                            "cards": len(cards),
                            "qualified_by_agent": 0,
                        }
                    )
                )
            return
        if args.cards_per_call > 1:
            return run_batches(args, db, cards, indexes, source)
        successes = 0
        failures = 0
        consecutive = 0
        halted = None
        seen = set()
        start = time.monotonic()
        for oid in args.card_id or cards:
            if oid in seen:
                continue
            seen.add(oid)
            if db.execute(
                "SELECT 1 FROM proposals WHERE oracle_id=?", (oid,)
            ).fetchone():
                continue
            if (
                not args.retry_failed
                and db.execute(
                    "SELECT 1 FROM failures WHERE oracle_id=?", (oid,)
                ).fetchone()
            ):
                continue
            try:
                record = review_one(args, cards[oid], indexes)
                record["producer_sha256"] = source["harness_sha256"]
                count = check_record(cards[oid], record, indexes, args.top)
                with db:
                    db.execute(
                        "INSERT INTO proposals VALUES (?,?)", (oid, dumps(record))
                    )
                    db.execute("DELETE FROM failures WHERE oracle_id=?", (oid,))
                successes += 1
                consecutive = 0
                print(
                    dumps(
                        {
                            "proposed": oid,
                            "name": cards[oid]["name"],
                            "source_units": count,
                            "seconds": round(
                                sum(m["total_duration"] for m in record["generation"])
                                / 1e9,
                                2,
                            ),
                        }
                    ),
                    flush=True,
                )
            except ProviderUnavailable as err:
                halted = str(err)[:1200]
                with db:
                    db.execute(
                        "INSERT INTO provider_stops(reason) VALUES (?)", (halted,)
                    )
                break
            except (
                ValueError,
                KeyError,
                TypeError,
                OSError,
                http.client.HTTPException,
            ) as err:
                with db:
                    db.execute(
                        "INSERT INTO failures VALUES (?,1,?) ON CONFLICT(oracle_id) DO UPDATE SET attempts=attempts+1,last_error=excluded.last_error",
                        (oid, str(err)[:1000]),
                    )
                failures += 1
                print(
                    dumps({"failed": oid, "error": str(err)[:1000]}),
                    file=sys.stderr,
                    flush=True,
                )
                consecutive += 1
                if consecutive >= 3:
                    halted = "Three consecutive invalid proposals; inspect failure receipts before resuming"
                    with db:
                        db.execute(
                            "INSERT INTO provider_stops(reason) VALUES (?)", (halted,)
                        )
                    break
            if args.limit and successes + failures >= args.limit:
                break
        print(
            dumps(
                {
                    "accepted_proposals": successes,
                    "failed_attempts": failures,
                    "provider_stop": halted,
                    "elapsed_seconds": round(time.monotonic() - start, 2),
                    "qualified_by_agent": 0,
                }
            ),
            flush=True,
        )
        if failures or halted:
            sys.exit(2)


if __name__ == "__main__":
    try:
        main()
    except (
        ValueError,
        KeyError,
        TypeError,
        OSError,
        sqlite3.Error,
        http.client.HTTPException,
        ProviderUnavailable,
    ) as error:
        sys.exit(str(error))
