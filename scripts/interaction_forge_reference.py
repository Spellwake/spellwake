#!/usr/bin/env python3
"""Execute immutable Blue interaction recipes with the pinned, isolated Forge harness."""
from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import shutil
import subprocess
import time
from pathlib import Path
from typing import Any

PIN = '3ead7e68d54bf95c6f778fd6aa7c604c036e0736'
AUTHORITY = {
    'content/manifest.json': '80dcf249f55095ab1bcb4942e4ebf9710fc1c63cc822019ad419c4a07c789165',
    'qualification/cases-v1.json': '828c3c353804c1f1b8ab789a33c29a7d615d9da5cbd90a92d2d1dc00f32eefcd',
    'qualification/focused-cases-v3.json': 'ebdce1ea1819b116f48532e79aedb038e828940d7981403c1ad70ee22e025045',
    'qualification/reader-controls-v3.json': '41f55885ee2a46beca09386b3babb98cce2271e56c438b5bd05db7a017451cca',
}
PLANNING = '.quzzar-system-design/architecture-proposed/work-planning-20261103/planning/blue-interaction-v1'
PROFILES = ('blue-interaction-v1', 'mandatory-trigger-v1', 'optional-payment-v1')
MANDATORY_PINS = {
    'primary-index': '6fbcb072d43a69d3f699150622e757b0bca34bd9af91e34a9c1e9504529a7ce0',
    'compiled-input': '767b6f92ba29a49ab5f5de1694e2a6cfbfec1625eb0a41cfed068ef4a8aa100b',
    'requirements': '5536a6abe5ce1e382f78a2ce1c8f3a4b06d16c2b14e67f8d71dfe196775edbec',
}


def strict_json(path: Path) -> Any:
    """Reject duplicate members and nonfinite numbers at the input boundary."""
    def pairs(items: list[tuple[str, Any]]) -> dict[str, Any]:
        result: dict[str, Any] = {}
        for key, value in items:
            if key in result:
                raise ValueError('Duplicate JSON member: ' + key)
            result[key] = value
        return result
    def constant(value: str) -> None:
        raise ValueError('Nonfinite JSON value: ' + value)
    return json.loads(path.read_text(), object_pairs_hook=pairs, parse_constant=constant)


def mandatory_authorities(root: Path, primary: Path, compiled: Path, requirements: Path) -> dict[str, Any]:
    """Read only explicitly supplied, frozen authorities and every epoch2 payload."""
    supplied = {'primary-index': primary, 'compiled-input': compiled, 'requirements': requirements}
    for name, path in supplied.items():
        if sha(path) != MANDATORY_PINS[name]:
            raise ValueError('Mandatory frozen authority mismatch: ' + name)
    index = strict_json(primary)
    if index['preparation_epoch'] != 2 or len(index['files']) != 13:
        raise ValueError('Mandatory primary epoch/inventory mismatch')
    payloads: dict[str, Any] = {}
    receipts = []
    for row in index['files']:
        # The immutable epoch deliberately retains the original path labels.
        # Payload resolution is local to the explicit index, never mutable planning discovery.
        path = primary.parent / Path(row['path']).name
        if sha(path) != row['sha256'] or path.stat().st_size != row['bytes']:
            raise ValueError('Mandatory primary payload mismatch: ' + str(path))
        payloads[path.name] = strict_json(path) if path.suffix == '.json' else path.read_text()
        receipts.append(dict(path=str(path.resolve()), sha256=row['sha256'], bytes=row['bytes']))
    data = strict_json(compiled); needs = strict_json(requirements)
    if data['profile'] != PROFILES[1] or data['schema_version'] != 1 or needs['input_sha256'] != sha(compiled):
        raise ValueError('Mandatory compiled/requirements relationship')
    ids = [case['id'] for case in data['cases']]
    if len(ids) != 23 or len(set(ids)) != 23 or ids != [case['id'] for case in needs['cases']]:
        raise ValueError('Mandatory exact23 case inventory')
    authorities = {}
    for row in data['authorities']:
        if row['name'] == 'primary_index': path = primary
        elif row['name'].startswith('primary_'): path = primary.parent / Path(row['path']).name
        else: path = root / row['path']
        if sha(path) != row['sha256']:
            raise ValueError('Mandatory source authority mismatch: ' + row['name'])
        authorities[row['name']] = dict(path=str(path.resolve()), sha256=row['sha256'])
    for hook in payloads['forge-hooks.json']['hooks']:
        if not hook['path'].startswith('work/reference-research/forge/'): continue
        path=root/hook['path']
        if sha(path)!=hook['sha256']: raise ValueError('Pinned Forge hook source mismatch: '+hook['id'])
        authorities['forge-hook:'+hook['id']]=dict(path=str(path.resolve()),sha256=hook['sha256'])
    return dict(source_root=str(root.resolve()),data=data, requirements=needs, payloads=payloads, authorities=authorities,
        supplied={name:dict(path=str(path.resolve()), sha256=sha(path)) for name,path in supplied.items()}, primary_payloads=receipts)


OPTIONAL_PINS = {
    'primary-index': '9ec45807922c73791a5878ad75fcbeee2ffaf2a2718850f16314121061f1f6ee',
    'compiled-input': 'd7858e932aff0de4960a7abe848a87061a350c69523a8917543dbb61ff35d1c2',
    'requirements': 'ce084e897cd3e6b3af4412ab6d431e600642a7d6b1c0bf89496470f698fab46e',
}


def optional_authorities(root: Path, primary: Path, compiled: Path, requirements: Path) -> dict[str, Any]:
    """Join exact native-bound source19, reviewed recipes and pinned Forge resources."""
    supplied = {'primary-index': primary, 'compiled-input': compiled, 'requirements': requirements}
    for name, path in supplied.items():
        if sha(path) != OPTIONAL_PINS[name]:
            raise ValueError('Optional frozen authority mismatch: ' + name)
    source_index = strict_json(primary); source = strict_json(compiled); data = strict_json(requirements)
    if source['profile'] != PROFILES[2] or data['profile'] != PROFILES[2] or source['epochs'] != data['epochs'] or source_index['forge_commit'] != PIN:
        raise ValueError('Optional source/profile/epoch mismatch')
    if len(source['definitions']) != 19 or len(data['definitions']) != 19 or len(source_index['cards']) != 19:
        raise ValueError('Optional exact source19 inventory')
    ids = [row['id'] for row in data['cases']]
    if ids != ['OP-FG01', 'OP-FG02'] + ['OP-R' + str(i).zfill(2) for i in range(1,25)]:
        raise ValueError('Optional exact26 case order')
    authorities = {}
    for index, row in enumerate([*source['authorities'], *data['authorities']]):
        path = root / row['path']
        if sha(path) != row['sha256']:
            raise ValueError('Optional source authority mismatch: ' + str(path))
        authorities['optional:' + str(index)] = dict(path=str(path.resolve()), sha256=row['sha256'])
    inventory_path = root / next(row['path'] for row in source['authorities'] if row['path'].endswith('selected19.full-inventory.json'))
    inventory = strict_json(inventory_path)
    raw = {row['oracle_id']: row for row in inventory['rows']}
    selected = {row['oracle_id']: row for row in source_index['cards']}
    native = {row['oracleId']: row for row in source['definitions']}
    cards = {}
    for definition in data['definitions']:
        oid = definition['oracle_id']; card = selected[oid]; saved = raw[oid]; bound = native[oid]
        if (definition['name'], definition['printing_id'], definition['source_line'], definition['raw_line_sha256'], definition['semantic_sha256']) != (bound['name'], bound['scryfallId'], bound['sourceLineOneBased'], bound['rawLineSha256IncludingLf'], bound['registrySignatureV1Sha256']):
            raise ValueError('Optional actual native source identity: ' + oid)
        if (card['name'], card['printing']['id'], card['raw_line_sha256_including_lf']) != (bound['name'], bound['scryfallId'], bound['rawLineSha256IncludingLf']) or saved['whole_raw_record']['legalities']['legacy'] != 'legal':
            raise ValueError('Optional saved source identity/Legacy: ' + oid)
        record = saved['whole_raw_record']
        face = {key: record.get(key) for key in ['name','mana_cost','type_line','oracle_text','power','toughness','colors']}
        face['oracle_text'] = card['full_forge_oracle']
        cards[bound['name']] = dict(oracle_id=oid, raw_line_sha256_including_lf=bound['rawLineSha256IncludingLf'], source_semantics=face,
            forge_printing=dict(edition=bound['set'],collector=bound['collector'],script_path=card['forge_script']['path'],script_sha256=card['forge_script']['sha256'],script_text=card['full_forge_script'],edition_path=card['forge_edition']['path'],edition_sha256=card['forge_edition']['sha256']),
            saved_source=record,native_source_binding=bound)
    payloads = {'source-authority.json': dict(cards=cards, tokens={})}
    for index, name in enumerate(['w_1_1_soldier.txt','b_2_2_zombie.txt']):
        path = root / 'work/reference-research/forge/forge-gui/res/tokenscripts' / name
        payloads['source-authority.json']['tokens'][str(index)] = dict(path=str(path.relative_to(root)),sha256=sha(path))
    return dict(profile=PROFILES[2],source_root=str(root.resolve()),data=data,requirements=data,payloads=payloads,authorities=authorities,
        supplied={name:dict(path=str(path.resolve()),sha256=sha(path)) for name,path in supplied.items()},primary_payloads=[])


def reference_alias(ref: dict[str, Any]) -> str:
    """Name a physical source or declared creation coordinate without guessing an engine ID."""
    if ref['kind'] in ('physical_current', 'physical_at_cause'): return ref['alias']
    if ref['kind'] == 'token_at_cause': return reference_alias(ref['token'])
    if ref['kind'] == 'created_token':
        entry = ref['creation_entry']; name = entry.get('trigger', entry.get('operation'))
        return 'token:' + name + ':' + str(ref['instruction']) + ':' + ref['creator'] + ':' + str(ref['member'])
    if ref['kind'] == 'saved_object': return 'saved:' + ref['binding']
    raise ValueError('Unsupported frozen object reference')


def make_mandatory_inputs(authority: dict[str, Any], resource: Path, out: Path) -> dict[str, Any]:
    """Lower only source/setup/actions/causal choices; primary expected facts never enter the JVM."""
    data = authority['data']; source = authority['payloads']['source-authority.json']
    retained = out / 'selected-resources'; retained.mkdir()
    optional = authority.get('profile') == PROFILES[2]
    if optional:
        root_resource=Path(authority['source_root'])/'work/reference-research/forge/forge-gui/res'
        definitions = {}; by_oracle = {}; colors = {'W':1,'U':2,'B':4,'R':8,'G':16}
        for definition in data['definitions']:
            name=definition['name']; card=source['cards'][name]; printing=card['forge_printing']; receipts=[]
            for kind in ('edition','script'):
                path=resource/Path(printing[kind+'_path']).relative_to(root_resource)
                if sha(path)!=printing[kind+'_sha256']:raise ValueError('Optional Forge resource mismatch: '+name)
                target=retained/path.name
                if target.exists():
                    if sha(target)!=sha(path):raise ValueError('Optional retained resource collision')
                else:shutil.copyfile(path,target)
                receipts.append(dict(path=str(path),sha256=sha(path),retained=str(target.relative_to(out))))
            definitions[name]=dict(name=name,set=printing['edition'],collector_number=printing['collector'],oracle_id=definition['oracle_id'],face=card['source_semantics'],color_mask=sum(colors[c] for c in card['source_semantics']['colors']),source_receipt=dict(saved_card=card,actual_record=card['saved_source'],native_source_binding=card['native_source_binding'],raw_line_sha256=definition['raw_line_sha256'],forge_resources=receipts))
            by_oracle[definition['oracle_id']]=name
    else:
        content=strict_json(Path(authority['authorities']['content']['path']))
        guards={row['name']:row for row in content['cards']}
        selected={row['source_line']:row for row in data['definitions']}; records={}
        with gzip.open(authority['authorities']['catalogue']['path'],'rb') as stream:
            for number,line in enumerate(stream,1):
                if number not in selected:continue
                definition=selected[number];record=json.loads(line);name=definition['name'];card=source['cards'][name]
                if hashlib.sha256(line).hexdigest()!=definition['raw_line_sha256'] or record['oracle_id']!=definition['oracle_id'] or record['id']!=definition['printing_id'] or record['name']!=name:
                    raise ValueError('Actual selected source row identity: '+name)
                if {key:record.get(key) for key in card['source_semantics']}!=card['source_semantics']:
                    raise ValueError('Actual selected source semantic tuple: '+name)
                guard=guards[name]['sourceGuard']
                if record['layout']!=guard['layout'] or record['legalities']['modern']!=guard['savedModernLegality']:
                    raise ValueError('Actual selected source layout/legality: '+name)
                for key,value in guard['rawTraits'].items():
                    if (key in record)!=value['present'] or record.get(key)!=value['value']:
                        raise ValueError('Actual selected raw source trait: '+name+'/'+key)
                records[name]=record
        if set(records)!={row['name'] for row in data['definitions']}:raise ValueError('Actual selected19 source row inventory')
        definitions = {}; by_oracle = {}; colors = {'W':1,'U':2,'B':4,'R':8,'G':16}
        for definition in data['definitions']:
            name = definition['name']; card = source['cards'][name]; printing = card['forge_printing']
            if definition['oracle_id'] != card['oracle_id'] or definition['raw_line_sha256'] != card['raw_line_sha256_including_lf']:
                raise ValueError('Mandatory source definition relationship')
            receipts=[]
            for kind in ('edition','script'):
                path = resource / Path(printing[kind+'_path']).relative_to('work/reference-research/forge/forge-gui/res')
                if sha(path) != printing[kind+'_sha256']: raise ValueError('Mandatory Forge resource mismatch: '+name)
                target = retained / path.name
                if target.exists():
                    if sha(target) != sha(path): raise ValueError('Retained resource collision')
                else: shutil.copyfile(path,target)
                receipts.append(dict(path=str(path),sha256=sha(path),retained=str(target.relative_to(out))))
            face=dict(card['source_semantics'])
            # The pinned Forge script uses its old named-self wording. Its exact text,
            # source identity and current-this-creature meaning are retained independently.
            oracle = next(line[7:] for line in printing['script_text'].splitlines() if line.startswith('Oracle:'))
            face['oracle_text']=oracle
            definitions[name]=dict(name=name,set=printing['edition'],collector_number=printing['collector'],oracle_id=definition['oracle_id'],
                face=face,color_mask=sum(colors[c] for c in card['source_semantics']['colors']),
                source_receipt=dict(saved_card=card,actual_record=records[name],raw_line_sha256=definition['raw_line_sha256'],forge_resources=receipts))
            by_oracle[definition['oracle_id']]=name
    token_resources=[]
    for token in source['tokens'].values():
        path=resource/Path(token['path']).relative_to('work/reference-research/forge/forge-gui/res')
        if sha(path)!=token['sha256']: raise ValueError('Mandatory token resource mismatch')
        target=retained/path.name;shutil.copyfile(path,target)
        token_resources.append(dict(path=str(path),sha256=sha(path),retained=str(target.relative_to(out))))
    cases=[]
    steps=['upkeep','draw','precombat_main','beginning_combat','declare_attackers','declare_blockers','first_strike_damage','combat_damage','end_combat','postcombat_main','end','cleanup']
    def boundary(value: dict[str,Any]) -> dict[str,Any]:
        return dict(individual_turn=value['turn'],step=value['step'].replace('-','_'),active_role=value['active'],seat=value['holder'])
    for case in data['cases']:
        init=case['initialization']; actions=[];combat=[];paid={'P0':[],'P1':[]}
        casts={row['id']:reference_alias(row['source']) for row in case['instructions'] if row['kind'].startswith('cast_')}
        def entry(row: dict[str,Any]) -> str:
            if row['kind']=='cast_entry':return row['operation'] if optional else casts[row['operation']]
            if row['kind']=='trigger_entry':return row['trigger']
            if optional and row['kind']=='copy_entry':return row['copy']
            raise ValueError('Unsupported stack entry reference')
        def entries(rows: list[dict[str,Any]]) -> list[str]:
            return [entry(row) for row in rows]
        for raw in case['instructions']:
            kind=raw['kind']; common=dict(policy_id=raw['id'],input_instruction=raw)
            if 'point' in raw:
                common.update(boundary(raw['point']['boundary'])); common['seat']=raw['point']['boundary']['holder']
                common['expected_stack_bottom_first']=entries(raw['point']['stack_bottom_first'])
            if kind in ('play_land','activate_mana','cast_untargeted','cast_targeted'):
                common.update(kind='cast' if kind.startswith('cast_') else kind,alias=reference_alias(raw['source']))
                if kind=='activate_mana':
                    common['mana_color']={'Plains':'W','Island':'U','Swamp':'B','Mountain':'R','Forest':'G'}.get(raw['ability'].get('land_type'),'G')
                    paid[common['seat']].append(common['alias'])
                if kind.startswith('cast_'):
                    common['required_cost']=definitions[by_oracle[next(row['oracle_id'] for row in case['inventory'] if row['alias']==common['alias'])]]['face']['mana_cost']
                    common['paid_sources']=paid[common['seat']]; paid[common['seat']]=[]
                    common['mana_units_paid']=sum(int(row['amount']) for row in raw['payment'])
                    if 'target' in raw:
                        target=raw['target']; common['target']=dict(kind=target['kind'])
                        if target['kind']=='object':common['target']['card']=reference_alias(target['object'])
                        elif target['kind']=='stack':common['target']['spell']=entry(target['entry'])
                        else:common['target']['player']=target['player']
                actions.append(common)
            elif kind in ('advance_ready','pass_to'):
                common.update(boundary(raw['boundary']),kind='seek',expected_stack_bottom_first=entries(raw.get('stack_bottom_first',[])))
                if kind=='pass_to':common['seat']=raw['receiver']
                actions.append(common)
            elif kind in ('probe_mana','save_object'):
                common.update(kind=kind,alias=reference_alias(raw['source'])); actions.append(common)
            elif kind=='combat':
                recipe=dict(active_role=raw['active'],individual_turn=raw['turn'],defender_role='P1' if raw['active']=='P0' else 'P0',
                    attackers=[reference_alias(r) for r in raw['attackers']],blockers=[dict(blocker=reference_alias(r['blocker']),attacker=reference_alias(r['attacker'])) for r in raw['blockers']],input_instruction=raw)
                combat.append(recipe)
                actions.extend([dict(common,kind='declare_attackers',individual_turn=raw['turn'],step='declare_attackers',seat=raw['active'],expected_stack_bottom_first=[]),dict(common,policy_id=raw['id']+'.blockers',kind='declare_blockers',individual_turn=raw['turn'],step='declare_blockers',seat=recipe['defender_role'],expected_stack_bottom_first=[])])
            else:raise ValueError('Unsupported mandatory input instruction: '+kind)
        inventory=[dict(row,definition_ref=by_oracle[row['oracle_id']]) for row in case['inventory']]
        operations=[];shuffles=[];opening=[]
        if init['kind']=='opening':
            for role in ['P0','P1']:
                op=dict(operation_id=case['id']+'.'+role+'.initial',planned_owner=role,role='initial_shuffle',target_ref=case['id']+'.'+role+'.initial');operations.append(op)
                shuffles.append(dict(operation_id=op['operation_id'],target_aliases=next(row['top_first'] for row in init['shuffle_orders'] if row['role']==role)))
            operations.append(dict(operation_id=case['id']+'.starting_chooser',role='starting_chooser',chosen_chooser='P0',chosen_starting_seat=init['starter'],raw_signed_32=0,planned_bound=2,planned_index=0))
            opening=[dict(chooser=row['role'],reply='keep',taken=row['taken'],aliases=[]) for row in init['keep_choices']]
            initialization=dict(kind='opening',life=init['life']);starter=init['starter']
        else:
            setup=dict(individual_turn=init['turn'],step=init['step'].replace('-','_'),seed_active_role=init['active'],life=init['life'],initial_battlefield_order=[r['alias'] for r in init['battlefield_oldest_first']],battlefield=init['battlefield_oldest_first'])
            for zone in ('hand','library_top_first','graveyard_top_first','exile'):setup[zone]={r['role']:r[zone] for r in init['zones']}
            initialization=dict(kind='scheduled',setup=setup);starter=init['active']
        stop=case['stop']
        if optional and stop['kind'] in ('bounded_ready','bounded_at_stack'):stop=dict(boundary(stop['boundary']),kind='at_stack' if stop['kind']=='bounded_at_stack' else 'ready_priority',priority_holder=stop['boundary']['holder'],required_settlements=[],boundary_ref=case['id']+'.stop',expected_stack_bottom_first=entries(stop.get('stack_bottom_first',[])))
        elif stop['kind']=='bounded_ready':stop=dict(boundary(stop['boundary']),kind='ready_priority',priority_holder=stop['boundary']['holder'],required_settlements=[],boundary_ref=case['id']+'.stop')
        decisions=[dict(row,kind='order_graveyard',seat=row['chooser'],individual_turn=next(i['turn'] for i in case['instructions'] if i['kind']=='combat'),step='combat_damage',aliases=[reference_alias(r) for r in row['entrants_top_first']]) for row in case['graveyard_choices']]
        cases.append(dict(case_id=case['id'],initialization=initialization,starting_role=starter,inventory=inventory,actions=actions,decisions=decisions,opening_policy=opening,forge_operation_queue=operations,shuffle_operations=shuffles,combat=combat,watchdog=case['budget'],pass_policy=dict(allowed_steps=steps),stop=stop,trigger_groups=case['trigger_groups'],player_choices=case['player_choices'],timestamp_choices=case['timestamp_choices'],causal_operations=case['causal_operations'],token_art_entropy=dict(raw_signed_32=0,maximum_samples_per_creation=4,purpose='Only source-traced TokenDb printing/PaperToken artwork random consumers; no gameplay chooser')))
    if optional:
        for lowered,case in zip(cases,data['cases'],strict=True):lowered['decision_plans']=case['decision_plans']
    return dict(schema='interaction-game-inputs-v1',profile=PROFILES[2] if optional else PROFILES[1],authority=authority['supplied'],definitions=definitions,token_resources=token_resources,cases=cases)


def sha(path: Path) -> str:
    """Hash actual file bytes without a second full packet allocation."""
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def save(path: Path, value: Any) -> None:
    """Create one immutable receipt, rejecting accidental attempt overwrites."""
    with path.open('x') as stream:
        json.dump(value, stream, indent=2)
        stream.write('\n')


def make_inputs(planning: Path, resource: Path, out: Path) -> dict[str, Any]:
    """Translate frozen action/setup inputs; expected gameplay values stay out of producer input."""
    for relative, digest in AUTHORITY.items():
        if sha(planning / relative) != digest:
            raise ValueError('Frozen authority mismatch: ' + relative)
    manifest = json.loads((planning / 'content/manifest.json').read_text())
    full = json.loads((planning / 'qualification/cases-v1.json').read_text())
    focused = json.loads((planning / 'qualification/focused-cases-v3.json').read_text())
    source = json.loads((planning / 'qualification/source-index.json').read_text())
    printings = {row['name']: row for row in source['forgePrintings']}
    retained = out / 'selected-resources'
    retained.mkdir()
    definitions: dict[str, Any] = {}
    for card in manifest['cards']:
        name = card['name']
        printing = printings[name]
        receipts = []
        for field in ['edition', 'script']:
            source_path = resource / Path(printing[field + 'Path']).relative_to('work/reference-research/forge/forge-gui/res')
            expected = printing[field + 'Sha256']
            if sha(source_path) != expected:
                raise ValueError('Forge resource mismatch: ' + name + ' ' + field)
            target = retained / source_path.name
            if not target.exists():
                shutil.copyfile(source_path, target)
            elif sha(target) != expected:
                raise ValueError('Resource retention collision')
            receipts.append(dict(path=str(source_path), sha256=expected, retained=str(target.relative_to(out))))
        definitions[name] = dict(name=name, set=printing['edition'], collector_number=printing['collector'],
            oracle_id=card['oracleId'], face=card['faces'][0], color_mask=0 if not card['rawSemanticFeatures']['colors'] else 2 if card['rawSemanticFeatures']['colors']==['U'] else 16,
            source_receipt=dict(saved_card=card, forge_resources=receipts))
    inventory = [dict(alias=alias, owner=row['role'], definition_ref=row['name']) for alias,row in full['inventory'].items()]
    cases = []
    allowed_steps = ['upkeep','draw','precombat_main','beginning_combat','declare_attackers','declare_blockers','first_strike_damage','combat_damage','end_combat','postcombat_main','end','cleanup']
    for case in full['cases']:
        actions = []
        for raw in case['actions']:
            row = dict(raw, alias=raw['card'], seat=raw['actor_role'], policy_id=raw['id'])
            row['kind'] = {'activate_intrinsic_mana':'activate_mana', 'cast':'cast'}.get(raw['kind'],raw['kind'])
            actions.append(row)
        combat = [dict(row, individual_turn=2*row['own_turn']-(1 if row['active_role']==case['starting_role'] else 0), defender_role='G' if row['active_role']=='U' else 'U') for row in case['combat']]
        operations = [dict(operation_id=f"{case['id']}.{role}.initial",planned_owner=role,role='initial_shuffle',target_ref=f"{case['id']}.{role}.initial") for role in ['U','G']]
        shuffles = [dict(operation_id=op['operation_id'],target_aliases=case['shuffle_target_orders_top_first'][op['planned_owner']]) for op in operations]
        operations.append(dict(operation_id=f"{case['id']}.starting_chooser",role='starting_chooser',chosen_chooser='U',chosen_starting_seat=case['starting_role'],raw_signed_32=0,planned_bound=2,planned_index=0))
        cases.append(dict(case_id=case['id'],initialization=dict(kind='opening'),starting_role=case['starting_role'],inventory=inventory,actions=actions,decisions=[],
            opening_policy=[dict(chooser=row['role'],reply=row['choice'],taken=row['taken'],aliases=[]) for row in case['opening_declarations']],
            forge_operation_queue=operations,shuffle_operations=shuffles,combat=combat,
            watchdog=dict(maximum_individual_turn=20,maximum_callbacks=10000),pass_policy=dict(allowed_steps=allowed_steps),stop=dict(kind='natural_outcome')))
    for case in focused['scenarios']:
        setup = case['setup']
        active = setup['active_role']
        number = setup['individual_turn']
        step = setup['step']
        actions=[]
        combat=[]
        live_plan: list[str] = []
        for index, raw in enumerate(case['actions']):
            kind=raw['kind']
            actor=raw.get('actor_role',active)
            current_step=raw.get('step',step)
            common=dict(seat=actor,active_role=active,active_own_turn=setup['active_own_turn'],individual_turn=number,step=current_step,expected_stack_bottom_first=list(raw.get('expected_stack_bottom_first',live_plan)))
            if kind=='actual_active_player_pass_to_U':
                actions.append(dict(common,kind='pass',policy_id=raw['id']))
            elif kind=='pass_priority':
                actions.append(dict(common,kind='pass',policy_id=raw['id']))
            elif kind=='ordinary_paid_hand_cast':
                for mana_index,source_alias in enumerate(raw['paid_sources']):
                    actions.append(dict(common,kind='activate_mana',alias=source_alias,policy_id=raw['id']+f'.mana.{mana_index}'))
                actions.append(dict(common,kind='cast',alias=raw['card'],policy_id=raw['id'],target=raw.get('target'),required_cost=raw['required_cost'],paid_sources=raw['paid_sources'],mana_units_paid=len(raw['paid_sources'])))
                live_plan.append(raw['card'])
            elif kind=='declare_attackers':
                actions.append(dict(common,kind='declare_attackers',aliases=raw['attackers'],policy_id=raw['id']))
                combat.append(dict(active_role=active,individual_turn=number,defender_role='G' if active=='U' else 'U',attackers=raw['attackers'],blockers=[],expected_eligible_attackers=raw['attackers']))
            elif kind=='declare_blockers':
                actions.append(dict(common,kind='declare_blockers',blockers=[dict(blocker=raw['blocker'],attacker=raw['attacker'])],policy_id=raw['id']))
                combat[-1]['blockers']=actions[-1]['blockers']
            else:
                raise ValueError('Unsupported frozen action '+case['id']+' '+kind)
        cases.append(dict(case_id=case['id'],initialization=dict(kind='scheduled',setup=setup),starting_role=active,inventory=inventory,actions=actions,decisions=[],opening_policy=[],forge_operation_queue=[],shuffle_operations=[],combat=combat,watchdog=dict(maximum_individual_turn=number,maximum_callbacks=2000),pass_policy=dict(allowed_steps=allowed_steps),stop=case['stop']))
    return dict(schema='interaction-game-inputs-v1',authority=AUTHORITY,definitions=definitions,cases=cases)


def classpath_receipts(parts: list[str]) -> list[dict[str, Any]]:
    """Retain the actual classpath file closure; JDK scope remains separately declared."""
    output=[]
    for part in parts:
        path=Path(part)
        if path.is_file():output.append(dict(path=str(path),kind='file',sha256=sha(path)))
        elif path.is_dir():
            files=[dict(path=str(p.relative_to(path)),sha256=sha(p)) for p in sorted(path.rglob('*')) if p.is_file()]
            output.append(dict(path=str(path),kind='directory',files=files))
        else:raise ValueError('Missing runtime classpath component: '+part)
    return output


def main() -> None:
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-root',type=Path,required=True)
    parser.add_argument('--out',type=Path,required=True)
    parser.add_argument('--case',action='append',default=[])
    parser.add_argument('--compile-only',action='store_true')
    parser.add_argument('--profile',choices=PROFILES,default=PROFILES[0])
    parser.add_argument('--primary-index',type=Path)
    parser.add_argument('--compiled-input',type=Path)
    parser.add_argument('--requirements',type=Path)
    args=parser.parse_args()
    explicit=[args.primary_index,args.compiled_input,args.requirements]
    if args.profile==PROFILES[2] and not all(explicit):parser.error('optional-payment-v1 requires --primary-index, --compiled-input and --requirements')
    if args.profile==PROFILES[1] and not all(explicit):parser.error('mandatory-trigger-v1 requires --primary-index, --compiled-input and --requirements')
    if args.profile==PROFILES[0] and any(explicit):parser.error('mandatory authorities are outside the Blue input grammar')
    root=args.source_root.resolve();out=args.out.resolve();out.mkdir(parents=True,exist_ok=False)
    checkout=Path(__file__).resolve().parents[1];planning=root/PLANNING;forge=root/'work/reference-research/forge'
    if subprocess.check_output(['git','rev-parse','HEAD'],cwd=forge,text=True).strip()!=PIN:raise ValueError('Forge source commit mismatch')
    resource=forge/'forge-gui/res'
    mandatory=optional_authorities(root,args.primary_index.resolve(),args.compiled_input.resolve(),args.requirements.resolve()) if args.profile==PROFILES[2] else mandatory_authorities(root,args.primary_index.resolve(),args.compiled_input.resolve(),args.requirements.resolve()) if args.profile==PROFILES[1] else None
    inputs=make_mandatory_inputs(mandatory,resource,out) if mandatory else make_inputs(planning,resource,out)
    selected=args.case or [c['case_id'] for c in inputs['cases']]
    if args.profile==PROFILES[2] and len(set(selected))!=len(selected):raise ValueError('Duplicate optional selected case')
    if set(selected)-{c['case_id'] for c in inputs['cases']}:raise ValueError('Unknown selected case')
    for directory in ['source','classes','evidence','java-temp','jvm-home','forge-gui-desktop','forge-gui']:(out/directory).mkdir()
    (out/'forge-gui/res').symlink_to(resource,target_is_directory=True)
    source_paths=[checkout/'tests/reference/forge/SpellwakeBlueInteractionReferenceTest.java',*[checkout/'scripts'/name for name in ['interaction_forge_reference.py','interaction_reference_compare.py','interaction_reference_controls.py','teaching_reference_compare.py','teaching_forge_reference.py']]]
    original_source_before={str(path):sha(path) for path in source_paths}
    for source in source_paths:
        shutil.copyfile(source,out/'source'/source.name)
    if mandatory:
        save(out/'authority-binding.json',{key:value for key,value in mandatory.items() if key not in ('data','requirements','payloads')})
        for name,path in [('primary-index',args.primary_index),('compiled-input',args.compiled_input),('requirements',args.requirements)]:shutil.copyfile(path,out/(name+'.json'))
    else:
        for relative in AUTHORITY:
            target=out/Path(relative).name;shutil.copyfile(planning/relative,target)
    save(out/'inputs.json',inputs)
    prior=json.loads((root/'work/engine-batches/supertypes-foundation/forge-casting-attempt1/commands.json').read_text())
    parts=prior['compile'][2].split(':')[1:]
    cp=str(out/'classes')+':'+':'.join(parts)
    compile_cmd=[prior['compile'][0],'-cp',cp,'-d',str(out/'classes'),str(out/'source/SpellwakeBlueInteractionReferenceTest.java')]
    methods='<include name="optional"/>' if args.profile==PROFILES[2] else '<include name="mandatory"/>' if mandatory else ''.join('<include name="'+case.lower()+'"/>' for case in selected)
    (out/'suite.xml').write_text('<!DOCTYPE suite SYSTEM "https://testng.org/testng-1.0.dtd">\n<suite name="Blue interaction" parallel="none"><test name="frozen"><classes><class name="forge.ai.SpellwakeBlueInteractionReferenceTest"><methods>'+methods+'</methods></class></classes></test></suite>\n')
    run_cmd=[prior['run'][0],'-Xmx3g','-cp',cp,'-Djava.awt.headless=true','-Djava.io.tmpdir='+str(out/'java-temp'),'-Duser.home='+str(out/'jvm-home'),'-Dspellwake.inputs.path='+str(out/'inputs.json'),'-Dspellwake.inputs.sha256='+sha(out/'inputs.json'),'-Dspellwake.evidence.dir='+str(out/'evidence'),'org.testng.TestNG','-d',str(out/'testng'),str(out/'suite.xml')]
    if mandatory:run_cmd.insert(2,'-Dspellwake.selected.cases='+','.join(selected))
    tracked=list((out/'source').iterdir())+[out/'inputs.json',out/'suite.xml']+([out/(name+'.json') for name in ['primary-index','compiled-input','requirements','authority-binding']] if mandatory else [out/Path(k).name for k in AUTHORITY])
    before={str(p.relative_to(out)):sha(p) for p in tracked}
    resource_paths={r['path'] for card in inputs['definitions'].values() for r in card['source_receipt']['forge_resources']}
    resource_paths.update(row['path'] for row in inputs.get('token_resources',[]))
    # TokenDb resolves editions through the loaded edition catalogue. Guard its
    # complete metadata lookup universe, then retain the actually selected token
    # printing in each birth receipt; no image bytes are loaded by this headless run.
    if mandatory:resource_paths.update(str(path) for path in (resource/'editions').rglob('*.txt'))
    resources_before={path:sha(Path(path)) for path in sorted(resource_paths)}
    authority_before={row['path']:sha(Path(row['path'])) for row in [*mandatory['authorities'].values(),*mandatory['supplied'].values(),*mandatory['primary_payloads']]} if mandatory else {}
    retained_before={str(p.relative_to(out)):sha(p) for p in sorted((out/'selected-resources').iterdir())}
    cp_before=classpath_receipts(parts)
    save(out/'commands.json',dict(compile=compile_cmd,run=run_cmd,cwd=str(out/'forge-gui-desktop'),forge_commit=PIN,selected=selected,source_before=before,classpath=cp_before,scope='Actual classpath files plus selected loaded resources/classes; not complete JDK, hermetic or authenticated execution'))
    for binary in ['java','javac']:
        executable=Path(prior['run'][0] if binary=='java' else prior['compile'][0]);save(out/(binary+'-toolchain.json'),dict(path=str(executable),sha256=sha(executable),version=subprocess.check_output([str(executable),'-version'],stderr=subprocess.STDOUT,text=True)))
    results={};class_before=None
    try:
        commands=[('compile',compile_cmd)]+([] if args.compile_only else [('run',run_cmd)])
        for name,command in commands:
            start=time.monotonic();started=time.time()
            with (out/(name+'.log')).open('x') as log, (out/(name+'.stderr')).open('x') as errors:
                try:
                    completed=subprocess.run(command,cwd=out/'forge-gui-desktop',stdout=log,stderr=errors if mandatory else subprocess.STDOUT,timeout=600)
                except subprocess.TimeoutExpired:
                    results[name]=dict(exit=None,seconds=round(time.monotonic()-start,3),interrupted=True,termination_reason='process_timeout')
                    raise
            results[name]=dict(exit=completed.returncode,seconds=round(time.monotonic()-start,3))
            if mandatory:results[name].update(command=command,cwd=str(out/'forge-gui-desktop'),started_unix=started,completed_unix=time.time(),interrupted=False,termination_reason=None,stdout=dict(path=str(out/(name+'.log')),sha256=sha(out/(name+'.log')),bytes=(out/(name+'.log')).stat().st_size),stderr=dict(path=str(out/(name+'.stderr')),sha256=sha(out/(name+'.stderr')),bytes=(out/(name+'.stderr')).stat().st_size))
            if mandatory and name=='run':results[name]['evidence']={str(path.relative_to(out)):dict(sha256=sha(path),bytes=path.stat().st_size) for path in sorted((out/'evidence').glob('*.json'))}
            print(name,completed.returncode,flush=True)
            if completed.returncode:break
            if name=='compile':class_before={str(p.relative_to(out)):sha(p) for p in sorted((out/'classes').rglob('*.class'))};save(out/'classes-before-run.json',class_before)
    finally:
        after={str(p.relative_to(out)):sha(p) for p in tracked};cp_after=classpath_receipts(parts);class_after={str(p.relative_to(out)):sha(p) for p in sorted((out/'classes').rglob('*.class'))}
        resources_after={path:sha(Path(path)) for path in resources_before};retained_after={path:sha(out/path) for path in retained_before};authority_after={path:sha(Path(path)) for path in authority_before};original_source_after={path:sha(Path(path)) for path in original_source_before}
        stability=dict(before=before,after=after,unchanged=before==after,classpath_unchanged=cp_before==cp_after,classes_before=class_before,classes_after=class_after,classes_unchanged=class_before==class_after if class_before is not None else None,resources_before=resources_before,resources_after=resources_after,resources_unchanged=resources_before==resources_after,retained_resources_before=retained_before,retained_resources_after=retained_after,retained_resources_unchanged=retained_before==retained_after)
        if mandatory:stability.update(authority_before=authority_before,authority_after=authority_after,authority_unchanged=authority_before==authority_after)
        stability.update(original_source_before=original_source_before,original_source_after=original_source_after,original_source_unchanged=original_source_before==original_source_after)
        save(out/'results.json',results);save(out/'source-stability.json',stability)
        if before!=after or cp_before!=cp_after or resources_before!=resources_after or retained_before!=retained_after or authority_before!=authority_after or original_source_before!=original_source_after or (class_before is not None and class_before!=class_after):raise ValueError('Source/runtime/build changed during execution')
    if any(row['exit'] for row in results.values()):raise SystemExit(1)


if __name__=='__main__':main()
