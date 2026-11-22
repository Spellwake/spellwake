#!/usr/bin/env python3
"""Strict selected teaching-game reader, semantic join and frozen-fact adjudication."""
from __future__ import annotations

import argparse
import copy
import hashlib
import json
import re
from pathlib import Path
from typing import Any

from teaching_forge_reference import PRINTINGS

ROLES = {'41': 'W', '3': 'G'}
MANA = {'W': 'white', 'U': 'blue', 'B': 'black', 'R': 'red', 'G': 'green', 'C': 'colorless'}
FORGE_PIN = '3ead7e68d54bf95c6f778fd6aa7c604c036e0736'
SPEC_SHA = 'a2d2d0f4e52f19afdbf32888e77dd8ffb474502af4b8b7d1ec2f577c64c6a9a4'
# Counted directly from the compressed catalogue bytes bound by the frozen manifest.
CATALOGUE_RECORDS = 39196


class EvidenceError(ValueError):
    """Selected evidence is incomplete, corrupt or semantically different."""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise EvidenceError(message)


def read(path: Path) -> Any:
    """Reject duplicate JSON members rather than silently keeping the last value."""
    def pairs(values: list[tuple[str, Any]]) -> dict[str, Any]:
        result = {}
        for key, value in values:
            require(key not in result, f'duplicate JSON member {key}')
            result[key] = value
        return result
    return json.loads(path.read_text(), object_pairs_hook=pairs)


def same(actual: Any, expected: Any, label: str) -> None:
    if actual != expected:
        raise EvidenceError(f'{label}: actual={str(actual)[:600]} expected={str(expected)[:600]}')


def unique(rows: list[dict[str, Any]], key: str, label: str) -> dict[str, dict[str, Any]]:
    result = {}
    for row in rows:
        value = row[key]
        require(value not in result, f'duplicate {label}: {value}')
        result[value] = row
    return result


def role(seat: Any) -> str:
    return ROLES[str(seat)]


def cost_tuple(text: str) -> dict[str, Any]:
    symbols = re.findall(r"\{([^}]+)\}", text)
    require(bool(symbols) and "".join("{"+v+"}" for v in symbols) == text, "unrecognized mana cost")
    generic, required = 0, {}
    for symbol in symbols:
        if symbol.isdecimal():
            generic += int(symbol)
        else:
            require(symbol in MANA, "unsupported cost symbol")
            color = MANA[symbol]
            required[color] = str(int(required.get(color, "0")) + 1)
    return dict(generic=str(generic), required=required)


def validate_world(world: dict[str, Any], aliases: set[str]) -> dict[str, Any]:
    """Cross-check physical cards, membership, unique zones and inventory conservation."""
    seen = []
    for owner, player in world['players'].items():
        owned = []
        for zone in ['library', 'hand', 'battlefield', 'graveyard', 'exile']:
            for alias in player[zone]:
                require(alias in aliases, 'unknown pile alias')
                same(world['cards'][alias]['zone'], zone, 'pile/card zone')
                same(world['cards'][alias]['owner'], owner, 'pile/card owner')
            owned += player[zone]
        owned += [alias for alias in world['stack'] if world['cards'][alias]['owner'] == owner]
        same(len(owned), 60, 'per-owner inventory conservation')
        seen += owned
    same(set(seen), aliases, 'world membership coverage')
    same(len(seen), len(set(seen)), 'world duplicate membership')
    same({a for a,c in world['cards'].items() if c['zone']=='stack'}, set(world['stack']), 'stack physical membership')
    return world


def forge_world(world: dict[str, Any], aliases: set[str]) -> dict[str, Any]:
    same(set(world['cards']), aliases, 'Forge full physical inventory')
    players = {}
    for owner, player in world['players'].items():
        require(owner in ['W', 'G'], 'unknown Forge owner')
        players[owner] = {key: player[key] for key in ['life', 'library', 'hand', 'battlefield', 'graveyard', 'exile']}
        players[owner]['mana'] = {long: player['mana'][short] for short, long in MANA.items()}
        for zone in ['hand', 'battlefield', 'exile']:
            players[owner][zone] = sorted(players[owner][zone])
    cards = {}
    for alias, card in world['cards'].items():
        cards[alias] = {key: card[key] for key in ['owner', 'controller', 'zone', 'tapped', 'power', 'toughness', 'marked_damage', 'sick', 'plus_one_counters', 'minus_one_counters']}
    stack = [row['ability']['source'] for row in world['stack']]
    for owner, player in players.items():
        counts=world['header']['players'][owner]['counts']
        for zone in ['library','hand','battlefield','graveyard','exile']:
            same(len(player[zone]),counts[zone],'Forge count vs physical pile')
        same(player['life'],world['header']['players'][owner]['life'],'Forge life header')
        same(player['mana'],{long:world['header']['players'][owner]['mana'][short] for short,long in MANA.items()},'Forge mana header')
    return validate_world(dict(turn=world['header']['individual_turn'], step=world['header']['step'],active=world['header']['active'],priority=None if world['header']['game_over'] else world['header']['raw_priority'], players=players, cards=cards, stack=stack), aliases)


def native_world(world: dict[str, Any], header: dict[str, Any], aliases: set[str]) -> dict[str, Any]:
    same({card['alias'] for card in world['cards'].values()}, aliases, 'native full physical inventory')
    require(not world['tokens'], 'unexpected native tokens')
    players = {owner: dict(life=header['life'][seat], mana=header['mana'][seat], library=[], hand=[], battlefield=[], graveyard=[], exile=[]) for seat, owner in ROLES.items()}
    cards = {}
    for card in world['cards'].values():
        obj, creature = card['object'], card['creature']
        alias, zone, owner = card['alias'], obj['zone']['kind'], role(obj['owner'])
        if zone in ['hand', 'battlefield', 'exile']:
            players[owner][zone].append(alias)
        counters = card['counters']
        cards[alias] = dict(owner=owner, controller=role(obj['controller']) if obj['controller'] is not None else None, zone=zone, tapped=obj['tapped'], power=creature['power'] if creature else None, toughness=creature['toughness'] if creature else None, marked_damage=creature['marked_damage'] if creature else None, sick=not obj['continuous_control_since_turn_start'] if creature else None, plus_one_counters=counters['plus_one'] if creature else None, minus_one_counters=counters['minus_one'] if creature else None)
    for pile in world['ordered_piles']:
        players[role(pile['pile']['owner'])][pile['pile']['kind']] = [ref['alias'] for ref in pile['objects_top_to_bottom']]
    for player in players.values():
        for zone in ['hand', 'battlefield', 'exile']:
            player[zone].sort()
    position = header['turn']['position'] if header['turn'] else None
    stack = [row['kind']['card']['alias'] for row in header['stack']]
    for seat, owner in ROLES.items():
        for zone in ['library','hand','battlefield','graveyard','exile']:
            same(len(players[owner][zone]),header['counts_by_owner'][seat][zone],'native count vs physical pile')
    return validate_world(dict(turn=position['number'] if position else 0, step=position['step'] if position else None,active=role(header['active']),priority=role(header['priority_holder']) if header['priority_holder'] is not None else None, players=players, cards=cards, stack=stack), aliases)


def validate_streams(raw: dict[str, Any]) -> None:
    """Validate complete strict callback nesting and the unbroken global raw ledger."""
    all_rows = []
    for stream in ['callbacks', 'events', 'random', 'operations', 'failures']:
        rows = raw[stream]
        same([row['ordinal'] for row in rows], list(range(len(rows))), f'{stream} ordinals')
        require(all(row['case_id'] == raw['case_id'] for row in rows), 'foreign raw case')
        all_rows.extend(rows)
    same(sorted(row['global_sequence'] for row in all_rows), list(range(1, len(all_rows) + 1)), 'raw missing/extra/duplicate global sequence')
    stack = []
    seen = set()
    for row in raw['callbacks']:
        if row['kind'] == 'entry':
            require(row['callback_id'] not in seen, 'duplicate callback identity')
            seen.add(row['callback_id'])
            same(row['parent_callback_id'], stack[-1]['callback_id'] if stack else None, 'callback parent')
            stack.append(row)
        elif row['kind'] == 'return':
            require(bool(stack), 'orphan callback return')
            parent = stack.pop()
            for key in ['callback_id', 'method', 'actor']:
                same(row[key], parent[key], 'callback matching ' + key)
        else:
            raise EvidenceError('unexpected callback failure/kind')
    require(not stack and not raw['failures'], 'unfinished callback or sticky failure')
    completion = raw['completion']
    for key in ['natural_start_game_return', 'game_over', 'game_finished_event']:
        same(completion[key], True, key)
    for prefix in ['actions', 'decisions', 'opening', 'random_operations']:
        same(completion[prefix + '_consumed'], completion[prefix + '_planned'], prefix + ' leftover')
    same(completion['sticky_failure_count'], 0, 'sticky failure count')
    same(completion['callback_depth'], 0, 'callback depth')


def forge_projection(raw: dict[str, Any], spec: dict[str, Any], manifest: dict[str, Any]) -> dict[str, Any]:
    same(raw['schema'], 'teaching-game-forge-v1', 'Forge schema')
    same(raw['forge_commit'], FORGE_PIN, 'Forge pin')
    same(raw['case_id'], spec['id'], 'Forge case')
    validate_streams(raw)
    aliases = set(spec['inventory'])
    same(set(raw['physical_aliases']), aliases, 'registered Forge aliases')
    same(len(set(raw['physical_aliases'].values())), 120, 'unique physical Forge IDs')
    same(set(raw['registered_players']),{'W','G'},'registered player roles')
    same(len({v['player_id'] for v in raw['registered_players'].values()}),2,'distinct registered player IDs')
    for checkpoint in raw['checkpoints'].values():
        same(set(checkpoint['cards']),aliases,'complete raw checkpoint inventory')
        for alias,card in checkpoint['cards'].items():
            same(card['physical_id'],str(raw['physical_aliases'][alias]),'physical alias binding across actual checkpoints')
        header=checkpoint['header']
        if header['individual_turn']>0:
            active=spec['starting_role'] if header['individual_turn']%2 else ('G' if spec['starting_role']=='W' else 'W')
            same(header['active'],active,'actual checkpoint active role')
    def check_views(value: Any) -> None:
        if isinstance(value,dict):
            if 'seat' in value and 'player_id' in value:
                require(value['seat'] in raw['registered_players'],'unknown actual registered role view')
                same(value['player_id'],raw['registered_players'][value['seat']]['player_id'],'actual registered player ID binding')
            if 'alias' in value and 'physical_id' in value:
                require(value['alias'] in raw['physical_aliases'],'unknown raw physical view alias')
                same(value['physical_id'],str(raw['physical_aliases'][value['alias']]),'raw view physical alias binding')
            for child in value.values():
                check_views(child)
        elif isinstance(value,list):
            for child in value:
                check_views(child)
    check_views(raw['callbacks']);check_views(raw['events']);check_views(raw['random']);check_views(raw['operations'])
    observed = unique(raw['observations'], 'key', 'Forge observation')
    require(set(raw['paper_cards']) == {card['name'] for card in manifest['cards']}, 'missing/extra selected Forge identity')
    for card in manifest['cards']:
        printing, face = raw['paper_cards'][card['name']], card['faces'][0]
        for key in ['oracle_text', 'printed_power', 'printed_toughness', 'mana_cost', 'type']:
            field = {'printed_power': 'power', 'printed_toughness': 'toughness', 'type': 'type_line'}.get(key, key)
            expected = face[field]
            if key == 'type':
                expected = expected.replace('\u2014', '-')
            if key == 'mana_cost' and not expected:
                expected = 'no cost'
            same(printing[key], expected, f'loaded {card["name"]} {key}')
        same((printing['set'],printing['collector_number']),PRINTINGS[card['name']],'exact chosen Forge printing')
        same(printing['foil'],False,'nonfoil selected printing')
        same(printing['functional_variant'],'','ordinary functional variant')
        land=face['type_line'].startswith('Basic Land')
        same(printing['color_mask'],0 if land else 1 if '{W}' in face['mana_cost'] else 16,'saved source colors')
        same(printing['core_types'],['Land'] if land else ['Creature'],'saved source core types')
        same(printing['supertypes'],['Basic'] if land else [],'saved source supertypes')
        same(printing['subtypes'],sorted(face['type_line'].split('\u2014 ')[1].split()),'saved source subtypes')
        same(printing['oracle_id'], card['oracleId'], 'printing oracle')
        same(printing['source_receipt']['saved_card'], card, 'source semantic receipt')
    def world(key: str) -> dict[str, Any]:
        require(key in observed, f'missing Forge cut {key}')
        return forge_world(raw['checkpoints'][observed[key]['checkpoint_ref']], aliases)
    cuts = {'opening': world('opening.complete'), 'terminal': world('terminal')}
    for index, action in enumerate(spec['actions']):
        cut = world('action.' + str(index))
        same(cut['cards'][action['card']]['zone'], {'play_land': 'battlefield', 'activate_intrinsic_mana': 'battlefield', 'cast_creature': 'stack'}[action['kind']], 'ordinary action zone')
        cuts['action.' + str(index)] = cut
        if action['kind'] == 'cast_creature':
            cuts['resolve.' + action['card']] = world('resolve.' + action['card'])
    for turn in range(1, 14):
        for step in ['upkeep', 'beginning_combat'] + (['postcombat_main'] if turn < 13 else []):
            cuts[f'phase.{turn}.{step}'] = world(f'priority.{turn}.{step}')
    callbacks = [row for row in raw['callbacks'] if row['kind'] == 'return']
    entries = {row['callback_id']: row for row in raw['callbacks'] if row['kind'] == 'entry'}
    expected_observations={'setup.predraw','opening.complete','terminal','terminal.return'}
    expected_observations.update('action.'+str(i) for i in range(len(spec['actions'])))
    expected_observations.update('resolve.'+action['card'] for action in spec['actions'] if action['kind']=='cast_creature')
    for row in entries.values():
        turn=row['context']['individual_turn']
        if row['method']=='chooseSpellAbilityToPlay':
            expected_observations.add(f"priority.{turn}.{row['context']['step']}")
        if row['method']=='tuckCardsViaMulligan':
            expected_observations.add('mulligan.bottom.before')
        if row['method']=='orderMoveToZoneList':
            expected_observations.add('death-order.'+str(turn))
        if row['method']=='chooseCardsToDiscardToMaximumHandSize':
            expected_observations.add('discard-before.'+str(turn))
    for row in raw['events']:
        turn=row['context']['individual_turn']
        if row['kind']=='GameEventMulligan':
            expected_observations.add('mulligan.bottom.after')
        if row['kind']=='GameEventAttackersDeclared':
            expected_observations.add('attack-event.'+str(turn))
        if row['kind']=='GameEventBlockersDeclared':
            expected_observations.add('block-event.'+str(turn))
        if row['kind']=='GameEventPlayerDamaged' and turn==13:
            expected_observations.add('damage.'+str((turn+1)//2))
    same(set(observed),expected_observations,'Forge exact observation selector inventory')
    same(set(raw['checkpoints']),{row['checkpoint_ref'] for row in observed.values() if row['checkpoint_ref'] is not None},'Forge exact full checkpoint inventory')
    known_methods = {'applyManaToCost','assignCombatDamage','autoPassCancel','awaitNextInput','cheatShuffle','chooseCardsToDiscardToMaximumHandSize','chooseSpellAbilityToPlay','chooseStartingPlayer','complainCardsCantPlayWell','declareAttackers','declareBlockers','getAbilityToPlay','getCostDecisionMaker','mulliganKeepHand','orderMoveToZoneList','payManaCost','playChosenSpellAbility','resetAtEndOfTurn','revealUnsupported','tuckCardsViaMulligan','visit_CostPartMana','visit_CostTap'}
    require(all(row['method'] in known_methods for row in callbacks), 'unknown callback method')
    actual_opening = sorted((dict(role=row['actor'],taken=entries[row['callback_id']]['arguments']['charged'],choice='keep' if row['reply']['keep'] else 'mulligan') for row in callbacks if row['method']=='mulliganKeepHand'),key=lambda row:(row['role'],row['taken']))
    same(actual_opening, sorted(spec['opening_declarations'],key=lambda row:(row['role'],row['taken'])), 'actual opening declaration/charge recipe')
    actual_bottoms = [dict(role=row['actor'],selected_top_first=row['reply']['selected']) for row in callbacks if row['method']=='tuckCardsViaMulligan']
    same(actual_bottoms,[{k:row[k] for k in ['role','selected_top_first']} for row in spec.get('london_bottom',[])],'actual London bottom choices')
    actual_shuffles = [row for row in raw['random'] if row['kind']=='operation_commit']
    expected_shuffles = [(owner,spec['shuffle_target_orders_top_first'][owner]) for owner in ['W','G']] + [(row['role'],row['target_order_top_first']) for row in spec.get('mulligan_shuffles',[])]
    same([(row['callback_owner'],row['actual_committed']) for row in actual_shuffles],expected_shuffles,'real committed shuffle target permutations')
    source_faces={row['name']:row['faces'][0] for row in manifest['cards']}
    for entry in entries.values():
        if entry['method']=='payManaCost':
            ability=entry['arguments']['ability']
            same(entry['arguments']['payer'],entry['actor'],'actual mana-cost payer')
            same(spec['inventory'][ability['source']]['role'],entry['actor'],'actual mana-cost source owner')
            name=spec['inventory'][ability['source']]['name']
            same(entry['arguments']['cost'],'{0}' if ability['mana_ability'] else source_faces[name]['mana_cost'],'every actual mana-cost callback')
    played=[row for row in callbacks if row['method']=='playChosenSpellAbility']
    same(len(played),len(spec['actions']),'actual play callback count')
    for actual,expected in zip(played,spec['actions']):
        same(actual['reply']['policy']['card'],expected['card'],'actual ordinary callback alias')
        same(actual['reply']['result'],True,'actual play accepted')
    operation_returns=[row for row in raw['operations'] if row['kind']=='ordinary_return']
    same(len(operation_returns),len(spec['actions']),'actual ordinary operation count')
    production_receipts={}
    pending_mana={};last_turn=None
    expected_paid_sources={}
    for index,(callback,operation,expected) in enumerate(zip(played,operation_returns,spec['actions'])):
        policy=callback['reply']['policy']
        for key,value in expected.items():
            actual=policy[key]
            if key=='kind':
                value={'activate_intrinsic_mana':'activate_mana','cast_creature':'cast_untargeted'}.get(value,value)
            same(actual,value,'exact frozen ordinary callback policy '+key)
        same(operation['policy'],policy,'ordinary operation/callback policy')
        same(operation['callback_return_sequence'],callback['global_sequence'],'ordinary operation/callback sequence')
        same(operation['operation_id'],callback['operation_id'],'ordinary operation identity')
        action_observation=observed['action.'+str(index)]
        action_world=raw['checkpoints'][action_observation['checkpoint_ref']]
        same(action_observation['source_global_sequence'],callback['global_sequence'],'action observation actual callback link')
        same(action_world['header'],callback['context'],'action world actual callback context')
        same(operation['context'],callback['context'],'operation actual callback context')
        same(policy['alias'],expected['card'],'exact adapter action alias')
        same(policy['seat'],expected['role'],'exact adapter action seat')
        same(policy['personal_turn'],expected['own_turn'],'exact adapter action own turn')
        same(policy['individual_turn'],2*expected['own_turn']-(1 if expected['role']==spec['starting_role'] else 0),'exact adapter action individual turn')
        same(callback['context']['individual_turn'],policy['individual_turn'],'ordinary actual callback turn')
        same(callback['context']['step'],expected['step'],'ordinary actual callback step')
        same(callback['context']['active'],expected['role'],'ordinary actual callback active role')
        same(callback['context']['raw_priority'],expected['role'],'ordinary actual callback priority holder')

        same(callback['actor'],expected['role'],'ordinary callback actual actor')
        same(callback['reply']['actual_ability']['source'],expected['card'],'ordinary actual source')
        turn=operation['context']['individual_turn']
        if turn!=last_turn:
            pending_mana={};last_turn=turn
        if expected['kind']=='activate_intrinsic_mana':
            source=expected['card'];pending_mana[source]=True
            card=raw['checkpoints'][observed['action.'+str(index)]['checkpoint_ref']]['cards'][source]
            same(card['tapped'],True,'actual produced mana source tapped')
            production_receipts[operation['operation_id']]=dict(source=source,color='white' if expected['role']=='W' else 'green',production_operation_id=operation['operation_id'],source_timestamp=card['timestamp'],output_index=0)
        if expected['kind']=='cast_creature':
            count=int(expected['mana_units_paid'])
            sources=list(pending_mana)[:count]
            same(len(sources),count,'enough actual named mana sources')
            same(policy['paid_sources'],sources,'frozen named debit sources')
            expected_paid_sources[expected['card']]=sources
            for source in sources:
                del pending_mana[source]
    declarations = []
    for recipe in spec['combat']:
        turn = 2 * recipe['own_turn'] - (1 if recipe['active_role'] == spec['starting_role'] else 0)
        matching = [row for row in callbacks if row['method'] == 'declareAttackers' and row['context']['individual_turn'] == turn]
        require(len(matching) == 1, 'missing/duplicate selected attack callback')
        callback = matching[0]
        same(callback['reply']['selected'], recipe['attackers'], 'attack recipe')
        declarations.append(dict(turn=turn, actor=recipe['active_role'], attackers=sorted(callback['reply']['selected']), eligible=sorted(entries[callback['callback_id']]['arguments']['eligible']), blockers=sorted(recipe['blockers'], key=lambda b: b['blocker'])))
        if recipe['blockers']:
            block = [row for row in callbacks if row['method'] == 'declareBlockers' and row['context']['individual_turn'] == turn]
            require(len(block) == 1, 'missing/duplicate selected blocker callback')
            same(block[0]['reply']['selected'], recipe['blockers'], 'block recipe')
    damage = []
    for row in raw['events']:
        if row['kind'] in ['GameEventCardDamaged', 'GameEventPlayerDamaged']:
            payload = row['payload']
            if int(payload['amount']) > 0:
                damage.append(dict(turn=row['context']['individual_turn'], source=payload['source']['alias'], target=payload['card']['alias'] if row['kind'] == 'GameEventCardDamaged' else payload['target'], amount=payload['amount']))
    attack_frames=[];block_frames=[];discards=[];death_orders=[]
    for row in callbacks:
        entry=entries[row['callback_id']]
        turn=row['context']['individual_turn']
        if row['method']=='declareAttackers':
            attack_frames.append(dict(turn=turn,actor=row['actor'],eligible=sorted(entry['arguments']['eligible']),selected=sorted(row['reply']['selected'])))
        if row['method']=='declareBlockers':
            block_frames.append(dict(turn=turn,actor=row['actor'],eligible=sorted(entry['arguments']['available_creatures']),selected=sorted(row['reply']['selected'],key=lambda b:b['blocker'])))
        if row['method']=='chooseCardsToDiscardToMaximumHandSize':
            discards.append(dict(turn=turn,actor=row['actor'],domain=sorted(entry['arguments']['domain']),selected=row['reply']['selected_top_to_bottom']))
        if row['method']=='orderMoveToZoneList':
            death_orders.append(dict(turn=turn,actor=row['actor'],domain=sorted(entry['arguments']['offered_sequence']),selected=row['reply']['selected_top_to_bottom']))
        if row['method']=='assignCombatDamage':
            args=entry['arguments'];assignments=row['reply']['assignments']
            same(len(assignments),1,'single-recipient damage callback')
            same(args['recipients'],[assignments[0]['recipient']],'damage callback actual recipient')
            same(args['amount'],assignments[0]['amount'],'damage callback assigned quantity')
            expected_damage=sum(int(d['amount']) for d in damage if d['turn']==turn and d['source']==args['source'] and d['target']==assignments[0]['recipient'])
            same(expected_damage,int(args['amount']),'actual assignment and dealt quantity')
    payments = []
    for row in callbacks:
        if row['method'] == 'applyManaToCost':
            entry = entries[row['callback_id']]
            parent=entries[entry['parent_callback_id']]
            require(parent['method']=='payManaCost','actual payment parent')
            same(entry['arguments']['payer'],entry['actor'],'actual applied mana payer')
            same(parent['actor'],entry['actor'],'actual payment actor nesting')
            same(parent['arguments']['payer'],entry['actor'],'actual payment parent payer')
            same(row['reply']['result'],True,'actual applied mana result')
            same(row['reply']['paid'],True,'actual paid cost completion')
            source=entry['arguments']['ability']['source']
            same(spec['inventory'][source]['role'],entry['actor'],'actual cast source owner')
            same([spent['token']['source'] for spent in row['reply']['spent']],expected_paid_sources[source],'actual named mana debit source sequence')
            same(row['reply']['paying_mana'],[spent['token'] for spent in row['reply']['spent']],'actual recorded paying mana')
            for spent in row['reply']['spent']:
                token=spent['token'];operation_id=token['production_operation_id']
                require(operation_id in production_receipts,'missing actual mana production operation')
                same(token,production_receipts[operation_id],'actual mana token production provenance')
                operation=next(op for op in operation_returns if op['operation_id']==operation_id)
                require(operation['global_sequence']<entry['global_sequence'],'mana debit follows actual production')
            same(row['reply']['spent'][-1]['unpaid_after'],'0','actual mana cost entirely discharged')
            payments.append(dict(card=entry['arguments']['ability']['source'], cost=cost_tuple(parent['arguments']['cost']), spent=[item['token']['color'] for item in row['reply']['spent']]))
    outcome = raw['actual_outcome']
    same(outcome['draw'],False,'natural Forge outcome cannot be draw')
    same(outcome['end_reason'],'AllOpponentsLost','natural Forge end reason')
    same(set(outcome['players']),{'W','G'},'natural outcome participant set')
    same(outcome['winning_seat'], spec['expected_terminal']['winner_role'], 'Forge outcome')
    terminal_raw=raw['checkpoints'][observed['terminal']['checkpoint_ref']]
    for owner,result in outcome['players'].items():
        won=owner==outcome['winning_seat']
        for field in ['won','game_outcome_is_winner']:
            same(result[field],won,'natural actual winner consistency '+field)
        same(result['lost'],not won,'natural actual loser consistency')
        same(result['loss_reason'],None if won else 'LifeReachedZero','natural participant loss reason')
        same(result['alternative_win_source'],None,'no alternative win source')
        same(result['lose_condition_spell'],None,'no synthetic lose-condition spell')
        for field in ['won','lost','loss_reason']:
            same(terminal_raw['players'][owner][field],result[field],'terminal/outcome player consistency')
    outcome_events=[row for row in raw['events'] if row['kind']=='GameEventGameOutcome']
    finished_events=[row for row in raw['events'] if row['kind']=='GameEventGameFinished']
    same(len(outcome_events),1,'one actual natural outcome event')
    same(len(finished_events),1,'one actual finished event')
    require(outcome_events[0]['global_sequence']<finished_events[0]['global_sequence'],'outcome precedes finish')
    require(not any(row['method']=='chooseSpellAbilityToPlay' and row['global_sequence']>outcome_events[0]['global_sequence'] for row in raw['callbacks']),'no priority after natural outcome')
    loser = spec['expected_terminal']['loser_role']
    same(outcome['players'][loser]['loss_reason'], 'LifeReachedZero', 'Forge natural loss')
    return dict(cuts=cuts, declarations=declarations, attack_frames=attack_frames,block_frames=block_frames,discards=discards,death_orders=death_orders,damage=sorted(damage, key=lambda v: (v['turn'], v['source'], v['target'])), payments=payments, winner=outcome['winning_seat'])


def native_projection(run: dict[str, Any], spec: dict[str, Any]) -> dict[str, Any]:
    same(run['fixture_ref'], spec['id'], 'native case')
    same(run['completed'], True, 'native completed')
    same(run['failure'], None, 'native failure')
    same([row['ordinal'] for row in run['accepted']], list(range(1, len(run['accepted']) + 1)), 'native accepted ordinals')
    for key in ['initialization_equal', 'final_state_equal', 'all_inputs_consumed']:
        same(run['rerun'][key], True, 'native rerun ' + key)
    same(run['final']['policy_complete'], True, 'native policy completion')
    same(run['final']['entropy_remaining'], 0, 'native entropy consumed')
    same(run['final']['policy_entries_consumed'], run['final']['policy_entries_total'], 'native recipe consumed')
    same(run['final']['accepted_inputs'],len(run['accepted']),'native accepted count')
    same(run['final']['final_ordinal'],len(run['accepted']),'native final ordinal')
    same(run['rerun']['compared_ordinals'],[row['ordinal'] for row in run['accepted']],'native exact replay input coverage')
    same(set(run['rerun']['checkpoint_ordinals_compared']),{int(key) for key in run['checkpoints']},'native exact replay full checkpoint coverage')
    probe_rows=unique(run['rejection_probes'],'probe_id','native rejection probe')
    same(set(run['rerun']['probe_ids_compared']),set(probe_rows),'native rerun rejected probes')
    families=set()
    for probe in probe_rows.values():
        same(probe['no_candidate'],True,'native rejected candidate absent')
        same(probe['state_retained'],True,'native rejection state atomicity')
        same(probe['actual_error'],probe['expected_error'],'native rejection error')
        families.add(probe['expectation_ref'])
    require(({'stale','foreign','insufficient-payment','second-land','young-attacker'} if spec['id']=='RD01' else {'stale','foreign','insufficient-payment','second-land'})<=families,'native required reached-state rejection coverage')
    aliases = set(spec['inventory'])
    checkpoints = run['checkpoints']
    old_world = copy.deepcopy(checkpoints['0'])
    current_cards = old_world['cards']
    piles = old_world['ordered_piles']
    snapshots = {}
    rows = [run['initialization']] + run['accepted']
    objects = {}
    for row in rows:
        for change in row.get('card_changes', []):
            same(current_cards[change['card']], change['before'], 'native delta before')
            current_cards[change['card']] = change['after']
        for change in row.get('pile_changes', []):
            matches = [i for i, pile in enumerate(piles) if pile['pile'] == change['pile']]
            require(len(matches) == 1, 'native pile domain')
            piles[matches[0]] = change
        for card in current_cards.values():
            objects[card['object']['object']] = card['alias']
        reconstructed = dict(cards=current_cards, tokens=old_world['tokens'], ordered_piles=piles)
        if str(row['ordinal']) in checkpoints:
            actual = checkpoints[str(row['ordinal'])]
            for key in ['cards', 'tokens', 'ordered_piles']:
                same(actual[key], reconstructed[key], 'native full checkpoint vs deltas ' + key)
        snapshots[row['ordinal']] = native_world(reconstructed, row['header'], aliases)
    cuts = {'terminal': snapshots[run['final']['final_ordinal']]}
    selected = []
    payments = []
    damage = []
    declarations = []
    actual_blocks = {}
    attack_frames=[];block_frames=[];discards=[];death_orders=[]
    actual_opening = []
    actual_bottoms = []
    last_header = run['initialization']['header']
    for row in run['accepted']:
        header, command, snapshot = row['header'], row['input']['command'], snapshots[row['ordinal']]
        if 'opening' not in cuts and header['opening']['complete'] and snapshot['step'] == 'upkeep' and header['priority_holder'] == header['active']:
            cuts['opening'] = snapshot
        if command['kind'] in ['play_land', 'activate_mana', 'cast_untargeted']:
            index = len(selected)
            require(index < len(spec['actions']), 'extra native ordinary action')
            expected = spec['actions'][index]
            same(command['kind'], {'play_land': 'play_land', 'activate_intrinsic_mana': 'activate_mana', 'cast_creature': 'cast_untargeted'}[expected['kind']], 'native action kind')
            field = {'play_land': 'land', 'activate_mana': 'source', 'cast_untargeted': 'spell'}[command['kind']]
            same(command[field]['alias'], expected['card'], 'native action alias')
            same(role(row['input']['actor']), expected['role'], 'native action role')
            same(snapshot['turn'], 2 * expected['own_turn'] - (1 if expected['role'] == spec['starting_role'] else 0), 'native action turn')
            cuts['action.' + str(index)] = snapshot
            selected.append(expected)
            if command['kind'] == 'cast_untargeted':
                payment = row['current_facts']['mana_payment']
                payments.append(dict(card=expected['card'], cost=payment['cost'], spent=[spend['mana_type'] for spend in payment['spend'] for _ in range(int(spend['amount']))]))
        permanent = row['current_facts']['permanent_entry']
        if permanent:
            alias = permanent['after']['alias']
            cuts['resolve.' + alias] = snapshot
        if snapshot['step'] in ['upkeep', 'beginning_combat', 'postcombat_main'] and header['priority_holder'] == header['active'] and not header['pending']:
            key = f"phase.{snapshot['turn']}.{snapshot['step']}"
            cuts.setdefault(key, snapshot)
        for applied in row['current_facts']['damage_journal']['applied']:
            if int(applied['amount']) > 0:
                source = objects[applied['source']['object']['object']]
                target = applied['target']
                damage.append(dict(turn=snapshot['turn'], source=source, target=role(target['player']) if target['kind'] == 'player' else objects[target['object']], amount=applied['amount']))
        if command['kind'] == 'reply' and command['choice']['kind'] == 'declare_attackers':
            recipe = next((r for r in spec['combat'] if 2 * r['own_turn'] - (1 if r['active_role'] == spec['starting_role'] else 0) == snapshot['turn']), None)
            if recipe:
                attackers = command['choice']['attackers']
                domain = last_header['pending']['frame']['frame']['eligible_attackers']
                declarations.append(dict(turn=snapshot['turn'], actor=role(row['input']['actor']), attackers=sorted(v['attacker']['alias'] for v in attackers), eligible=sorted(v['alias'] for v in domain), blockers=sorted(recipe['blockers'], key=lambda b: b['blocker'])))
        if command['kind']=='reply':
            choice=command['choice']
            frame=last_header['pending']['frame']['frame']
            if choice['kind']=='declare_attackers':
                attack_frames.append(dict(turn=snapshot['turn'],actor=role(row['input']['actor']),eligible=sorted(ref['alias'] for ref in frame['eligible_attackers']),selected=sorted(v['attacker']['alias'] for v in choice['attackers'])))
            if choice['kind']=='declare_blockers':
                block_frames.append(dict(turn=snapshot['turn'],actor=role(row['input']['actor']),eligible=sorted(ref['alias'] for ref in frame['eligible_blockers']),selected=sorted([dict(blocker=b['blocker']['alias'],attacker=b['attacker']['alias']) for b in choice['blockers']],key=lambda b:b['blocker'])))
            if choice['kind']=='discard':
                discards.append(dict(turn=last_header['turn']['position']['number'],actor=role(row['input']['actor']),domain=sorted(ref['alias'] for ref in frame['domain']),selected=[ref['alias'] for ref in choice['ordered']]))
            if choice['kind']=='order_graveyard':
                death_orders.append(dict(turn=snapshot['turn'],actor=role(row['input']['actor']),domain=sorted(ref['alias'] for ref in frame['domain']),selected=[ref['alias'] for ref in choice['ordered']]))
            if choice['kind']=='declare_blockers':
                require(snapshot['turn'] not in actual_blocks,'duplicate native blocker choice')
                actual_blocks[snapshot['turn']]=[dict(blocker=b['blocker']['alias'],attacker=b['attacker']['alias']) for b in choice['blockers']]
            if choice['kind']=='mulligan':
                actual_opening.append(dict(role=role(row['input']['actor']),taken=last_header['pending']['frame']['frame']['taken'],choice=choice['choice']))
            if choice['kind']=='bottom':
                actual_bottoms.append(dict(role=role(row['input']['actor']),selected_top_first=[ref['alias'] for ref in choice['ordered']]))
        last_header = header
    same(sorted(actual_opening,key=lambda row:(row['role'],row['taken'])), sorted(spec['opening_declarations'],key=lambda row:(row['role'],row['taken'])),'native actual opening choices')
    same(actual_bottoms,[{k:row[k] for k in ['role','selected_top_first']} for row in spec.get('london_bottom',[])],'native actual bottom choices')
    for declaration in declarations:
        observed=actual_blocks.get(declaration['turn'],[])
        same(sorted(observed,key=lambda b:b['blocker']),declaration['blockers'],'native actual blockers')
        declaration['blockers']=sorted(observed,key=lambda b:b['blocker'])
    same(len(selected), len(spec['actions']), 'all native actions')
    required_cuts = {'opening', 'terminal'} | {'action.' + str(i) for i in range(len(spec['actions']))} | {'resolve.' + a['card'] for a in spec['actions'] if a['kind'] == 'cast_creature'} | {f'phase.{turn}.{step}' for turn in range(1, 14) for step in ['upkeep', 'beginning_combat'] + (['postcombat_main'] if turn < 13 else [])}
    same(set(cuts), required_cuts, 'native cut coverage')
    return dict(cuts=cuts, declarations=declarations, attack_frames=attack_frames,block_frames=block_frames,discards=discards,death_orders=death_orders,damage=sorted(damage, key=lambda v: (v['turn'], v['source'], v['target'])), payments=payments, winner=role(run['final']['outcome']['seat']))


def pointer(document: Any, path: str) -> Any:
    require(path.startswith('/'),'invalid capture pointer')
    current=document
    try:
        for token in path[1:].split('/'):
            token=token.replace('~1','/').replace('~0','~')
            current=current[int(token)] if isinstance(current,list) else current[token]
    except (KeyError,IndexError,TypeError,ValueError) as error:
        raise EvidenceError('unresolved capture pointer '+path) from error
    return current


def validate_registry_manifest(registry: dict[str, Any], manifest: dict[str, Any]) -> None:
    """Bind the actual witness to all frozen identities and signatures independently."""
    same(registry['sourceSha256'],manifest['provenance']['compressedSha256'],'registry frozen catalogue bytes')
    same(registry['creatureVocabularyId'],manifest['provenance']['creatureVocabularyId'],'registry actual selected vocabulary')
    same(registry['sourceRecords'],CATALOGUE_RECORDS,'registry complete pinned catalogue record count')
    for key,value in dict(requiredRecords=20,requiredPartialBindings=20,requiredWithoutBinding=0,fullyQualifiedCards=0,registryVersion=5).items():
        same(registry[key],value,'registry selected finite count '+key)
    same(registry['behaviorEvidence'],'not-evaluated-by-registry','conservative registry qualification boundary')
    cards=unique(registry['cards'],'oracleId','registry Oracle identity')
    same(set(cards),{card['oracleId'] for card in manifest['cards']},'registry exact selected identity set')
    for expected in manifest['cards']:
        actual=cards[expected['oracleId']]
        same(actual['name'],expected['name'],'registry frozen name')
        same(actual['required'],True,'registry selected required card')
        same(actual['behaviorEvidence'],'not-evaluated-by-registry','registry card evidence boundary')
        same(actual['binding'],dict(status='partial',program='teaching-basic-land-v1' if expected['faces'][0]['type_line'].startswith('Basic Land') else 'teaching-vanilla-creature-v1',scope='teaching-ordinary-card-v1',semanticSha256=expected['registrySignatureV1Sha256']),'registry frozen semantic signature and per-identity authored key/scope')


def unwrap_native(document: dict[str, Any], allow_uncaptured: bool, manifest: dict[str, Any] | None = None) -> tuple[dict[str, Any], dict[str, Any]]:
    """Accept a complete teaching capture or explicitly marked preliminary output."""
    require('capture_failure' not in document,'native capture_failure')
    if document.get('profile')=='teaching':
        same(document['schema_version'],9,'native current schema')
        require('type_context_witness' in document,'missing native witness')
        witness=document['type_context_witness']
        same(witness['schema'],'spellwake.current-type-context-witness.v7','native witness schema')
        same(witness['registry']['tag'],'Registry','actual teaching Registry witness required')
        registry=witness['registry']['coverage']
        require(manifest is not None,'frozen manifest required for actual Registry witness')
        validate_registry_manifest(registry,manifest)
        same(registry['registryVersion'],5,'witness registry version')
        same(registry['sourceSha256'],'c1c9dc28fa9a7a4bae61c02330ba5a1190205aab1a93361e590ff5d9a146af08','witness actual catalogue hash')
        coverage=witness['coverage']
        same(coverage['complete'],True,'native capture complete')
        same(coverage['uncovered'],[],'native uncovered capture')
        points=unique(witness['snapshots'],'point_id','capture point')
        calls=unique(witness['calls'],'call_id','capture call')
        same(set(coverage['required_points']),set(points),'native required point inventory')
        same(len(coverage['required_points']),len(points),'duplicate required points')
        same(set(coverage['required_calls']),set(calls),'native required call inventory')
        same(len(coverage['required_calls']),len(calls),'duplicate required calls')
        joins=unique(coverage['observation_to_points'],'pointer','capture observation pointer')
        for path,join in joins.items():
            pointer(document,path)
            for point_id in join['points']:
                require(point_id in points and path in points[point_id]['observation_pointers'],'capture point observation join')
        for point_id,point in points.items():
            require(bool(point['observation_pointers']),'unbound native point')
            for path in point['observation_pointers']:
                require(path in joins and point_id in joins[path]['points'],'uncovered native observation')
            if point['projection']['tag']=='RecordPointer':
                pointer(document,point['projection']['pointer'])
        for path in coverage['record_pointers']:
            pointer(document,path)
        source_inputs=witness['source_inputs']
        unique(source_inputs,'path','native source input')
        for source in source_inputs:
            require(bool(source['path']) and re.fullmatch('[0-9a-f]{64}',source['sha256']) is not None and source['bytes']>0,'native source receipt shape')
        provenance=dict(captured=True,registry=registry,source_inputs=source_inputs,complete=True,points=len(points),calls=len(calls),scope='Complete capture inventory and actual Registry guards checked here; current-profile structural/semantic checks remain independently required.')
        inner=document['observations']
    else:
        require(allow_uncaptured,'uncaptured native report requires explicit preliminary opt-in')
        require('observations' not in document and 'profile' not in document,'unknown native outer profile')
        provenance=dict(captured=False,complete=False,scope='Preliminary adapter join only; no current capture claim.')
        inner=document
    same(inner.get('status'),'success','native success observation required; failure/partial/unsupported rejected')
    require(isinstance(inner.get('data'),dict),'missing native report payload')
    return inner['data'],provenance


def compare(spec_path: Path, manifest_path: Path, forge_dir: Path, native_path: Path, allow_uncaptured: bool = False) -> dict[str, Any]:
    """Compare every selected common fact; never derive expectations from either engine."""
    same(hashlib.sha256(spec_path.read_bytes()).hexdigest(), SPEC_SHA, 'frozen independent case bytes')
    spec, manifest, native = read(spec_path), read(manifest_path), read(native_path)
    native,capture_provenance=unwrap_native(native,allow_uncaptured,manifest)
    same(native['profile'], 'catalogue-teaching-pair-v1', 'native profile')
    same(native['registry_version'], 5, 'native registry')
    same(native['catalogue_sha256'],spec['authority']['saved_oracle_compressed_sha256'],'native actual selected catalogue')
    runs = unique(native['runs'], 'fixture_ref', 'native case')
    same(set(runs), {'RD01', 'RD02'}, 'native exact case set')
    same({path.name for path in forge_dir.glob('RD*.json')},{'RD01.json','RD02.json'},'Forge exact case file set')
    input_path=forge_dir.parent/'inputs.json'
    actual_inputs=read(input_path)
    same(actual_inputs['case_spec_sha256'],SPEC_SHA,'reference case source binding')
    same(actual_inputs['manifest_sha256'],hashlib.sha256(manifest_path.read_bytes()).hexdigest(),'reference manifest binding')
    report = {'schema': 'teaching-common-fact-comparison-v1', 'cases': [],'provenance':dict(native_sha256=hashlib.sha256(native_path.read_bytes()).hexdigest(),case_spec_sha256=SPEC_SHA,manifest_sha256=hashlib.sha256(manifest_path.read_bytes()).hexdigest(),reference_input_sha256=hashlib.sha256(input_path.read_bytes()).hexdigest(),native_capture=capture_provenance,reference_raw_sha256={path.name:hashlib.sha256(path.read_bytes()).hexdigest() for path in sorted(forge_dir.glob('RD*.json'))})}

    for case in spec['cases']:
        case = dict(case, inventory=spec['inventory'])
        raw=read(forge_dir / (case['id'] + '.json'))
        same(raw['inputs_sha256'],report['provenance']['reference_input_sha256'],'raw actual reference input hash')
        same(raw['fixture'],next(row for row in actual_inputs['cases'] if row['case_id']==case['id']),'actual reference fixture receipt')
        for receipt in raw['paper_cards'].values():
            for resource in receipt['source_receipt']['forge_resources']:
                retained=forge_dir.parent/resource['retained']
                same(hashlib.sha256(retained.read_bytes()).hexdigest(),resource['sha256'],'actual retained Forge resource')
        forge = forge_projection(raw, case, manifest)
        static=runs[case['id']]['static_inputs']
        definitions=unique(static['definitions'],'name','native selected definition')
        same(set(definitions),{card['name'] for card in manifest['cards']},'native exact selected20 source identities')
        for card in manifest['cards']:
            definition=definitions[card['name']]
            same(definition['oracle_id'],card['oracleId'],'native saved Oracle identity')
            same(definition['printing_id'],card['scryfallId'],'native actual saved source printing')
            require(any(source['sha256']==spec['authority']['saved_oracle_compressed_sha256'] for source in definition['sources']),'native catalogue source receipt')
        actual_aliases=unique(static['aliases'],'alias','native source alias')
        same(set(actual_aliases),set(spec['inventory']),'native static alias inventory')
        for alias,entry in spec['inventory'].items():
            same(role(actual_aliases[alias]['owner']),entry['role'],'native static owner')
            same(actual_aliases[alias]['definition_ref'],entry['oracle_id'],'native static Oracle binding')
        same(static['setup']['participants'],[41,3],'native canonical participant order')
        same(role(static['setup']['active']),case['starting_role'],'native actual starting role')
        same(static['setup']['mana'],[],'no native seed mana')
        native_facts = native_projection(runs[case['id']], case)
        same(set(forge), set(native_facts), 'selected fact family set')
        for family in forge:
            if family == 'cuts':
                same(set(forge[family]), set(native_facts[family]), 'common cut set')
                for key in forge[family]:
                    same(forge[family][key], native_facts[family][key], case['id'] + ' ' + key)
            else:
                same(forge[family], native_facts[family], case['id'] + ' ' + family)
        assert_frozen_facts(forge, case)
        report['cases'].append(dict(case_id=case['id'], equal=True, selected_cuts=len(forge['cuts']), declarations=len(forge['declarations']), positive_damage_events=len(forge['damage']), paid_casts=len(forge['payments']), independent_expected_observations=len(case['expected_observations']),source_bindings=dict(native_static_inputs_sha256=hashlib.sha256(json.dumps(static,sort_keys=True,separators=(',',':')).encode()).hexdigest(),native_contexts=static['contexts'],native_source_build_ref=runs[case['id']]['source_build_ref'],reference_case_input_sha256=raw['inputs_sha256'],reference_loaded_classes=raw['runtime']['loader_classes'],scope='Source/printing/runtime contracts are retained independently; common semantic equality does not assert identical physical IDs or entropy.'),common_facts=forge))
    report['limits'] = ['Selected privileged semantic cuts and positive damage events; engine-local IDs, raw protocol counts and entropy words remain separate.', 'No common full state before automatic state checks is claimed; selected event quantities and settled deaths are compared.', 'Native capture/authentication status is the caller-provided packet status; this reader does not replace current_capture witness validation.', 'Ten distinct played/cast identities across two trajectories; all20 Forge printing semantics loaded; remaining ordinary native per-card tests are separate.']
    return report


def assert_frozen_facts(facts: dict[str, Any], case: dict[str, Any]) -> None:
    """Assert the source-authored terminal arithmetic in addition to engine equality."""
    cuts=facts['cuts']
    for observation in case['expected_observations']:
        at, expected_facts=observation['at'],observation['facts']
        actor=case['starting_role'] if at=='opening_complete' else at[0]
        own=1 if at=='opening_complete' else int(at[1:].split('.')[0])
        turn=2*own-(1 if actor==case['starting_role'] else 0)
        if at=='opening_complete':
            cut=cuts['opening']
        elif 'after_cast' in at:
            cut=cuts['resolve.'+expected_facts['spell']]
        elif 'cleanup_after' in at:
            cut=cuts[f'phase.{turn+1}.upkeep']
        elif 'before_combat' in at or 'beginning_combat' in at:
            cut=cuts[f'phase.{turn}.beginning_combat']
        else:
            cut=cuts['terminal'] if turn==13 else cuts[f'phase.{turn}.postcombat_main']
        for key,expected_value in expected_facts.items():
            if key=='life':
                actual={owner:cut['players'][owner]['life'] for owner in ['W','G']}
            elif key in ['hand_count','library_count']:
                zone=key.removesuffix('_count')
                actual={owner:len(cut['players'][owner][zone]) for owner in ['W','G']} if isinstance(expected_value,dict) else len(cut['players'][actor][zone])
            elif key=='first_priority':
                actual=dict(role=case['starting_role'],step=cut['step'],individual_turn=cut['turn'])
            elif key=='green_hand':
                actual=cut['players']['G']['hand'];expected_value=sorted(expected_value)
            elif key=='green_library_bottom':
                actual=cut['players']['G']['library'][-len(expected_value):]
            elif key=='spell':
                actual=expected_value if cut['cards'][expected_value]['zone']=='battlefield' else None
            elif key=='cost_paid':
                payment=next((p for p in facts['payments'] if p['card']==expected_facts['spell']),None)
                require(payment is not None,'frozen required payment missing')
                actual=payment['cost'];expected_value=cost_tuple(expected_value)
            elif key in ['source_power','source_toughness']:
                actual=cut['cards'][expected_facts['spell']][key.removeprefix('source_')]
            elif key in ['pool_white','pool_green']:
                color=key.removeprefix('pool_')
                actual=cut['players'][actor]['mana'][color]
            elif key=='tapped_lands':
                actual=sorted(alias for alias in cut['players'][actor]['battlefield'] if cut['cards'][alias]['power'] is None and cut['cards'][alias]['tapped'])
                expected_value=sorted(expected_value)
            elif key in ['ineligible_young','young_attacker_excluded']:
                young=expected_value if isinstance(expected_value,list) else [expected_value]
                for alias in young:
                    same(cut['cards'][alias]['sick'],True,'independent young-creature status')
                actual=expected_value
            elif key=='eligible_attackers':
                declaration=next(d for d in facts['declarations'] if d['turn']==turn)
                actual=declaration['eligible'];expected_value=sorted(expected_value)
            elif key in ['marked_damage','survivor_marked_damage']:
                actual={alias:cut['cards'][alias]['marked_damage'] for alias in expected_value}
            elif key=='graveyard_top_first':
                actual={owner:cut['players'][owner]['graveyard'] for owner in ['W','G']} if isinstance(expected_value,dict) else cut['players'][actor]['graveyard']
            elif key=='battlefield_creatures':
                actual={owner:sorted(alias for alias in cut['players'][owner]['battlefield'] if cut['cards'][alias]['power'] is not None) for owner in ['W','G']}
                expected_value={owner:sorted(value) for owner,value in expected_value.items()}
            elif key in ['damage_to_creatures','damage_to_players']:
                # CR120.8 and510.2: zero power causes no positive damage event.
                # The frozen zero quantities are verified as absence of positive events.
                actual={target:str(sum(int(d['amount']) for d in facts['damage'] if d['turn']==turn and d['target']==target)) for target in expected_value}
            else:
                raise EvidenceError('unknown independent expected fact '+key)
            same(actual,expected_value,case['id']+' frozen '+at+' '+key)
    terminal, expected = facts['cuts']['terminal'], case['expected_terminal']
    same(facts['winner'], expected['winner_role'], 'independent terminal winner')
    same(terminal['turn'], expected['individual_turn'], 'independent terminal turn')
    same(terminal['step'], expected['step'], 'independent terminal step')
    for owner in ['W', 'G']:
        player = terminal['players'][owner]
        same(player['life'], expected['life'][owner], 'independent terminal life')
        same(len(player['library']), expected['library_count'][owner], 'independent terminal library')
        same(player['hand'], sorted(expected['hand_aliases'][owner]), 'independent terminal hand')
        same(player['battlefield'], sorted(expected['battlefield_lands'][owner] + expected['battlefield_creatures'][owner]), 'independent terminal field')
        same(player['graveyard'], expected['graveyard_top_first'][owner], 'independent terminal grave order')
    if 'green_library_bottom' in expected:
        same(terminal['players']['G']['library'][-len(expected['green_library_bottom']):], expected['green_library_bottom'], 'independent bottom')


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--spec', type=Path, required=True)
    parser.add_argument('--manifest', type=Path, required=True)
    parser.add_argument('--forge-dir', type=Path, required=True)
    parser.add_argument('--native', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--allow-uncaptured', action='store_true', help='Preliminary adapter preparation only')
    args = parser.parse_args()
    report = compare(args.spec, args.manifest, args.forge_dir, args.native,args.allow_uncaptured)
    with args.out.open('x') as stream:
        json.dump(report, stream, indent=2)
        stream.write('\n')
    print(json.dumps({'equal': True, 'cases': [{key: value for key, value in case.items() if key not in ['common_facts','source_bindings']} for case in report['cases']]}))


if __name__ == '__main__':
    main()
