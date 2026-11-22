"""Run production Patreon current workflows against an owned local Temporal namespace.

Controlled HTTP tests supply backend observations, not managed SQL or live Patreon proof.
Only the actual child server created by this invocation is stopped; no remote activation.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import time
import uuid
from test_local_price_workflow import stop

ROOT = Path(__file__).resolve().parents[1]


def main() -> None:
    """Retain the unique store, source test output and actual fetched run histories."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--case', choices=['all', 'continuation', 'restart', 'fairness', 'cancellation', 'installer', 'process'], default='all')
    parser.add_argument('--domain', choices=['current'], default='current')
    args = parser.parse_args()
    selected = args.case
    filters = {'all': ('patreon::refresh_workflow::tests::', 5, ['continuation', 'fairness', 'restart', 'heartbeat', 'cancellation', 'installer']), 'cancellation': ('patreon::refresh_workflow::tests::cancellation::production_temporal_cancellation_observes_context_and_blocks_later_provider_work', 1, ['heartbeat', 'cancellation']), 'installer': ('patreon::refresh_workflow::tests::installer::production_installer_recovers_hidden_ack_reuses_running_and_rejects_wrong_type_or_terminal_state', 1, ['installer'])}
    filters['continuation'] = ('patreon::refresh_workflow::tests::runtime::production_temporal_continues_recorded_tasks_and_cancels_idle_timer_without_provider_io', 1, ['continuation'])
    filters['restart'] = ('patreon::refresh_workflow::tests::runtime::production_temporal_recovers_recorded_task_after_worker_and_server_restart', 1, ['restart'])
    filters['fairness'] = ('patreon::refresh_workflow::tests::runtime::production_temporal_exhaustion_preserves_discovery_cursor_and_revisits_failed_delivery', 1, ['fairness'])
    test_filter, expected_tests, receipt_names = filters[selected] if selected != 'process' else ('', 0, [])
    temporal = shutil.which('temporal')
    if temporal is None:
        raise RuntimeError('Temporal CLI required')
    temporal = str(Path(temporal).resolve())
    initial_cli_hash = hashlib.sha256(Path(temporal).read_bytes()).hexdigest()
    initial_cli_version = subprocess.check_output([temporal, '--version'], text=True).strip()
    with socket.socket() as probe:
        probe.bind(('127.0.0.1', 0))
        port = probe.getsockname()[1]
    stage = ROOT / 'target' / ('patreon-temporal-' + uuid.uuid4().hex)
    stage.mkdir(mode=0o700)
    empty = stage / 'empty.yaml'
    empty.write_text('{}')
    namespace = 'spellwake-patreon-test-' + uuid.uuid4().hex
    address = f'127.0.0.1:{port}'
    isolated = ['--disable-config-env', '--disable-config-file', '--env-file', str(empty)]
    environment = {k: v for k, v in os.environ.items() if not k.startswith('TEMPORAL_')}
    server = None
    print(f'Owned local Temporal artifacts: {stage}', flush=True)
    with (stage / 'server.log').open('wb') as server_log:
        try:
            def cli(*args: str) -> bytes:
                return subprocess.check_output([temporal, *args, *isolated, '--address', address, '--namespace', namespace, '--output', 'json', '--command-timeout', '10s'], env=environment, stderr=subprocess.PIPE, timeout=15)

            def start_server() -> subprocess.Popen[bytes]:
                nonlocal server
                server = subprocess.Popen([temporal, 'server', 'start-dev', *isolated, '--ip', '127.0.0.1', '--port', str(port), '--namespace', namespace, '--headless', '--db-filename', str(stage / 'temporal.db')], env=environment, stdout=server_log, stderr=subprocess.STDOUT)
                deadline = time.monotonic() + 45
                while True:
                    if server.poll() is not None:
                        raise RuntimeError('Owned server exited during startup')
                    try:
                        cli('operator', 'cluster', 'health')
                        return server
                    except subprocess.SubprocessError:
                        if time.monotonic() >= deadline:
                            raise RuntimeError('Owned Temporal readiness timed out')
                        time.sleep(0.2)

            server = start_server()
            test_environment = environment.copy()
            test_environment.update(SPELLWAKE_TEST_PATREON_TEMPORAL_NAMESPACE=namespace, SPELLWAKE_TEST_PATREON_TEMPORAL_ADDRESS='http://' + address, SPELLWAKE_TEST_PATREON_ARTIFACT_DIR=str(stage))
            project = ROOT.parents[1]
            source_files = list((ROOT / 'src').rglob('*.rs')) + list((ROOT / 'tests' / 'fixtures').rglob('*'))
            source_files += [ROOT / 'Cargo.toml', ROOT / 'Cargo.lock', Path(__file__).resolve(), Path(__file__).with_name('test_local_price_workflow.py'), Path(__file__).with_name('patreon_process_acceptance.py'), project / 'rust-toolchain.toml']
            source_hashes = {str(path.relative_to(project)): hashlib.sha256(path.read_bytes()).hexdigest() for path in source_files if path.is_file()}
            cli_binary = Path(temporal).resolve()
            cli_hash = initial_cli_hash
            if hashlib.sha256(cli_binary.read_bytes()).hexdigest() != cli_hash:
                raise RuntimeError('CLI changed after initial server startup')
            source_pins = {'sourceHashes': source_hashes, 'temporalCli': {'path': str(cli_binary), 'sha256': cli_hash, 'version': initial_cli_version}}
            (stage / 'source-pins.json').write_text(json.dumps(source_pins, indent=2) + '\n')
            if selected == 'process':
                from patreon_process_acceptance import qualify
                def stable_sources() -> None:
                    if any(hashlib.sha256((project / path).read_bytes()).hexdigest() != digest for path, digest in source_hashes.items()) or hashlib.sha256(cli_binary.read_bytes()).hexdigest() != cli_hash:
                        raise RuntimeError('Process acceptance source or CLI changed')
                receipt = qualify(ROOT, stage, test_environment, cli, stable_sources, args.domain)
                (stage / 'process-receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
                (stage / 'source-check.json').write_text(json.dumps({'unchanged': True, 'sourceCount': len(source_hashes), 'cliUnchanged': True, 'productionBinariesUnchanged': True}, indent=2) + '\n')
                (stage / 'result.json').write_text(json.dumps({'passed': True, 'selectedCase': selected, 'domain': args.domain, 'namespace': namespace, 'processReceipt': receipt}, indent=2) + '\n')
                return
            build = ['cargo', 'test', '--locked', '--manifest-path', str(ROOT / 'Cargo.toml'), '--lib', '--no-run', '--message-format=json']
            with (stage / 'build.log').open('wb') as build_log:
                subprocess.run(build, cwd=ROOT, env=test_environment, stdout=build_log, stderr=subprocess.STDOUT, check=True, timeout=180)
            if any(hashlib.sha256((project / path).read_bytes()).hexdigest() != digest for path, digest in source_hashes.items()) or hashlib.sha256(cli_binary.read_bytes()).hexdigest() != cli_hash:
                raise RuntimeError('Acceptance source or CLI changed during build')
            executables = []
            for line in (stage / 'build.log').read_text().splitlines():
                if not line.startswith('{'):
                    continue
                artifact = json.loads(line)
                if artifact.get('reason') == 'compiler-artifact' and artifact.get('target', {}).get('name') == 'spellwake_application' and artifact.get('profile', {}).get('test') and artifact.get('executable'):
                    executables.append(artifact['executable'])
            if len(executables) != 1:
                raise RuntimeError('Expected exactly one native application test executable')
            executable = Path(executables[0]).resolve()
            binary_hash = hashlib.sha256(executable.read_bytes()).hexdigest()
            exact = ['--exact'] if selected != 'all' else []
            listed = subprocess.check_output([str(executable), test_filter, *exact, '--ignored', '--list'], cwd=ROOT, env=test_environment, text=True, timeout=15)
            (stage / 'selection.log').write_text(listed)
            if sum(line.endswith(': test') for line in listed.splitlines()) != expected_tests:
                raise RuntimeError('Actual native test selection does not match the declared case')
            command = [str(executable), test_filter, *exact, '--ignored', '--test-threads=1', '--nocapture']
            (stage / 'execution.json').write_text(json.dumps({'buildCommand': build, 'command': command, 'binary': str(executable), 'sha256': binary_hash}, indent=2) + '\n')
            with (stage / 'cargo.log').open('wb') as cargo_log:
                tests = subprocess.Popen(command, cwd=ROOT, env=test_environment, stdout=cargo_log, stderr=subprocess.STDOUT)
                deadline = time.monotonic() + 600
                try:
                    while tests.poll() is None:
                        request_path = stage / 'restart-request.json'
                        if request_path.is_file() and not (stage / 'restart-done.json').exists():
                            request = json.loads(request_path.read_text())
                            if set(request) != {'case', 'nonce', 'workflowId', 'runId'} or request['case'] != 'recorded-task' or not request['workflowId'].startswith('patreon-' + args.domain + '-test-'):
                                raise RuntimeError('Unexpected local restart request')
                            if str(uuid.UUID(request['nonce'])) != request['nonce'] or str(uuid.UUID(request['runId'])) != request['runId']:
                                raise RuntimeError('Noncanonical local restart request')
                            (stage / 'restart-before-history.json').write_bytes(cli('workflow', 'show', '--workflow-id', request['workflowId'], '--run-id', request['runId']))
                            (stage / 'restart-before-description.json').write_bytes(cli('workflow', 'describe', '--workflow-id', request['workflowId'], '--run-id', request['runId']))
                            if server.poll() is not None:
                                raise RuntimeError('Owned server stopped before requested restart')
                            old_pid = server.pid
                            stop(server)
                            old_code = server.returncode
                            if old_code is None:
                                raise RuntimeError('Owned server did not stop')
                            server = start_server()
                            reply = {'nonce': request['nonce'], 'oldPid': old_pid, 'oldReturnCode': old_code, 'newPid': server.pid, 'healthReadbackSucceeded': True, 'store': str(stage / 'temporal.db')}
                            temporary = stage / 'restart-done.tmp'
                            temporary.write_text(json.dumps(reply))
                            temporary.replace(stage / 'restart-done.json')
                        elif server.poll() is not None:
                            raise RuntimeError('Owned server exited during tests')
                        if time.monotonic() >= deadline:
                            raise RuntimeError('Local Temporal test deadline exceeded')
                        time.sleep(0.05)
                finally:
                    stop(tests)
                test_code = tests.returncode
            print((stage / 'cargo.log').read_text(), flush=True)
            if test_code:
                raise RuntimeError('Production local Temporal tests failed; retained cargo log')
            if hashlib.sha256(executable.read_bytes()).hexdigest() != binary_hash:
                raise RuntimeError('Native test executable changed during acceptance')
            if any(hashlib.sha256((project / path).read_bytes()).hexdigest() != digest for path, digest in source_hashes.items()) or hashlib.sha256(cli_binary.read_bytes()).hexdigest() != cli_hash:
                raise RuntimeError('Acceptance source or CLI changed during execution')
            (stage / 'source-check.json').write_text(json.dumps({'unchanged': True, 'sourceCount': len(source_hashes), 'cliUnchanged': True, 'binaryUnchanged': True}, indent=2) + '\n')
            receipts = {}
            for name in receipt_names:
                receipt = json.loads((stage / f'{name}-receipt.json').read_text())
                valid_id = receipt['workflowId'] == 'spellwake-patreon-current-refresh-v1' if name == 'installer' else receipt['workflowId'].startswith('patreon-' + args.domain + '-test-')
                if not valid_id:
                    raise RuntimeError('Unexpected owned workflow identity')
                for index, run in enumerate(receipt['runs']):
                    raw = cli('workflow', 'show', '--workflow-id', receipt['workflowId'], '--run-id', run)
                    json.loads(raw)
                    (stage / f'{name}-history-{index}.json').write_bytes(raw)
                receipts[name] = receipt
            (stage / 'result.json').write_text(json.dumps({'passed': True, 'selectedCase': selected, 'domain': args.domain, 'expectedTests': expected_tests, 'namespace': namespace, 'receipts': receipts}, indent=2) + '\n')
        finally:
            stop(server)


if __name__ == '__main__':
    main()
