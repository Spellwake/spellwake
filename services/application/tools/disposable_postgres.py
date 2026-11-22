"""Owned disposable databases for Auth qualification, never application data."""
import subprocess
import json
import time
import re
import uuid

CONTAINER = 'spellwake-application-db'


class DisposablePostgres:
    """Create UUID resources and verify their removal after stopping callers."""

    def __init__(self, purpose: str, role_labels: tuple[str, ...]) -> None:
        if not purpose.isidentifier() or not all(label.isidentifier() for label in role_labels):
            raise ValueError('Invalid disposable resource label')
        suffix = uuid.uuid4().hex
        self.database = f'spellwake_{purpose}_{suffix}'
        self.roles = {label: f'{label}_{suffix}' for label in role_labels}
        if len(self.database) > 63 or any(len(role) > 63 for role in self.roles.values()):
            raise ValueError('Disposable resource name too long')
        self.cleanup_verified = False
        self.container = CONTAINER
        self.port = 55439
        self.image_id: str | None = None
        self.server_version: str | None = None

    def sql(self, body: str, user: str = 'postgres', database: str | None = None, timeout: float = 60) -> str:
        """Use the actual LOGIN role; keep statement/error data out of output."""
        if user not in ('postgres', *self.roles.values()):
            raise ValueError('Unowned SQL login')
        result = subprocess.run(
            ['docker', 'exec', '-i', self.container, 'psql', '-U', user, '-d',
             database or self.database, '-X', '-qAt', '-v', 'ON_ERROR_STOP=1'],
            input=body, text=True, capture_output=True, check=False, timeout=timeout,
        )
        if result.returncode:
            raise RuntimeError('Disposable Auth qualification SQL failed')
        return result.stdout.strip()

    def __enter__(self) -> 'DisposablePostgres':
        image = subprocess.check_output(
            ['docker', 'inspect', '--format', '{{.Config.Image}}', self.container], text=True,
        ).strip()
        if image != 'postgres:17':
            raise RuntimeError('Unexpected owned qualification database image')
        self.image_id = subprocess.check_output(
            ['docker', 'inspect', '--format', '{{.Image}}', self.container], text=True,
        ).strip()
        try:
            for role in self.roles.values():
                self.sql(f'CREATE ROLE {role} LOGIN NOINHERIT;', database='postgres')
            self.sql(f'CREATE DATABASE {self.database};', database='postgres')
            self.server_version = self.sql('SHOW server_version')
            if not self.server_version.startswith('17.'):
                raise RuntimeError('Unexpected owned database server version')
        except BaseException:
            self.__exit__(None, None, None)
            raise
        return self

    def __exit__(self, *_: object) -> None:
        subprocess.run(
            ['docker', 'exec', self.container, 'dropdb', '--if-exists', '--force',
             '-U', 'postgres', self.database], check=True, capture_output=True,
        )
        for role in self.roles.values():
            self.sql(f'DROP ROLE IF EXISTS {role};', database='postgres')
        role_list = ','.join("'" + role + "'" for role in self.roles.values())
        absent = self.sql(
            f"SELECT (SELECT count(*) FROM pg_database WHERE datname='{self.database}')"
            f'+(SELECT count(*) FROM pg_roles WHERE rolname IN ({role_list}));',
            database='postgres',
        )
        if absent != '0':
            raise RuntimeError('Disposable Auth resources remain')
        self.cleanup_verified = True


class DisposableAuthPostgres(DisposablePostgres):
    """Own a fresh container so canonical Auth LOGIN cannot alter shared roles.

    Trust authentication is confined to a fresh loopback-bound test database.
    Docker removes this invocation's anonymous data volume with the container.
    """

    def __init__(self) -> None:
        super().__init__('auth_application', ('gotrue_probe',))
        self.container = 'spellwake-auth-db-' + uuid.uuid4().hex
        self.roles = {'gotrue_probe': 'supabase_auth_admin'}
        self.volume_cleanup_verified = False
        self.owned_volumes: set[str] = set()

    def remove_container(self) -> None:
        """Remove only this generated container and its anonymous volume."""
        try:
            inspect = subprocess.run(
                ['docker', 'inspect', '--format', '{{json .Mounts}}', self.container],
                capture_output=True, text=True, check=False,
            )
            volumes = [item['Name'] for item in json.loads(inspect.stdout)
                       if item.get('Type') == 'volume'] if inspect.returncode == 0 else []
            if any(not re.fullmatch('[0-9a-f]{64}', name) for name in volumes):
                raise RuntimeError('Unexpected named volume on disposable Auth database')
            self.owned_volumes.update(volumes)
            # Prove the exact lookup can see the still-attached volume before using
            # the same filter as an absence check. Cleanup still runs on failure.
            visible = all(subprocess.check_output(
                ['docker', 'volume', 'ls', '--filter', 'name=^' + name + '$', '--format', '{{.Name}}'],
                text=True,
            ).strip() == name for name in volumes)
        finally:
            subprocess.run(['docker', 'rm', '-f', '-v', self.container], capture_output=True, check=False)
        remaining = subprocess.check_output(
            ['docker', 'ps', '-a', '--filter', 'name=^/' + self.container + '$', '--format', '{{.Names}}'],
            text=True,
        ).strip()
        if remaining:
            raise RuntimeError('Owned disposable Auth database container remains')
        for name in self.owned_volumes:
            if subprocess.check_output(
                ['docker', 'volume', 'ls', '--filter', 'name=^' + name + '$', '--format', '{{.Name}}'],
                text=True,
            ).strip():
                raise RuntimeError('Owned disposable Auth database volume remains')
        if not visible:
            raise RuntimeError('Owned volume lookup failed its presence control')
        self.volume_cleanup_verified = True

    def __enter__(self) -> 'DisposableAuthPostgres':
        try:
            result = subprocess.run(
                ['docker', 'run', '-d', '--name', self.container,
                 '-e', 'POSTGRES_HOST_AUTH_METHOD=trust', '-p', '127.0.0.1::5432', 'postgres:17'],
                capture_output=True, check=False,
            )
            if result.returncode:
                raise RuntimeError('Disposable Auth database did not start')
            deadline = time.monotonic() + 30
            while subprocess.run(
                ['docker', 'exec', self.container, 'pg_isready', '-h', '127.0.0.1', '-U', 'postgres'],
                capture_output=True, check=False,
            ).returncode:
                if time.monotonic() >= deadline:
                    raise RuntimeError('Disposable Auth database readiness deadline exceeded')
                time.sleep(0.2)
            ports = json.loads(subprocess.check_output(
                ['docker', 'inspect', '--format', '{{json .NetworkSettings.Ports}}', self.container], text=True,
            ))
            binding = ports['5432/tcp'][0]
            if binding['HostIp'] != '127.0.0.1':
                raise RuntimeError('Unexpected disposable database listener binding')
            self.port = int(binding['HostPort'])
            self.sql('CREATE ROLE anon NOLOGIN; CREATE ROLE authenticated NOLOGIN; '
                     'CREATE ROLE service_role NOLOGIN BYPASSRLS;', database='postgres')
            super().__enter__()
            return self
        except BaseException:
            self.remove_container()
            raise

    def __exit__(self, *_: object) -> None:
        try:
            super().__exit__()
        finally:
            self.remove_container()
