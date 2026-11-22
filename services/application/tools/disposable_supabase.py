"""Own one new CLI project, bridge and control; never operate on retained projects."""
import hashlib
import json
import os
from pathlib import Path
import signal
import shutil
import sys
import socket
import subprocess
import uuid
from typing import Literal

ROOT = Path(__file__).resolve().parents[1]
CLI = ROOT / 'target/tools/supabase-v2.122.0/supabase'
LABEL = 'com.supabase.cli.project'


class QualificationCommandError(RuntimeError):
    """Safe command classification; arguments and captured output stay private."""

    def __init__(self, reason: Literal['timeout', 'interrupted', 'failed'], sequence: int) -> None:
        self.reason = reason
        self.sequence = sequence
        super().__init__(f'Owned qualification subprocess {reason}; command {sequence} diagnostics retained privately')


def private_write(path: Path, data: bytes) -> None:
    """Create an owner-only artifact without overwriting an existing file."""
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, 'wb') as output:
        output.write(data)


class DisposableSupabase:
    """A generated name, not a caller-supplied project ID, determines ownership."""

    def __init__(self, *, storage: bool = False, mail: bool = False,
                 jwt_expiry_seconds: int | None = None) -> None:
        if jwt_expiry_seconds is not None and (type(jwt_expiry_seconds) is not int or not 1 <= jwt_expiry_seconds <= 604800):
            raise ValueError('Owned JWT lifetime must be positive seconds within the configured maximum')
        self.jwt_expiry_seconds = jwt_expiry_seconds
        self.storage = storage
        self.mail = mail
        suffix = uuid.uuid4().hex
        self.project = 'swactor-' + suffix
        if len(self.project) != 40:
            raise RuntimeError('Generated project ID must be sanitization-stable')
        self.network = 'spellwake-actor-net-' + suffix
        self.control = 'spellwake-actor-control-' + suffix
        self.stage = ROOT / 'target' / ('managed-actor-' + suffix)
        self.stage.mkdir(mode=0o700)
        self.environment = {key: os.environ[key] for key in (
            'PATH', 'HOME', 'TMPDIR', 'LANG', 'LC_ALL', 'DOCKER_HOST', 'DOCKER_CONTEXT', 'DOCKER_CONFIG',
        ) if key in os.environ}
        self.real_docker = str(Path(shutil.which('docker') or '').resolve())
        if not Path(self.real_docker).is_file():
            raise RuntimeError('Docker executable is unavailable')
        self.shim = self.stage / 'bin/docker'
        self.events = self.stage / 'publication-events.jsonl'
        self.network_attempted = False
        self.control_attempted = False
        self.start_attempted = False
        self.cleanup_verified = False
        self.network_id = ''
        self.runtime: list[dict] = []
        self.observed_volumes: set[str] = set()
        self.manifest: list[dict] = []
        self.status: dict = {}
        self.sequence = 0
        self.background: list[subprocess.Popen] = []
        with socket.socket() as first, socket.socket() as second, socket.socket() as third, socket.socket() as mail_listener:
            for listener in (first, second, third, mail_listener):
                listener.bind(('127.0.0.1', 0))
            self.ports = [listener.getsockname()[1] for listener in (first, second, third)]
            self.mail_port = mail_listener.getsockname()[1]

    def command(self, arguments: list[str], data: bytes | None = None, timeout: int = 60,
                extra_environment: dict[str, str] | None = None) -> bytes:
        """Terminate the owned process group and reap its direct child before cleanup."""
        self.sequence += 1
        sequence = self.sequence
        if arguments and arguments[0] == 'docker':
            arguments = [self.real_docker, *arguments[1:]]
        environment = self.environment | (extra_environment or {})
        process = subprocess.Popen(arguments, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   env=environment, cwd=self.stage, start_new_session=True)
        try:
            output, error = process.communicate(data, timeout=timeout)
        except BaseException as error:
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                output, captured_error = process.communicate(timeout=5)
            except subprocess.TimeoutExpired:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                output, captured_error = process.communicate()
            else:
                # A redirected descendant may outlive the reaped leader without
                # keeping its pipes open. Terminate that owned group as well.
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            # communicate after termination includes the buffered pre-timeout
            # bytes. Do not concatenate TimeoutExpired.output again: it overlaps.
            private_write(self.stage / f'private-command-{sequence}.log', output + captured_error)
            reason = 'timeout' if isinstance(error, subprocess.TimeoutExpired) else 'interrupted'
            raise QualificationCommandError(reason, sequence) from None
        private_write(self.stage / f'private-command-{sequence}.log', output + error)
        if process.returncode:
            raise QualificationCommandError('failed', sequence)
        return output

    def start_background(self, arguments: list[str], extra_environment: dict[str, str]) -> subprocess.Popen:
        """Own a direct child in a new group with private output and no input pipe."""
        self.sequence += 1
        log = self.stage / f'private-background-{self.sequence}.log'
        private_write(log, b'')
        with log.open('ab') as output:
            process = subprocess.Popen(arguments, stdin=subprocess.DEVNULL, stdout=output, stderr=output,
                env=self.environment | extra_environment, cwd=self.stage, start_new_session=True)
        self.background.append(process)
        return process

    def stop_background(self) -> None:
        """Reap exact child handles before touching their database resources."""
        for process in self.background:
            if process.poll() is None:
                try:
                    os.killpg(process.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    try:
                        os.killpg(process.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    process.wait(timeout=5)
            if process.poll() is None:
                raise RuntimeError('Owned background process did not exit')
        self.background.clear()

    def cli(self, arguments: list[str], timeout: int = 60) -> bytes:
        return self.command([str(CLI), *arguments, '--workdir', str(self.stage), '--agent', 'no'], timeout=timeout)

    def resources(self, kind: str) -> list[str]:
        """Cross-check label discovery against exact generated resource-name namespace."""
        if kind == 'container':
            base = ['ps', '-a']; format_value = '{{.Names}}'
        elif kind == 'volume':
            base = ['volume', 'ls']; format_value = '{{.Name}}'
        else:
            raise ValueError('Unsupported owned resource kind')
        labeled = self.command(['docker', *base, '--filter', f'label={LABEL}={self.project}', '--format', format_value]).decode().split()
        named = self.command(['docker', *base, '--filter', 'name=^supabase_[a-z_]+_' + self.project + '$', '--format', format_value]).decode().split()
        return sorted(set(labeled) | set(named))

    def inspect(self, name: str, expression: str, kind: str = 'container') -> object:
        args = ['docker', 'inspect'] if kind == 'container' else ['docker', kind, 'inspect']
        return json.loads(self.command([*args, '--format', expression, name]))

    def sql(self, query: str) -> str:
        """SQL output stays in the owned0600 diagnostics; never surface vendor errors."""
        return self.command(['docker', 'exec', '-i', 'supabase_db_' + self.project, 'psql', '-U', 'postgres',
                             '-d', 'postgres', '-v', 'ON_ERROR_STOP=1', '-qAt'], query.encode()).decode().strip()

    def __enter__(self) -> 'DisposableSupabase':
        try:
            # Exact UUID namespace absence is checked before any ownership claim.
            if self.resources('container') or self.resources('volume'):
                raise RuntimeError('Generated project namespace is not empty')
            names = self.command(['docker', 'ps', '-a', '--filter', 'name=' + self.project, '--format', '{{.Names}}']).decode().split()
            if names or self.command(['docker', 'network', 'ls', '--filter', 'name=^' + self.network + '$', '--format', '{{.ID}}']).strip():
                raise RuntimeError('Generated resource namespace is not empty')
            if self.command(['docker', 'ps', '-a', '--filter', 'name=^/' + self.control + '$', '--format', '{{.Names}}']).strip():
                raise RuntimeError('Generated control namespace is not empty')
            self.network_attempted = True
            self.network_id = self.command(['docker', 'network', 'create', '--label', f'{LABEL}={self.project}',
                '--opt', 'com.docker.network.bridge.host_binding_ipv4=127.0.0.1', self.network]).decode().strip()
            metadata = self.inspect(self.network, '{{json .}}', 'network')
            if metadata['Id'] != self.network_id or metadata['Options'].get('com.docker.network.bridge.host_binding_ipv4') != '127.0.0.1':
                raise RuntimeError('Owned loopback bridge configuration differs')
            self.prepare_shim()
            self.control_attempted = True
            self.command([str(self.shim), 'run', '-d', '--name', self.control, '--network', self.network,
                          '--label', f'{LABEL}={self.project}', '--publish', '5432', 'postgres:17', 'sleep', '300'])
            ports = self.inspect(self.control, '{{json .NetworkSettings.Ports}}')
            if ports.get('5432/tcp') is None or any(p['HostIp'] != '127.0.0.1' for p in ports['5432/tcp']):
                raise RuntimeError('Docker default publishing does not enforce loopback')
            self.command(['docker', 'rm', '-f', '-v', self.control])
            self.control_attempted = False
            self.prepare()
            if self.cli(['--version']).decode().strip() != '2.122.0':
                raise RuntimeError('Cached CLI version differs')
            self.start_attempted = True
            excluded = 'realtime,imgproxy,postgres-meta,studio,edge-runtime,logflare,vector,supavisor'
            if not self.mail:
                excluded += ',mailpit'
            if not self.storage:
                excluded += ',storage-api'
            self.cli(['start', '--network-id', self.network, '--exclude', excluded], timeout=300)
            containers = self.resources('container')
            if not containers:
                raise RuntimeError('Owned CLI project has no labeled containers')
            for name in containers:
                metadata = self.inspect(name, '{"labels":{{json .Config.Labels}},"image":{{json .Config.Image}},"imageId":{{json .Image}},"ports":{{json .NetworkSettings.Ports}},"mounts":{{json .Mounts}}}')
                # Retain only non-secret source/bind evidence; never serialize Config.Env.
                if not name.endswith('_' + self.project) or metadata['labels'].get(LABEL) != self.project:
                    raise RuntimeError('CLI project resource ownership differs')
                bindings = metadata['ports'] or {}
                if any(p['HostIp'] != '127.0.0.1' for values in bindings.values() if values for p in values):
                    raise RuntimeError('CLI fixture publishes a non-loopback listener')
                volumes = [mount['Name'] for mount in metadata.get('mounts', []) if mount['Type'] == 'volume']
                for volume in volumes:
                    if not volume.endswith('_' + self.project):
                        raise RuntimeError('Owned stack has an unexpected volume namespace')
                    labels = self.inspect(volume, '{{json .Labels}}', 'volume') or {}
                    if labels.get(LABEL) != self.project:
                        raise RuntimeError('Owned volume label differs')
                    self.observed_volumes.add(volume)
                self.runtime.append({'name': name, 'volumes': volumes, 'image': metadata['image'], 'imageId': metadata['imageId'], 'ports': bindings})
            if 'supabase_db_' + self.project not in self.observed_volumes or not self.resources('volume'):
                raise RuntimeError('Owned database volume presence was not established')
            events = [json.loads(row) for row in self.events.read_text().splitlines()]
            cli_names = {event['name'] for event in events if event['phase'] == 'cli'}
            published_names = {row['name'] for row in self.runtime if any(row['ports'].values())}
            if cli_names != published_names or not published_names or sum(event['phase'] == 'control' for event in events) != 1:
                raise RuntimeError('Actual CLI publication did not traverse the binding adapter')
            runtime_by_name = {row['name']: row for row in self.runtime}
            for event in events:
                if event['phase'] != 'cli':
                    continue
                actual = runtime_by_name[event['name']]['ports']
                if sum(len(values or []) for values in actual.values()) != len(event['ports']):
                    raise RuntimeError('CLI publication event count differs from actual bindings')
                for mapping in event['ports']:
                    host, target = mapping.removeprefix('127.0.0.1:').split(':')
                    target = target if '/' in target else target + '/tcp'
                    if actual.get(target) != [{'HostIp': '127.0.0.1', 'HostPort': host}]:
                        raise RuntimeError('CLI publication mapping differs from actual binding')
            self.status = json.loads(self.cli(['status', '-o', 'json']))
            if self.status.get('API_URL') != f'http://127.0.0.1:{self.ports[0]}' or not self.status.get('PUBLISHABLE_KEY', '').startswith('sb_publishable_'):
                raise RuntimeError('Owned gateway address or key format differs')
            return self
        except BaseException as primary:
            try:
                self.__exit__()
            except BaseException as cleanup:
                raise BaseExceptionGroup("Owned startup and cleanup both failed", [primary, cleanup]) from None
            raise

    def prepare_shim(self) -> None:
        """Adapt only this CLI child's Docker calls; all driver cleanup uses real Docker."""
        self.shim.parent.mkdir(mode=0o700)
        code = (f'#!{sys.executable}\nimport sys\nfrom pathlib import Path\n'
                f'sys.path.insert(0, {str(ROOT / "tools")!r})\nfrom loopback_docker import main\n'
                f'main({self.real_docker!r}, {self.project!r}, {set([self.network,self.network_id])!r}, {self.control!r}, Path({str(self.events)!r}))\n')
        private_write(self.shim, code.encode())
        self.shim.chmod(0o700)
        podman = self.shim.parent / 'podman'
        private_write(podman, b'#!/bin/sh\nexit 125\n')
        podman.chmod(0o700)
        self.environment['PATH'] = str(self.shim.parent) + os.pathsep + self.environment.get('PATH', '')

    def prepare(self) -> None:
        """Stage the exact approved migrations; no dotenv or ancestor project discovery."""
        directory = self.stage / 'supabase'
        directory.mkdir(mode=0o700)
        migrations = directory / 'migrations'
        migrations.mkdir(mode=0o700)
        for source in sorted((ROOT / 'supabase/migrations').glob('*.sql')):
            data = source.read_bytes()
            private_write(migrations / source.name, data)
            self.manifest.append({'file': source.name, 'sha256': hashlib.sha256(data).hexdigest()})
        api, database, shadow = self.ports
        config = f'''project_id = "{self.project}"
[api]
enabled = true
port = {api}
schemas = ["public", "app"]
extra_search_path = ["public", "extensions"]
auto_expose_new_tables = false
[db]
port = {database}
shadow_port = {shadow}
major_version = 17
health_timeout = "3m"
[db.migrations]
enabled = true
[db.seed]
enabled = false
[realtime]
enabled = false
[studio]
enabled = false
[local_smtp]
enabled = {str(self.mail).lower()}
port = {self.mail_port}
[storage]
enabled = {str(self.storage).lower()}
file_size_limit = "2GiB"
[edge_runtime]
enabled = false
[analytics]
enabled = false
[auth]
enabled = true
site_url = "http://127.0.0.1:5173"
enable_signup = {str(self.mail).lower()}
enable_anonymous_sign_ins = false
enable_manual_linking = false
[auth.email]
enable_signup = {str(self.mail).lower()}
enable_confirmations = {str(self.mail).lower()}
otp_length = 6
[auth.hook.custom_access_token]
enabled = true
uri = "pg-functions://postgres/app/custom_access_token_hook"
[experimental.pgdelta]
enabled = false
'''
        if self.mail:
            templates = directory / 'templates'
            templates.mkdir(mode=0o700)
            # Qualification-only template: test actual SMTP delivery, not a final email design.
            private_write(templates / 'otp.html', b'<!doctype html><html lang="en"><body><p>Your Spellwake test code: <strong>{{ .Token }}</strong></p></body></html>')
            config += '''
[auth.email.template.confirmation]
subject = "Spellwake local test code"
content_path = "./supabase/templates/otp.html"
[auth.email.template.magic_link]
subject = "Spellwake local test code"
content_path = "./supabase/templates/otp.html"
'''
        if self.jwt_expiry_seconds is not None:
            config = config.replace('[auth]\nenabled = true\n', '[auth]\nenabled = true\njwt_expiry = ' + str(self.jwt_expiry_seconds) + '\n', 1)
        private_write(directory / 'config.toml', config.encode())

    def __exit__(self, *_: object) -> None:
        if self.cleanup_verified:
            return
        # A live API must not outlive its database. If exact-child termination
        # cannot be verified, retain the owned resources and report cleanup failure.
        # The caller separately records primary and API termination failures.
        self.stop_background()
        failures: list[str] = []
        if self.control_attempted:
            try:
                self.command(['docker', 'rm', '-f', '-v', self.control])
                self.control_attempted = False
            except RuntimeError:
                failures.append('control')
        if self.start_attempted:
            try:
                self.cli(['stop', '--project-id', self.project, '--no-backup'], timeout=90)
                if self.resources('container') or self.resources('volume'):
                    failures.append('project')
            except RuntimeError:
                failures.append('project')
        if self.network_attempted:
            try:
                if not self.network_id:
                    present_name = self.command(['docker', 'network', 'ls', '--filter', 'name=^' + self.network + '$', '--format', '{{.ID}}']).strip()
                    if present_name:
                        recovered = self.inspect(self.network, '{{json .}}', 'network')
                        if recovered['Name'] != self.network or recovered.get('Labels', {}).get(LABEL) != self.project:
                            raise RuntimeError('Interrupted network creation has ambiguous ownership')
                        self.network_id = recovered['Id']
                if not self.network_id:
                    if failures:
                        raise RuntimeError('Owned fixture cleanup failed before network creation')
                    self.cleanup_verified = True
                    return
                # CLI may already have removed our labeled bridge, or left it intact.
                present = self.command(['docker', 'network', 'ls', '--filter', 'id=' + self.network_id, '--format', '{{.ID}}']).strip()
                if present:
                    metadata = self.inspect(self.network, '{{json .}}', 'network')
                    if metadata['Id'] != self.network_id or metadata.get('Containers'):
                        raise RuntimeError('Owned bridge has unexpected attachments')
                    self.command(['docker', 'network', 'rm', self.network_id])
                if self.command(['docker', 'network', 'ls', '--filter', 'id=' + self.network_id, '--format', '{{.ID}}']).strip():
                    failures.append('network')
            except RuntimeError:
                failures.append('network')
        if failures:
            raise RuntimeError('Owned fixture cleanup failed: ' + ','.join(failures))
        self.cleanup_verified = True
