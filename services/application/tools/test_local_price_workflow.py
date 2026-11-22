"""Opt-in real Temporal restart -> real local Supabase extraction qualification.

Requires the owned empty Supabase stack and privately captured local status.
Only processes created by this invocation are stopped. No hosted services.
"""

import base64
from datetime import datetime, timedelta, timezone
import fcntl
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import sys
import time
import uuid

from test_local_extraction import qualify, sql, literal
from test_local_storage import ROOT
from test_local_deletion_delivery import qualify_delivery
from test_local_deletion_database import qualify_database
from test_live_price_day import qualify_live_day


def stop(process: subprocess.Popen[bytes] | None) -> None:
    """Stop only this run's actual child handle, never by executable/name scan."""
    if process is not None and process.poll() is None:
        process.send_signal(signal.SIGINT)
        try:
            process.wait(timeout=35)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=10)


def main() -> None:
    """Use one owned namespace/store and prove queued work survives a restart."""
    temporal = shutil.which("temporal")
    if temporal is None:
        raise RuntimeError("Temporal CLI required")
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 57233))
    if sys.argv[1:] not in ([], ["--temporal-only"], ["--real-deletion"], ["--live-day"]):
        raise RuntimeError("Use no arguments, --temporal-only, --real-deletion or --live-day")
    temporal_only = bool(sys.argv[1:])
    real_deletion = sys.argv[1:] == ["--real-deletion"]
    live_day = sys.argv[1:] == ["--live-day"]
    for binary in ([] if temporal_only else ["price_worker", "price_workflow"]):
        if not (ROOT / "target/debug" / binary).is_file():
            raise RuntimeError("Build current native worker binaries first")
    stage = ROOT / "target" / ("temporal-qualification-" + uuid.uuid4().hex)
    stage.mkdir(mode=0o700)
    (stage / "empty.yaml").write_text("{}")
    namespace = "spellwake-price-test-" + uuid.uuid4().hex
    isolated = ["--disable-config-env", "--disable-config-file", "--env-file", str(stage / "empty.yaml")]
    cli_environment = {key: value for key, value in os.environ.items() if not key.startswith("TEMPORAL_")}
    server: subprocess.Popen[bytes] | None = None
    worker: subprocess.Popen[bytes] | None = None
    logs = []

    def log(name: str):
        path = stage / name
        path.touch(mode=0o600)
        stream = path.open("ab")
        logs.append(stream)
        return stream

    def cli(*arguments: str) -> dict:
        result = subprocess.run([temporal, *arguments, *isolated, "--address", "127.0.0.1:57233",
                                 "--namespace", namespace, "--output", "json", "--command-timeout", "10s"],
                                env=cli_environment, capture_output=True, timeout=15, check=True)
        return json.loads(result.stdout) if result.stdout.strip() else {}

    def start_server() -> subprocess.Popen[bytes]:
        process = subprocess.Popen([temporal, "server", "start-dev", *isolated, "--ip", "127.0.0.1",
                                    "--port", "57233", "--namespace", namespace, "--headless",
                                    "--db-filename", str(stage / "temporal.db")], env=cli_environment,
                                   stdout=log("server.log"), stderr=subprocess.STDOUT)
        deadline = time.monotonic() + 45
        while time.monotonic() < deadline:
            if process.poll() is not None:
                raise RuntimeError("Owned Temporal server exited during startup")
            try:
                cli("operator", "cluster", "health")
                return process
            except (subprocess.SubprocessError, json.JSONDecodeError):
                time.sleep(0.5)
        stop(process)
        raise RuntimeError("Owned Temporal server readiness timed out")

    def dispatch(slot: str, environment: dict[str, str]) -> str:
        nonlocal server, worker
        environment = environment.copy()
        environment.update(SPELLWAKE_TEMPORAL_ADDRESS="http://127.0.0.1:57233", SPELLWAKE_TEMPORAL_NAMESPACE=namespace)
        environment.pop("SPELLWAKE_TEMPORAL_API_KEY", None)

        def command(action: str, day: str, success: bool = True) -> dict:
            result = subprocess.run([str(ROOT / "target/debug/price_workflow"), action, day], env=environment,
                                    capture_output=True, timeout=60)
            payload = json.loads(result.stdout)
            if (result.returncode == 0) != success or payload["status"] != ("success" if success else "error"):
                raise RuntimeError("Unexpected workflow command disposition")
            return payload

        identifier = "spellwake-price-day-v1:" + slot
        command("start-day", slot)  # No worker exists yet: actual durable queued job.
        cli("workflow", "terminate", "--workflow-id", identifier, "--reason", "owned fixture recovery test")
        command("start-day", slot)  # Failed/terminated IDs can be resumed safely.
        queued = cli("workflow", "show", "--workflow-id", identifier)
        if not queued.get("events"):
            raise RuntimeError("Queued workflow history missing")
        stop(server)
        server = start_server()  # Same owned SQLite store and namespace.
        worker = subprocess.Popen([str(ROOT / "target/debug/price_worker")], env=environment,
                                  stdout=log("worker.log"), stderr=subprocess.STDOUT)
        result = command("run-day", slot)
        if command("run-day", slot) != result:
            raise RuntimeError("Repeated completed dispatch changed its selected source")
        history = cli("workflow", "show", "--workflow-id", identifier)
        names = [event["activityTaskScheduledEventAttributes"]["activityType"]["name"] for event in history["events"]
                 if "activityTaskScheduledEventAttributes" in event]
        if names != ["spellwake-price-collect-v1", "spellwake-price-extract-v1"]:
            raise RuntimeError("Unexpected real workflow activity sequence")
        encoded = json.dumps(history).encode()
        secret = environment["SPELLWAKE_SUPABASE_SECRET_KEY"].encode()
        if secret in encoded or base64.b64encode(secret) in encoded:
            raise RuntimeError("Private backend credential entered workflow history")
        (stage / "day-history.json").write_bytes(encoded)
        tomorrow = (datetime.now(timezone.utc) + timedelta(days=1)).date().isoformat()
        installed = command("schedule", tomorrow)
        if command("schedule", tomorrow) != installed:
            raise RuntimeError("Calendar installation did not reconcile")
        different = (datetime.now(timezone.utc) + timedelta(days=2)).date().isoformat()
        command("schedule", different, success=False)
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            calendar = cli("workflow", "show", "--workflow-id", "spellwake-price-calendar-v1")
            if any("timerStartedEventAttributes" in event for event in calendar["events"]):
                break
            time.sleep(0.2)
        else:
            raise RuntimeError("Calendar did not establish its future durable timer")
        (stage / "calendar-history.json").write_text(json.dumps(calendar))
        print("Real Temporal passed: queued day survives server restart; exact activities, duplicate result, private history and date-bound calendar installation.")
        return result["data"]["manifestId"]

    def settle(markets: list[str], environment: dict[str, str]) -> None:
        """Exercise real automatic lifecycle delivery with zero-position fixtures.

        Funded hold/coin conservation is tested separately by the real PG suite.
        """
        environment=environment.copy()
        environment.update(SPELLWAKE_TEMPORAL_ADDRESS="http://127.0.0.1:57233", SPELLWAKE_TEMPORAL_NAMESPACE=namespace)
        environment.pop("SPELLWAKE_TEMPORAL_API_KEY",None)
        identifier="spellwake-prediction-maintenance-v1"
        def install() -> dict:
            result=subprocess.run([str(ROOT / "target/debug/price_workflow"),"maintenance"],env=environment,capture_output=True,timeout=30,check=True)
            payload=json.loads(result.stdout)
            if payload != {"status":"success","data":{"workflowId":identifier}}:
                raise RuntimeError("Unexpected maintenance installation disposition")
            return payload
        if install()!=install():
            raise RuntimeError("Maintenance installation did not reconcile")
        expected={markets[0]:["resolved","yes"],markets[1]:["resolved","void"],markets[2]:["resolved","void"],markets[3]:["closed",None]}
        ids=",".join(map(literal,markets))
        deadline=time.monotonic()+20
        while time.monotonic()<deadline:
            actual=json.loads(sql("SELECT jsonb_object_agg(id::text,jsonb_build_array(state,outcome)) FROM app.prediction_market WHERE id IN ("+ids+")"))
            if actual==expected:
                history=cli("workflow","show","--workflow-id",identifier)
                if any("timerStartedEventAttributes" in event for event in history["events"]):
                    break
            time.sleep(0.2)
        else:
            raise RuntimeError("Automatic maintenance did not settle ready markets and retain pending evidence")
        names=[event["activityTaskScheduledEventAttributes"]["activityType"]["name"] for event in history["events"] if "activityTaskScheduledEventAttributes" in event]
        if names != ["spellwake-prediction-maintain-v1"]:
            raise RuntimeError("Unexpected maintenance activity history")
        if sql("SELECT count(*) FROM app.prediction_market WHERE id IN ("+ids+") AND settlement_operation_id IS NOT NULL")!="3":
            raise RuntimeError("Missing terminal source operations")
        encoded=json.dumps(history).encode(); secret=environment["SPELLWAKE_SUPABASE_SECRET_KEY"].encode()
        if secret in encoded or base64.b64encode(secret) in encoded:
            raise RuntimeError("Private key entered maintenance history")
        (stage / "maintenance-history.json").write_bytes(encoded)
        cli("workflow","cancel","--workflow-id",identifier)
        deadline=time.monotonic()+10
        while time.monotonic()<deadline:
            history=cli("workflow","show","--workflow-id",identifier)
            if any("workflowExecutionCanceledEventAttributes" in event for event in history["events"]):
                break
            time.sleep(0.1)
        else:
            raise RuntimeError("Maintenance timer cancellation did not finish")
        print("Real maintenance passed: automatic close/Yes/void/pending, repeat installation, next-sweep timer and cancellation (zero-position fixtures).")

    try:
        server = start_server()
        if live_day:
            qualify_live_day(ROOT,stage,namespace,cli,stop)
            return
        if not temporal_only:
            with (ROOT / "target/local-storage-test.lock").open("a") as lock:
                fcntl.flock(lock, fcntl.LOCK_EX)
                qualify(dispatch,settle)
        if real_deletion:
            with (ROOT / "target/local-storage-test.lock").open("a") as lock:
                fcntl.flock(lock, fcntl.LOCK_EX)
                qualify_database(ROOT, stage, namespace, cli, stop)
        else:
            qualify_delivery(ROOT, stage, namespace, cli, stop)
        test_environment = os.environ.copy()
        test_environment["SPELLWAKE_TEST_TEMPORAL_NAMESPACE"] = namespace
        subprocess.run(["cargo", "test", "--locked", "--lib",
                        "price_workflow::tests::temporal::real_temporal_",
                        "--", "--ignored"], cwd=ROOT, env=test_environment, check=True, timeout=120)
        probe_test = "api::tests::prediction_publication::real_temporal_preparation_api_reconciles_queued_work_and_rechecks_delivery_authority"
        listed = subprocess.check_output(["cargo", "test", "--locked", "--lib", probe_test,
                                          "--", "--list", "--ignored", "--exact"],
                                         cwd=ROOT, env=test_environment, text=True, timeout=60)
        if f"{probe_test}: test" not in listed:
            raise RuntimeError("Prediction preparation API test was not selected")
        subprocess.run(["cargo", "test", "--locked", "--lib", probe_test,
                        "--", "--ignored", "--exact"], cwd=ROOT, env=test_environment, check=True, timeout=60)
    finally:
        stop(worker)
        stop(server)
        for stream in logs:
            stream.close()


if __name__ == "__main__":
    main()
