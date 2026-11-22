#!/usr/bin/env python3
"""Run frozen teaching recipes with isolated harness classes and the selected Forge runtime."""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import subprocess
import time
from pathlib import Path
from typing import Any

PIN = '3ead7e68d54bf95c6f778fd6aa7c604c036e0736'
SPEC_SHA = 'a2d2d0f4e52f19afdbf32888e77dd8ffb474502af4b8b7d1ec2f577c64c6a9a4'
MANIFEST_SHA = '6eac14f3638b072d38672a85284329a50e271b9cc650b98d60e4231685f7a11d'
PRINTINGS = {
    'Plains': ('AKH', '250'), 'Forest': ('AKH', '254'),
    'Glory Seeker': ('8ED', '21'), 'Savannah Lions': ('8ED', '43'),
    'Grizzly Bears': ('8ED', '256'), 'Spined Wurm': ('8ED', '279'),
    'Terrain Elemental': ('KLD', '272'), 'Alpine Grizzly': ('KTK', '127'),
    'Elite Vanguard': ('M10', '9'), 'Silvercoat Lion': ('M10', '32'),
    'Centaur Courser': ('M10', '172'), 'Runeclaw Bear': ('M10', '203'),
    'Pillarfield Ox': ('M13', '25'), 'Kalonian Tusker': ('M14', '182'),
    'Rumbling Baloth': ('M14', '193'), 'Oreskos Swiftclaw': ('M15', '22'),
    'Yoked Ox': ('ORI', '42'), 'Traveling Philosopher': ('THS', '34'),
    'Nessian Courser': ('THS', '165'), 'Loxodon Wayfarer': ('SOM', '15'),
}


def sha(path: Path) -> str:
    """Hash actual input bytes."""
    return hashlib.sha256(path.read_bytes()).hexdigest()


def save(path: Path, value: Any) -> None:
    """Write a fresh evidence document."""
    with path.open('x') as stream:
        json.dump(value, stream, indent=2)
        stream.write('\n')


def make_inputs(spec: dict[str, Any], manifest: dict[str, Any], resource: Path, out: Path) -> dict[str, Any]:
    """Translate frozen aliases/actions into the strict adapter's explicit recipes."""
    definitions = {}
    resources = out / 'selected-resources'
    resources.mkdir()
    for card in manifest['cards']:
        name = card['name']
        edition, collector = PRINTINGS[name]
        matching = []
        for candidate in sorted((resource / 'editions').glob('*.txt')):
            text = candidate.read_text()
            if f'Code={edition}\n' in text and re.search(r'^' + re.escape(collector) + r' [A-Z] ' + re.escape(name) + r'(?: @.*)?$', text, re.M):
                matching.append(candidate)
        if len(matching) != 1:
            raise ValueError(f'Exact existing printing not found: {name} {edition} {collector}')
        script_name = name.lower().replace(' ', '_') + '.txt'
        script = resource / 'cardsfolder' / name[0].lower() / script_name
        if not script.is_file() or f'Name:{name}\n' not in script.read_text():
            raise ValueError(f'Actual card script missing: {name}')
        receipts = []
        for source in [matching[0], script]:
            retained = resources / source.name
            if not retained.exists():
                shutil.copyfile(source, retained)
            receipts.append({'path': str(source), 'sha256': sha(source), 'retained': str(retained.relative_to(out))})
        definitions[name] = dict(name=name, set=edition, collector_number=collector, oracle_id=card['oracleId'], face=card['faces'][0], source_receipt=dict(saved_card=card, forge_resources=receipts))
    result: dict[str, Any] = {'schema': 'teaching-game-inputs-v1', 'definitions': definitions, 'case_spec_sha256': SPEC_SHA, 'manifest_sha256': MANIFEST_SHA, 'cases': []}
    for case in spec['cases']:
        starting = case['starting_role']
        individual = lambda role, own: 2 * own - (1 if role == starting else 0)
        actions = []
        unspent: dict[str, list[str]] = {'W': [], 'G': []}
        last_turn = 0
        for index, action in enumerate(case['actions']):
            row = dict(action, alias=action['card'], seat=action['role'], personal_turn=action['own_turn'], individual_turn=individual(action['role'], action['own_turn']), policy_id=f"{case['id']}.action.{index}")
            if row['individual_turn'] != last_turn:
                unspent = {'W': [], 'G': []}
                last_turn = row['individual_turn']
            row['kind'] = {'activate_intrinsic_mana': 'activate_mana', 'cast_creature': 'cast_untargeted'}.get(action['kind'], action['kind'])
            if row['kind'] == 'activate_mana':
                unspent[row['seat']].append(row['alias'])
            if row['kind'] == 'cast_untargeted':
                count = int(row['mana_units_paid'])
                row['paid_sources'] = unspent[row['seat']][:count]
                del unspent[row['seat']][:count]
            actions.append(row)
        decisions = []
        for order in case.get('state_check_order_choices', []):
            active_own = int(order['at'][1:].split('.')[0])
            decisions.append(dict(kind='order_graveyard', seat=order['owner_role'], personal_turn=active_own, individual_turn=individual(order['at'][0], active_own), step='combat_damage', aliases=order['selected_graveyard_top_first']))
        for discard in case['cleanup_discards']:
            decisions.append(dict(kind='discard', seat=discard['role'], personal_turn=discard['own_turn'], individual_turn=individual(discard['role'], discard['own_turn']), step='cleanup', aliases=discard['cards']))
        decisions.sort(key=lambda row: row['individual_turn'])
        opening = [dict(chooser=row['role'], reply=row['choice'], aliases=[], taken=row['taken']) for row in case['opening_declarations']]
        for row in case.get('london_bottom', []):
            opening.append(dict(chooser=row['role'], reply='bottom', aliases=row['selected_top_first']))
        operations = []
        shuffles = []
        for role in ['W', 'G']:
            op = f"{case['id']}.{role}.initial"
            operations.append(dict(operation_id=op, planned_owner=role, role='initial_shuffle', target_ref=op))
            shuffles.append(dict(operation_id=op, target_aliases=case['shuffle_target_orders_top_first'][role]))
        operations.append(dict(operation_id=f"{case['id']}.starting_chooser", role='starting_chooser', chosen_chooser='W', chosen_starting_seat=starting, raw_signed_32=0, planned_bound=2, planned_index=0))
        for index, row in enumerate(case.get('mulligan_shuffles', [])):
            op = f"{case['id']}.{row['role']}.mulligan.{index}"
            operations.append(dict(operation_id=op, planned_owner=row['role'], role='mulligan_shuffle', target_ref=op))
            shuffles.append(dict(operation_id=op, target_aliases=row['target_order_top_first']))
        inventory = [dict(alias=alias, owner=row['role'], definition_ref=row['name']) for alias, row in spec['inventory'].items()]
        combat = [dict(row, individual_turn=individual(row['active_role'], row['own_turn'])) for row in case['combat']]
        result['cases'].append(dict(case_id=case['id'], starting_role=starting, inventory=inventory, actions=actions, decisions=decisions, opening_policy=opening, forge_operation_queue=operations, shuffle_operations=shuffles, combat=combat, watchdog=dict(maximum_individual_turn=13, maximum_callbacks=10000), pass_policy=dict(allowed_steps=['upkeep','draw','precombat_main','beginning_combat','declare_attackers','declare_blockers','first_strike_damage','combat_damage','end_combat','postcombat_main','end','cleanup'])))
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-root', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    root = args.source_root.resolve()
    checkout = Path(__file__).resolve().parents[1]
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    planning = root / '.quzzar-system-design/architecture-proposed/work-planning-20261103/planning/first-wave-v1'
    spec_path, manifest_path = planning / 'qualification/cases-v1.json', planning / 'content/first-pair.json'
    if sha(spec_path) != SPEC_SHA or sha(manifest_path) != MANIFEST_SHA:
        raise ValueError('Frozen source identity mismatch')
    spec, manifest = json.loads(spec_path.read_text()), json.loads(manifest_path.read_text())
    forge = root / 'work/reference-research/forge'
    pin = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=forge, text=True).strip()
    if pin != PIN:
        raise ValueError(f'Forge pin mismatch {pin}')
    shutil.copyfile(spec_path, out / 'cases-v1.json')
    shutil.copyfile(manifest_path, out / 'first-pair.json')
    source_dir = out / 'source'
    source_dir.mkdir()
    for source in [Path(__file__), checkout / 'tests/reference/forge/SpellwakeTeachingDeckReferenceTest.java']:
        shutil.copyfile(source, source_dir / source.name)
    resource = forge / 'forge-gui/res'
    inputs = make_inputs(spec, manifest, resource, out)
    save(out / 'inputs.json', inputs)
    for directory in ['classes', 'evidence', 'java-temp', 'jvm-home', 'forge-gui-desktop', 'forge-gui']:
        (out / directory).mkdir()
    (out / 'forge-gui/res').symlink_to(resource, target_is_directory=True)
    prior = json.loads((root / 'work/engine-batches/supertypes-foundation/forge-casting-attempt1/commands.json').read_text())
    cp = str(out / 'classes') + ':' + prior['compile'][2].split(':', 1)[1]
    compile_command = [prior['compile'][0], '-cp', cp, '-d', str(out / 'classes'), str(source_dir / 'SpellwakeTeachingDeckReferenceTest.java')]
    suite = '<!DOCTYPE suite SYSTEM "https://testng.org/testng-1.0.dtd">\n<suite name="Teaching ordinary games" parallel="none"><test name="frozen"><classes><class name="forge.ai.SpellwakeTeachingDeckReferenceTest"/></classes></test></suite>\n'
    (out / 'suite.xml').write_text(suite)
    run_command = [prior['run'][0], '-cp', cp, '-Djava.awt.headless=true', '-Djava.io.tmpdir=' + str(out / 'java-temp'), '-Duser.home=' + str(out / 'jvm-home'), '-Dspellwake.inputs.path=' + str(out / 'inputs.json'), '-Dspellwake.inputs.sha256=' + sha(out / 'inputs.json'), '-Dspellwake.evidence.dir=' + str(out / 'evidence'), 'org.testng.TestNG', '-d', str(out / 'testng'), str(out / 'suite.xml')]
    tracked = list(source_dir.iterdir()) + [out / 'inputs.json', out / 'cases-v1.json', out / 'first-pair.json', out / 'suite.xml']
    source_before = {str(p.relative_to(out)): sha(p) for p in tracked}
    runtime_cp = [dict(path=part, kind='directory' if Path(part).is_dir() else 'file', sha256=sha(Path(part)) if Path(part).is_file() else None) for part in cp.split(':')[1:]]
    save(out / 'commands.json', dict(compile=compile_command, run=run_command, cwd=str(out / 'forge-gui-desktop'), forge_commit=pin, classpath=runtime_cp, source_before=source_before, scope='Selected loaded-class and resource receipt, not hermetic or authenticated execution'))
    java=Path(prior['run'][0]);javac=Path(prior['compile'][0])
    jdk_release=java.parent.parent/'release'
    save(out/'toolchain.json',dict(java=dict(path=str(java),sha256=sha(java),version=subprocess.check_output([str(java),'-version'],stderr=subprocess.STDOUT,text=True)),javac=dict(path=str(javac),sha256=sha(javac),version=subprocess.check_output([str(javac),'-version'],stderr=subprocess.STDOUT,text=True)),release=dict(path=str(jdk_release),sha256=sha(jdk_release),contents=jdk_release.read_text())))
    results = {}
    before_classes = None
    for name, command in [('compile', compile_command), ('run', run_command)]:
        started = time.monotonic()
        with (out / (name + '.log')).open('x') as log:
            result = subprocess.run(command, cwd=out / 'forge-gui-desktop', stdout=log, stderr=subprocess.STDOUT, timeout=300)
        results[name] = dict(exit=result.returncode, seconds=round(time.monotonic() - started, 3))
        print(name, result.returncode, flush=True)
        if result.returncode:
            break
        if name=='compile':
            before_classes={str(p.relative_to(out)):sha(p) for p in sorted((out/'classes').rglob('*.class'))}
            save(out/'classes-before-run.json',before_classes)
    save(out / 'results.json', results)
    save(out / 'source-stability.json', dict(before=source_before, after={str(p.relative_to(out)): sha(p) for p in tracked}))
    after_classes={str(p.relative_to(out)):sha(p) for p in sorted((out/'classes').rglob('*.class'))}
    save(out/'classes.json',after_classes)
    save(out/'class-stability.json',dict(before=before_classes,after=after_classes,unchanged=before_classes==after_classes))


if __name__ == '__main__':
    main()
