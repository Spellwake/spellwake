"""Real deletion worker/Temporal delivery against an explicit synthetic RPC server.

This is protocol qualification, not a Supabase/SQL or real-account deletion test.
"""

from contextlib import contextmanager
import base64
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import threading
import time
from typing import Callable, Iterator
import uuid


@contextmanager
def backend_fixture() -> Iterator[tuple[str, dict[str, object]]]:
    """Only synthetic UUIDs and fake credentials enter this loopback fixture."""
    actor = str(uuid.uuid4())
    target = str(uuid.uuid4())
    state: dict[str, object] = {'canceled': False, 'calls': 0, 'failure': None}

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, _format: str, *args: object) -> None:
            pass

        def do_POST(self) -> None:
            try:
                if self.headers.get('apikey') != 'sb_secret_test_only' or self.headers.get('Content-Profile') != 'app':
                    raise AssertionError('Invalid private RPC headers')
                length = int(self.headers.get('Content-Length', '0'))
                if not 0 < length < 4096:
                    raise AssertionError('Invalid fixture request bound')
                body = json.loads(self.rfile.read(length))
                if self.path == '/rest/v1/rpc/read_deletion_account_page':
                    if body != {'p_after': None, 'p_limit': 1} and body != {'p_after': actor, 'p_limit': 1}:
                        raise AssertionError('Unexpected account cursor')
                    result = {'items': [actor], 'nextCursor': actor} if body['p_after'] is None else {'items': [], 'nextCursor': None}
                elif self.path == '/rest/v1/rpc/read_account_retirement_work':
                    if body != {'p_account': actor}:
                        raise AssertionError('Unexpected retirement target')
                    result = {'state': 'pending', 'accountId': actor}
                elif self.path == '/rest/v1/rpc/retire_account':
                    if body != {'p_account': actor}:
                        raise AssertionError('Unexpected retirement attempt')
                    result = {'status': 'success', 'data': {'state': 'waiting', 'accountId': actor}}
                elif self.path == '/rest/v1/rpc/read_deletion_work':
                    if body != {'p_actor': actor, 'p_limit': 25}:
                        raise AssertionError('Unexpected cleanup discovery')
                    result = [] if state['canceled'] else [{'kind': 'trade', 'id': target}]
                elif self.path == '/rest/v1/rpc/cancel_deletion_work':
                    if body != {'p_actor': actor, 'p_kind': 'trade', 'p_target': target}:
                        raise AssertionError('Unexpected cleanup target')
                    state['calls'] = int(state['calls']) + 1
                    state['canceled'] = True
                    result = {'status': 'success', 'data': {'resourceId': target}}
                else:
                    raise AssertionError('Unexpected fixture RPC')
                encoded = json.dumps(result).encode()
                self.send_response(200)
                self.send_header('Content-Type', 'application/json')
                self.send_header('Content-Length', str(len(encoded)))
                self.end_headers()
                self.wfile.write(encoded)
            except Exception as error:
                state['failure'] = type(error).__name__
                self.send_error(500, 'fixture protocol failed')

    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f'http://127.0.0.1:{server.server_port}/', state
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


def qualify_worker(root: Path, stage: Path, namespace: str, cli: Callable[..., dict], stop: Callable,
                   environment: dict[str, str], verify: Callable[[], bool]) -> None:
    """Drive the actual worker against caller-qualified backend fixtures."""
    for name in ['deletion_worker', 'deletion_workflow']:
        if not (root / 'target/debug' / name).is_file():
            raise RuntimeError('Build current deletion binaries before qualification')
    environment = environment.copy()
    environment.update(SPELLWAKE_TEMPORAL_ADDRESS='http://127.0.0.1:57233', SPELLWAKE_TEMPORAL_NAMESPACE=namespace)
    environment.pop('SPELLWAKE_TEMPORAL_API_KEY', None)
    log_path = stage / 'deletion-worker.log'
    log_path.touch(mode=0o600)
    worker = None
    identifier = 'spellwake-deletion-cleanup-v1'
    try:
        with log_path.open('ab') as log:
            worker = subprocess.Popen([str(root / 'target/debug/deletion_worker')], env=environment,
                                      stdout=log, stderr=subprocess.STDOUT)
            for _ in range(2):
                installed = subprocess.run([str(root / 'target/debug/deletion_workflow'), 'install'], env=environment,
                                           capture_output=True, timeout=30, check=True)
                if json.loads(installed.stdout) != {'status': 'success', 'data': {'workflowId': identifier}}:
                    raise RuntimeError('Unexpected deletion installation acknowledgment')
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                if worker.poll() is not None:
                    raise RuntimeError('Owned deletion worker exited early')
                history = cli('workflow', 'show', '--workflow-id', identifier)
                events = history.get('events', [])
                if verify() and any('timerStartedEventAttributes' in event for event in events):
                    break
                time.sleep(0.2)
            else:
                raise RuntimeError('Deletion worker did not reach the next-sweep timer')
            names = [event['activityTaskScheduledEventAttributes']['activityType']['name'] for event in events if 'activityTaskScheduledEventAttributes' in event]
            if names != ['spellwake-deletion-deliver-v1', 'spellwake-deletion-deliver-v1'] or not verify():
                raise RuntimeError('Unexpected real deletion delivery sequence')
            encoded = json.dumps(history).encode()
            secret = environment['SPELLWAKE_SUPABASE_SECRET_KEY'].encode()
            if secret in encoded or base64.b64encode(secret) in encoded:
                raise RuntimeError('Backend credential entered workflow history')
            (stage / 'deletion-history.json').write_bytes(encoded)
            cli('workflow', 'cancel', '--workflow-id', identifier)
            deadline = time.monotonic() + 10
            while time.monotonic() < deadline:
                description = cli('workflow', 'describe', '--workflow-id', identifier)
                if description.get('workflowExecutionInfo', {}).get('status') == 'WORKFLOW_EXECUTION_STATUS_CANCELED':
                    break
                time.sleep(0.1)
            else:
                raise RuntimeError('Deletion timer cancellation did not finish')
    finally:
        stop(worker)

def qualify_delivery(root: Path, stage: Path, namespace: str, cli: Callable[..., dict], stop: Callable) -> None:
    """Protocol-only fixture, distinct from the real Supabase qualification."""
    with backend_fixture() as (url, state):
        environment = os.environ.copy()
        environment.update(SPELLWAKE_SUPABASE_URL=url, SPELLWAKE_SUPABASE_SECRET_KEY='sb_secret_test_only')
        qualify_worker(root,stage,namespace,cli,stop,environment,
                       lambda: state['canceled'] is True and state['calls']==1 and state['failure'] is None)
    print('Real deletion worker passed: synthetic RPC discovery/cancel, repeated install, own activity queue, timer, key-free history and cancellation. No SQL/finalization claim.')
