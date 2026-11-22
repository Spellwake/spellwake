"""Narrow shared lifecycle for the two owned Auth qualification HTTP services."""
import json
import os
from pathlib import Path
import re
import subprocess
import uuid

SERVICES = {
    'gotrue': ('public.ecr.aws/supabase/gotrue:v2.197.0', 'spellwake-auth-producer-', 9999),
    'postgrest': ('public.ecr.aws/supabase/postgrest:v16.5', 'spellwake-auth-rest-', 3000),
}


class DisposableHttpService:
    """Own one generated container and only files successfully created by it."""

    def __init__(self, kind: str, config: dict[str, str], stage: Path) -> None:
        self.image, prefix, self.port = SERVICES[kind]
        if any(not re.fullmatch('[A-Z][A-Z0-9_]*', key) or not isinstance(value, str)
               or any(char in value for char in '\n\r\0') for key, value in config.items()):
            raise ValueError('Invalid private qualification environment')
        if not stage.is_dir() or stage.stat().st_mode & 0o077:
            raise ValueError('Qualification stage must be an owner-only directory')
        self.config = config
        self.container = prefix + uuid.uuid4().hex
        self.environment_file = stage / 'private.env'
        self.log_file = stage / 'private-server.log'
        self.owns_environment = False
        self.attempted_start = False
        self.cleanup_verified = False
        self.base = ''
        self.image_id = ''

    def __enter__(self) -> 'DisposableHttpService':
        try:
            self.image_id = subprocess.check_output(
                ['docker', 'image', 'inspect', self.image, '--format', '{{.Id}}'], text=True,
            ).strip()
            fd = os.open(self.environment_file, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            self.owns_environment = True
            with os.fdopen(fd, 'w') as file:
                file.write(''.join(key + '=' + value + '\n' for key, value in self.config.items()))
            self.attempted_start = True
            result = subprocess.run(
                ['docker', 'run', '-d', '--name', self.container, '--env-file', str(self.environment_file),
                 '-p', f'127.0.0.1::{self.port}', self.image], capture_output=True, check=False,
            )
            if result.returncode:
                raise RuntimeError('Owned qualification HTTP service did not start')
            ports = json.loads(subprocess.check_output(
                ['docker', 'inspect', '--format', '{{json .NetworkSettings.Ports}}', self.container], text=True,
            ))
            bindings = ports[f'{self.port}/tcp']
            if len(bindings) != 1 or bindings[0]['HostIp'] != '127.0.0.1':
                raise RuntimeError('Unexpected owned HTTP listener binding')
            self.base = 'http://127.0.0.1:' + bindings[0]['HostPort']
            return self
        except BaseException:
            self.__exit__()
            raise

    def __exit__(self, *_: object) -> None:
        if self.cleanup_verified:
            return
        try:
            if self.attempted_start:
                try:
                    logs = subprocess.run(['docker', 'logs', self.container], capture_output=True, check=False)
                    fd = os.open(self.log_file, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
                    with os.fdopen(fd, 'wb') as file:
                        file.write(logs.stdout + logs.stderr)
                finally:
                    subprocess.run(['docker', 'rm', '-f', '-v', self.container], capture_output=True, check=False)
                    remaining = subprocess.check_output(
                        ['docker', 'ps', '-a', '--filter', 'name=^/' + self.container + '$', '--format', '{{.Names}}'],
                        text=True,
                    ).strip()
                    if remaining:
                        raise RuntimeError('Owned qualification HTTP container remains')
        finally:
            if self.owns_environment:
                self.environment_file.unlink(missing_ok=True)
                self.owns_environment = False
        self.cleanup_verified = True
