"""Compose actual managed identity/SQL with an owned local Temporal queue.

The retained price manifest is a synthetic normalized fixture. No worker runs,
so queued preparation does not qualify extraction or Scryfall source truth.
"""
import base64
from concurrent.futures import ThreadPoolExecutor
import json
import shutil
import socket
import subprocess
import time
import uuid
from typing import Callable

from disposable_supabase import DisposableSupabase, private_write
from human_api_acceptance import Request, Expect
from qualification_http import object_json


def qualify_temporal_preparation(
    realm: DisposableSupabase, request: Request, expect: Expect,
    configure: Callable[[dict[str, str]], None], owner_token: str, peer_token: str,
    owner: str, peer: str, source_path: str, printing: str, slot: str,
) -> dict:
    """Verify shared dispatch with real managed callers and exact durable input."""
    temporal = shutil.which('temporal')
    if temporal is None:
        raise RuntimeError('Local Temporal CLI required for composed API qualification')
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        port = listener.getsockname()[1]
    namespace = 'spellwake-api-test-' + uuid.uuid4().hex
    config = realm.stage / 'temporal-empty.yaml'
    private_write(config, b'{}')
    isolated = ['--disable-config-env', '--disable-config-file', '--env-file', str(config)]
    address = '127.0.0.1:' + str(port)
    process = realm.start_background([
        temporal, 'server', 'start-dev', *isolated, '--ip', '127.0.0.1',
        '--port', str(port), '--namespace', namespace, '--headless',
        '--db-filename', str(realm.stage / 'temporal.db'),
    ], {})

    def cli(*arguments: str) -> dict | list:
        output = realm.command([
            temporal, *arguments, *isolated, '--address', address,
            '--namespace', namespace, '--output', 'json', '--command-timeout', '10s',
        ], timeout=15)
        return json.loads(output) if output.strip() else {}

    deadline = time.monotonic() + 45
    while True:
        if process.poll() is not None:
            raise RuntimeError('Owned Temporal exited before readiness')
        try:
            cli('operator', 'cluster', 'health')
            break
        except (RuntimeError, json.JSONDecodeError, subprocess.SubprocessError):
            if time.monotonic() >= deadline:
                raise RuntimeError('Owned Temporal readiness deadline exceeded') from None
            time.sleep(0.2)
    configure({'SPELLWAKE_TEMPORAL_ADDRESS': 'http://' + address,
               'SPELLWAKE_TEMPORAL_NAMESPACE': namespace})
    # Different managed principals request the same immutable source concurrently.
    # This exercises the real queue's UseExisting/memo reconciliation, not a mock.
    with ThreadPoolExecutor(max_workers=2) as pool:
        replies = list(pool.map(lambda token: request(source_path, token, {}),
                                [owner_token, peer_token]))
    expected_input = {'slot': slot, 'externalPrintingId': printing}
    for label, account, reply in zip(['owner', 'peer'], [owner, peer], replies):
        payload = expect('prediction-prepare-temporal-' + label, reply, 202)
        if payload != {'status': 'success', 'data': {
            'accountId': account, 'printingId': printing, 'slot': slot, 'state': 'requested',
        }}:
            raise RuntimeError('Composed preparation lost original caller/source identity')
    repeated = expect('prediction-prepare-temporal-repeat', request(source_path, owner_token, {}), 202)
    if repeated != object_json(replies[0][1]):
        raise RuntimeError('Repeated preparation changed its queued caller/source result')
    identifier = 'spellwake-price-probe-v1:' + slot + ':' + printing
    history = object_json(cli('workflow', 'show', '--workflow-id', identifier))
    events = history.get('events')
    if not isinstance(events, list) or not events:
        raise RuntimeError('Actual queued Temporal history missing')
    started = object_json(object_json(events[0])['workflowExecutionStartedEventAttributes'])
    if object_json(started['workflowType'])['name'] != 'spellwake-price-probe-v1':
        raise RuntimeError('Composed preparation dispatched a different workflow')
    inputs = object_json(started['input'])['payloads']
    if not isinstance(inputs, list) or len(inputs) != 1:
        raise RuntimeError('Queued preparation input cardinality differs')
    memo = object_json(object_json(started['memo'])['fields'])
    for payload in [object_json(inputs[0]), object_json(memo['source'])]:
        if json.loads(base64.b64decode(payload['data'], validate=True)) != expected_input:
            raise RuntimeError('Queued workflow input/memo differs from exact source')
    listed = cli('workflow', 'list', '--query', 'WorkflowId = "' + identifier + '"')
    if not isinstance(listed, list) or len(listed) != 1:
        raise RuntimeError('Concurrent preparation did not share one workflow execution')
    encoded = json.dumps(history).encode()
    for credential in [owner_token, peer_token, realm.status['SECRET_KEY']]:
        if credential.encode() in encoded or base64.b64encode(credential.encode()) in encoded:
            raise RuntimeError('Private credential entered preparation history')
    if any('activityTaskScheduledEventAttributes' in object_json(event) for event in events):
        raise RuntimeError('Queue-only fixture unexpectedly executed extraction')
    return {'passed': True, 'namespace': namespace, 'sharedWorkflowExecutions': 1,
            'actualManagedCallers': 2, 'exactInputAndMemo': True,
            'scope': 'managed Auth/PostgREST/API -> real Temporal queued acknowledgment; no worker/extraction/source truth'}
