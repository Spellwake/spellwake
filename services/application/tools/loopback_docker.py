"""Fixture-only Docker argv adaptation; changes publication addresses, never Auth behavior."""
import json
import os
from pathlib import Path
import re
import sys

VALUE_FLAGS = {'--name', '--hostname', '-e', '--env', '--env-file', '-v', '--volume', '--volumes-from',
    '--tmpfs', '-p', '--publish', '--expose', '--health-cmd', '--health-interval', '--health-timeout',
    '--health-retries', '--health-start-period', '--restart', '--security-opt', '--add-host', '--network',
    '--network-alias', '--label', '-l', '--entrypoint', '--user', '-u', '--workdir', '-w', '--shm-size'}
BOOL_FLAGS = {'--rm', '-d', '--detach', '-i', '--interactive', '-t', '--tty', '--init', '--read-only'}
SERVICES = {'db', 'kong', 'auth', 'rest', 'realtime', 'storage', 'imgproxy', 'studio', 'edge_runtime',
            'vector', 'analytics', 'pooler', 'mailpit', 'inbucket', 'pg_meta'}


def loopback_mapping(value: str) -> str:
    """Accept only unambiguous single-port Docker mappings; retain their ports/protocol."""
    if value.startswith('127.0.0.1:'):
        tail = value.removeprefix('127.0.0.1:')
        if re.fullmatch(r'\d{0,5}:\d{1,5}(?:/(?:tcp|udp))?', tail):
            return value
        raise ValueError('Unsupported owned port mapping')
    if re.fullmatch(r'\d{1,5}(?:/(?:tcp|udp))?', value):
        return '127.0.0.1::' + value
    if re.fullmatch(r'\d{1,5}:\d{1,5}(?:/(?:tcp|udp))?', value):
        return '127.0.0.1:' + value
    raise ValueError('Unsupported owned port mapping')


def bind_arguments(arguments: list[str], project: str, networks: set[str], control: str) -> tuple[list[str], dict | None]:
    """Parse Docker flags only before the image, skipping flag values atomically."""
    if not arguments or arguments[0] not in ('create', 'run'):
        return arguments, None
    output = arguments.copy()
    index = 1
    names: list[str] = []
    selected_networks: list[str] = []
    labels: list[str] = []
    ports: list[str] = []
    rewrites = 0
    while index < len(arguments) and arguments[index].startswith('-'):
        flag = arguments[index]
        if flag in BOOL_FLAGS:
            index += 1
            continue
        if flag not in VALUE_FLAGS or index + 1 >= len(arguments):
            raise ValueError('Unsupported Docker fixture flag: ' + (flag.split('=', 1)[0] if re.fullmatch(r'--?[a-z][a-z-]*(?:=.*)?', flag) else 'unsupported-form'))
        value = arguments[index + 1]
        if flag == '--name':
            names.append(value)
        elif flag == '--network':
            selected_networks.append(value)
        elif flag in ('--label', '-l'):
            labels.append(value)
        elif flag in ('-p', '--publish'):
            output[index + 1] = loopback_mapping(value)
            ports.append(output[index + 1])
            rewrites += output[index + 1] != value
        index += 2
    if index >= len(arguments):
        raise ValueError('Docker fixture image is missing')
    if not ports:
        return output, None
    if len(names) != 1 or names[0] not in ({'supabase_' + service + '_' + project for service in SERVICES} | {control}):
        raise ValueError('Publication is not owned by this fixture')
    if len(selected_networks) != 1 or selected_networks[0] not in networks or [label for label in labels if label.startswith('com.supabase.cli.project=')] != ['com.supabase.cli.project=' + project]:
        raise ValueError('Publication network or project label differs')
    return output, {'name': names[0], 'phase': 'control' if names[0] == control else 'cli', 'ports': ports, 'rewrites': rewrites}


def main(real_docker: str, project: str, networks: set[str], control: str, record: Path) -> None:
    """Preserve inherited process streams/env/exit via exec, recording no raw argv."""
    try:
        arguments, event = bind_arguments(sys.argv[1:], project, networks, control)
        if event is not None:
            descriptor = os.open(record, os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o600)
            with os.fdopen(descriptor, 'a') as file:
                file.write(json.dumps(event, separators=(',', ':')) + '\n')
        os.execv(real_docker, [real_docker, *arguments])
    except ValueError as failure:
        print('Owned Docker publication adapter rejected this invocation: ' + str(failure), file=sys.stderr)
        raise SystemExit(125) from None
    except OSError:
        print('Owned Docker publication adapter could not execute the original Docker', file=sys.stderr)
        raise SystemExit(125) from None
