"""POSIX process acceptance for explicit current worker and installer binaries."""
import base64
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import signal
import subprocess
import threading
import time
from typing import Callable, Any
import uuid
from test_local_price_workflow import stop


def canonical(value: Any) -> str:
    """Validate application UUID wire values, not provider identifiers."""
    parsed = uuid.UUID(value)
    if parsed.int == 0 or str(parsed) != value:
        raise RuntimeError('Noncanonical application identity')
    return value


def qualify(root: Path, stage: Path, environment: dict[str, str], cli: Callable[..., bytes], stable_sources: Callable[[], None], domain: str = 'current') -> dict[str, Any]:
    """Use only this invocation's actual child handles and owned server namespace."""
    if os.name != 'posix':
        raise RuntimeError('This signal/crash qualification requires POSIX')
    specifications = {
        'current': ('spellwake-patreon-current-v1', 'spellwake-patreon-current-refresh-v1', 'patreon_current_worker', 'patreon_current_workflow', 'read_patreon_refresh_result', 'p_association', 'spellwake-patreon-current-deliver-v1'),
    }
    if domain not in specifications:
        raise RuntimeError('Explicit member domain required')
    queue, workflow, worker_name, installer_name, recovery_rpc, membership_parameter, delivery_name = specifications[domain]
    names = [worker_name, installer_name]
    build = ['cargo', 'build', '--locked', '--manifest-path', str(root / 'Cargo.toml'), '--message-format=json']
    for name in names:
        build += ['--bin', name]
    with (stage / 'process-build.log').open('wb') as stream:
        subprocess.run(build, cwd=root, env=environment, stdout=stream, stderr=subprocess.STDOUT, timeout=180, check=True)
    stable_sources()
    binaries: dict[str, Path] = {}
    for line in (stage / 'process-build.log').read_text().splitlines():
        if not line.startswith('{'):
            continue
        artifact = json.loads(line)
        name = artifact.get('target', {}).get('name')
        if artifact.get('reason') == 'compiler-artifact' and name in names and artifact.get('executable'):
            if name in binaries:
                raise RuntimeError('Duplicate production executable artifact')
            binaries[name] = Path(artifact['executable']).resolve()
    if set(binaries) != set(names):
        raise RuntimeError('Both production executables required')
    hashes = {name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in binaries.items()}
    (stage / 'process-execution.json').write_text(json.dumps({'buildCommand': build, 'binaries': {k: str(v) for k, v in binaries.items()}, 'sha256': hashes}, indent=2) + '\n')
    state: dict[str, Any] = {'phase': 'steady', 'calls': [], 'errors': [], 'recoveries': 0}
    lock = threading.Lock()
    entered = threading.Event()
    release = threading.Event()
    association = {'steady': str(uuid.UUID(int=1001)), 'crash': str(uuid.UUID(int=1002))}

    class Handler(BaseHTTPRequestHandler):
        """Strict fixture responds only to private discovery and accepted-result reads."""
        def log_message(self, _format: str, *args: Any) -> None:
            pass

        def do_POST(self) -> None:
            gated = False
            try:
                length = int(self.headers.get('Content-Length', '-1'))
                if not 0 <= length <= 32768 or self.headers.get('apikey') != 'sb_secret_test_only' or self.headers.get('Content-Profile') != 'app' or self.headers.get('Authorization') is not None:
                    raise RuntimeError('Unexpected private request envelope')
                body = json.loads(self.rfile.read(length))
                with lock:
                    phase = state['phase']
                    actor = association[phase]
                    state['calls'].append({'phase': phase, 'path': self.path, 'body': body})
                    if self.path == '/rest/v1/rpc/read_patreon_refresh_work':
                        if set(body) != {'p_campaign', 'p_after', 'p_limit'} or body['p_campaign'] != '123' or type(body['p_limit']) is not int or body['p_limit'] != 1:
                            raise RuntimeError('Unexpected discovery parameters')
                        if body['p_after'] is None:
                            value = [actor]
                        elif canonical(body['p_after']) == actor:
                            value = []
                        else:
                            raise RuntimeError('Unexpected discovery cursor')
                    elif self.path == '/rest/v1/rpc/' + recovery_rpc:
                        if set(body) != {membership_parameter, 'p_request'} or canonical(body[membership_parameter]) != actor:
                            raise RuntimeError('Unexpected recovery identity')
                        canonical(body['p_request'])
                        state['recoveries'] += 1
                        gated = phase == 'crash' and state['recoveries'] == 1
                        value = {'associationId': actor, 'sourceEventId': str(uuid.UUID(int=2001 if phase == 'steady' else 2002)), 'outcome': 'applied'}
                    else:
                        raise RuntimeError('Unexpected provider/preparation/commit operation')
                if gated:
                    entered.set()
                    if not release.wait(180):
                        raise RuntimeError('Gated recovery was not released')
                encoded = json.dumps(value).encode()
                self.send_response(200)
                self.send_header('Content-Type', 'application/json')
                self.send_header('Content-Length', str(len(encoded)))
                self.send_header('Connection', 'close')
                self.end_headers()
                self.wfile.write(encoded)
            except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
                if not gated:
                    with lock:
                        state['errors'].append('Unexpected disconnected response')
            except Exception as error:
                with lock:
                    state['errors'].append(type(error).__name__ + ': ' + str(error))
                try:
                    self.send_error(500)
                except (BrokenPipeError, ConnectionResetError):
                    pass

    backend = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    backend.daemon_threads = True
    thread = threading.Thread(target=backend.serve_forever, daemon=True)
    thread.start()
    child_environment = {k: v for k, v in environment.items() if not k.startswith(('SPELLWAKE_', 'TEMPORAL_'))}
    child_environment.update(SPELLWAKE_SUPABASE_URL=f'http://127.0.0.1:{backend.server_port}/', SPELLWAKE_SUPABASE_SECRET_KEY='sb_secret_test_only', SPELLWAKE_PATREON_CAMPAIGN_ID='123', SPELLWAKE_PATREON_CREATOR_ACCESS_TOKEN='patreon_test_creator_synthetic', SPELLWAKE_TEMPORAL_ADDRESS=environment['SPELLWAKE_TEST_PATREON_TEMPORAL_ADDRESS'], SPELLWAKE_TEMPORAL_NAMESPACE=environment['SPELLWAKE_TEST_PATREON_TEMPORAL_NAMESPACE'], RUST_LOG='warn')
    workers: list[subprocess.Popen[bytes]] = []
    logs = []
    lifecycle: list[dict[str, Any]] = []

    def raw(*args: str) -> dict[str, Any]:
        return json.loads(cli(*args))

    def wait(predicate: Callable[[], Any], seconds: float, label: str, worker: subprocess.Popen[bytes] | None = None) -> Any:
        deadline = time.monotonic() + seconds
        while True:
            if worker is not None and worker.poll() is not None:
                raise RuntimeError(f'Owned worker exited during {label}')
            with lock:
                if state['errors']:
                    raise RuntimeError('Strict process backend rejected an operation')
            found = predicate()
            if found:
                return found
            if time.monotonic() >= deadline:
                raise RuntimeError(f'Timed out: {label}')
            time.sleep(0.1)

    def description() -> dict[str, Any]:
        return raw('workflow', 'describe', '--workflow-id', workflow)

    def history(name: str) -> dict[str, Any]:
        result = raw('workflow', 'show', '--workflow-id', workflow)
        (stage / f'{name}-history.json').write_text(json.dumps(result, indent=2) + '\n')
        return result

    def pollers() -> dict[str, Any]:
        result = raw('task-queue', 'describe', '--legacy-mode', '--task-queue', queue, '--task-queue-type-legacy', 'activity')
        (stage / 'last-pollers.json').write_text(json.dumps(result, indent=2) + '\n')
        return result

    def start_worker() -> subprocess.Popen[bytes]:
        before = pollers()
        def access_ns(poller: dict[str, Any]) -> int:
            timestamp = poller['last_access_time']
            if type(timestamp.get('seconds')) is not int or type(timestamp.get('nanos', 0)) is not int:
                raise RuntimeError('Unexpected legacy poller timestamp')
            return timestamp['seconds'] * 1_000_000_000 + timestamp.get('nanos', 0)
        prior = max((access_ns(p) for p in (before.get('pollers') or [])), default=0)
        started = time.time_ns()
        stream = (stage / f'worker-{len(workers)+1}.log').open('wb')
        logs.append(stream)
        worker = subprocess.Popen([str(binaries[worker_name])], env=child_environment, stdout=stream, stderr=subprocess.STDOUT)
        workers.append(worker)
        def ready() -> Any:
            value = pollers()
            for poller in (value.get('pollers') or []):
                observed_time = access_ns(poller)
                if poller.get('identity') == queue and observed_time > prior and observed_time >= started:
                    return value
            return None
        observed = wait(ready, 45, 'fresh actual activity poll', worker)
        (stage / f'worker-{len(workers)}-ready.json').write_text(json.dumps(observed, indent=2) + '\n')
        lifecycle.append({'pid': worker.pid, 'event': 'fresh_poll_observed'})
        return worker

    def signal_worker(worker: subprocess.Popen[bytes], sig: signal.Signals, expected: int) -> None:
        if worker.poll() is not None:
            raise RuntimeError('Owned worker already exited before intended signal')
        worker.send_signal(sig)
        code = worker.wait(timeout=30)
        lifecycle.append({'pid': worker.pid, 'signal': sig.name, 'returnCode': code})
        if code != expected:
            raise RuntimeError('Unexpected production worker signal outcome')

    def install() -> dict[str, Any]:
        result = subprocess.run([str(binaries[installer_name]), 'install'], env=child_environment, capture_output=True, text=True, timeout=30, check=True)
        output = json.loads(result.stdout)
        if output != {'status': 'success', 'data': {'workflowId': workflow}}:
            raise RuntimeError('Unexpected actual installer output')
        return description()

    def has_tail() -> Any:
        events = raw('workflow', 'show', '--workflow-id', workflow)['events']
        return events if any('timerStartedEventAttributes' in e for e in events) else None

    try:
        first = start_worker()
        empty = raw('workflow', 'list', '--query', f"WorkflowId = '{workflow}'")
        (stage / 'before-install.json').write_text(json.dumps(empty, indent=2) + '\n')
        if empty != []:
            raise RuntimeError('Worker construction installed a workflow')
        with lock:
            if state['calls']:
                raise RuntimeError('Uninstalled worker dispatched backend work')
        initial = install()
        initial_run = initial['workflowExecutionInfo']['execution']['runId']
        wait(has_tail, 30, 'initial recovery and sweep timer', first)
        signal_worker(first, signal.SIGTERM, 0)
        still = description()
        if still['workflowExecutionInfo']['status'] != 'WORKFLOW_EXECUTION_STATUS_RUNNING' or still['workflowExecutionInfo']['execution']['runId'] != initial_run:
            raise RuntimeError('SIGTERM changed the durable workflow')
        second = start_worker()
        if install()['workflowExecutionInfo']['execution']['runId'] != initial_run:
            raise RuntimeError('Repeated actual installation changed the run')
        signal_worker(second, signal.SIGINT, 0)
        after_interrupt = description()
        if after_interrupt['workflowExecutionInfo']['status'] != 'WORKFLOW_EXECUTION_STATUS_RUNNING' or after_interrupt['workflowExecutionInfo']['execution']['runId'] != initial_run:
            raise RuntimeError('SIGINT changed the durable workflow')
        history('signals')
        with lock:
            if len(state['calls']) != 3:
                raise RuntimeError('Unexpected graceful lifecycle work')
        cli('workflow', 'terminate', '--workflow-id', workflow, '--reason', 'owned graceful lifecycle cleanup')
        with lock:
            state['phase'] = 'crash'
            state['recoveries'] = 0
        third = start_worker()
        crash_run = install()['workflowExecutionInfo']['execution']['runId']
        if crash_run == initial_run:
            raise RuntimeError('Terminated workflow was not explicitly reinstalled')
        wait(entered.is_set, 30, 'gated actual recovery call', third)
        def heartbeat() -> Any:
            value = description()
            for activity in value.get('pendingActivities', []):
                if activity.get('activityType', {}).get('name') == delivery_name and activity.get('lastHeartbeatTime'):
                    if activity.get('heartbeatDetails', {}).get('payloads', []) or activity['attempt'] != 1:
                        raise RuntimeError('Unexpected active heartbeat payload/attempt')
                    return value
            return None
        before_kill = wait(heartbeat, 30, 'actual process unit heartbeat', third)
        (stage / 'before-kill.json').write_text(json.dumps(before_kill, indent=2) + '\n')
        signal_worker(third, signal.SIGKILL, -signal.SIGKILL)
        release.set()
        replacement = start_worker()
        def timed_out() -> Any:
            value = description()
            for activity in value.get('pendingActivities', []):
                failure = activity.get('lastFailure', {})
                if failure.get('timeoutFailureInfo', {}).get('timeoutType') == 'TIMEOUT_TYPE_HEARTBEAT':
                    return value
            return None
        timeout = wait(timed_out, 100, 'durable heartbeat timeout after SIGKILL', replacement)
        (stage / 'crash-timeout.json').write_text(json.dumps(timeout, indent=2) + '\n')
        wait(has_tail, 70, 'same-task process crash recovery', replacement)
        signal_worker(replacement, signal.SIGTERM, 0)
        final = description()
        if final['workflowExecutionInfo']['execution']['runId'] != crash_run or final['workflowExecutionInfo']['status'] != 'WORKFLOW_EXECUTION_STATUS_RUNNING':
            raise RuntimeError('Crash recovery changed the durable run')
        crash_history = history('crash')
        events = crash_history['events']
        delivery = [e for e in events if e.get('activityTaskScheduledEventAttributes', {}).get('activityType', {}).get('name') == delivery_name]
        if len(delivery) != 1:
            raise RuntimeError('Crash recovery regenerated a logical delivery')
        encoded = delivery[0]['activityTaskScheduledEventAttributes']['input']['payloads']
        if len(encoded) != 1 or base64.b64decode(encoded[0]['metadata']['encoding']) != b'json/plain':
            raise RuntimeError('Unexpected persisted delivery payload')
        task = json.loads(base64.b64decode(encoded[0]['data']))
        if set(task) != {'associationId', 'requestId'} or canonical(task['associationId']) != association['crash']:
            raise RuntimeError('Unexpected persisted logical task')
        canonical(task['requestId'])
        schedule_id = delivery[0]['eventId']
        starts = [e['activityTaskStartedEventAttributes'] for e in events if e.get('activityTaskStartedEventAttributes', {}).get('scheduledEventId') == schedule_id]
        if not any(start['attempt'] == 2 for start in starts) or any(start['attempt'] > 2 for start in starts):
            raise RuntimeError('Actual crashed activity did not resume attempt two')
        completed = [e['activityTaskCompletedEventAttributes'] for e in events if e.get('activityTaskCompletedEventAttributes', {}).get('scheduledEventId') == schedule_id]
        if len(completed) != 1:
            raise RuntimeError('Recovered delivery lacks one actual completion')
        payloads = completed[0]['result']['payloads']
        expected = {'associationId': association['crash'], 'sourceEventId': str(uuid.UUID(int=2002)), 'outcome': 'applied'}
        if len(payloads) != 1 or base64.b64decode(payloads[0]['metadata']['encoding']) != b'json/plain' or json.loads(base64.b64decode(payloads[0]['data'])) != expected:
            raise RuntimeError('Recovered completion differs from accepted source')
        with lock:
            calls = list(state['calls'])
            recoveries = [c['body'] for c in calls if c['phase'] == 'crash' and c['path'].endswith(recovery_rpc)]
            if len(calls) != 7 or len(recoveries) != 2 or recoveries[0] != recoveries[1] or recoveries[0] != {membership_parameter: task['associationId'], 'p_request': task['requestId']} or state['errors']:
                raise RuntimeError('Crash task identity or backend call count differs')
        cli('workflow', 'terminate', '--workflow-id', workflow, '--reason', 'owned crash lifecycle cleanup')
        (stage / 'process-backend-calls.json').write_text(json.dumps(calls, indent=2) + '\n')
        stable_sources()
        if any(hashlib.sha256(path.read_bytes()).hexdigest() != hashes[name] for name, path in binaries.items()):
            raise RuntimeError('Production binary changed during process acceptance')
        return {'domain': domain, 'workflowId': workflow, 'runs': [initial_run, crash_run], 'lifecycle': lifecycle, 'startupDidNotInstall': True, 'startupDidNotCallBackend': True, 'sigtermClean': True, 'sigintClean': True, 'sigkillObserved': True, 'heartbeatTimeoutObserved': True, 'crashRecoveredSameTask': task, 'sameDeliverySchedule': True, 'completedAttempt': 2, 'backendCalls': len(calls), 'providerRequests': 0, 'scope': 'actual POSIX worker/installer binaries with controlled private backend; no live provider/managed SQL or outbound-network fault injection'}
    finally:
        release.set()
        for worker in workers:
            stop(worker)
        backend.shutdown()
        backend.server_close()
        thread.join(timeout=5)
        for stream in logs:
            stream.close()
