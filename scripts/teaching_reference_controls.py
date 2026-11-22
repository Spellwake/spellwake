#!/usr/bin/env python3
"""Run retained mutations against actual teaching evidence and require each rejection."""
from __future__ import annotations

import argparse
import copy
import hashlib
import json
from pathlib import Path
from typing import Any, Callable

from teaching_reference_compare import (
    EvidenceError, assert_frozen_facts, compare, forge_projection,
    native_projection, read, require, same, unique, unwrap_native,
)


def exercise_controls(spec: dict[str, Any], manifest: dict[str, Any], raw: dict[str, Any], native_run: dict[str, Any]) -> list[dict[str, Any]]:
    """Mutate independently selected quantities and protocol fields in real evidence."""
    case = dict(next(c for c in spec['cases'] if c['id'] == 'RD02'), inventory=spec['inventory'])
    baseline_forge = forge_projection(raw, case, manifest)
    baseline_native = native_projection(native_run, case)
    same(baseline_forge, baseline_native, 'control baseline')
    assert_frozen_facts(baseline_forge, case)
    controls: list[dict[str, Any]] = []

    def control(name: str, engine: str, mutate: Callable[[dict[str, Any]], str]) -> None:
        forged = copy.deepcopy(raw if engine == 'forge' else native_run)
        detail = mutate(forged)
        try:
            selected = forge_projection(forged, case, manifest) if engine == 'forge' else native_projection(forged, case)
            same(selected, baseline_native if engine == 'forge' else baseline_forge, 'mutated selected common facts')
            assert_frozen_facts(selected, case)
        except EvidenceError as error:
            controls.append(dict(name=name, engine=engine, mutation=detail, rejected=True, reason=str(error), mutated_sha256=hashlib.sha256(json.dumps(forged, sort_keys=True, separators=(',', ':')).encode()).hexdigest()))
        else:
            raise EvidenceError('ineffective mutation control: ' + name)

    def checkpoint(raw_value: dict[str, Any], selector: str) -> dict[str, Any]:
        observation = next(row for row in raw_value['observations'] if row['key'] == selector)
        return raw_value['checkpoints'][observation['checkpoint_ref']]

    def mutate_cut(selector: str, field: str, value: Any) -> Callable[[dict[str, Any]], str]:
        def apply(document: dict[str, Any]) -> str:
            current = checkpoint(document, selector)
            parts = field.split('/')
            for part in parts[:-1]:
                current = current[part]
            current[parts[-1]] = value
            return selector + '/' + field + '=' + repr(value)
        return apply

    def mutate_callback(document: dict[str, Any], method: str, kind: str, path: list[str], value: Any) -> str:
        row = next(r for r in document['callbacks'] if r['method'] == method and r['kind'] == kind)
        current = row
        for key in path[:-1]:
            current = current[key]
        current[path[-1]] = value
        return f'callback {row["callback_id"]} {method}/{"/".join(path)}={value!r}'

    control('forge.missing-callback', 'forge', lambda d: (d['callbacks'].pop(1), 'remove callback row1')[1])
    control('forge.duplicate-callback', 'forge', lambda d: (d['callbacks'].append(copy.deepcopy(d['callbacks'][0])), 'append duplicate callback row0')[1])
    control('forge.unknown-callback', 'forge', lambda d: mutate_callback(d, 'chooseStartingPlayer', 'entry', ['method'], 'unsupportedSyntheticChoice'))
    control('forge.choice-consumption', 'forge', lambda d: (d['completion'].__setitem__('opening_consumed', 3), 'opening_consumed=3')[1])
    control('forge.nonnatural-return', 'forge', lambda d: (d['completion'].__setitem__('natural_start_game_return', False), 'natural_start_game_return=false')[1])
    control('forge.wrong-actual-mana-cost', 'forge', lambda d: mutate_callback(d, 'payManaCost', 'entry', ['arguments', 'cost'], '{9}{G}'))
    control('forge.wrong-payment-debit', 'forge', lambda d: mutate_callback(d, 'applyManaToCost', 'return', ['reply', 'spent'], []))
    control('forge.mana-float', 'forge', mutate_cut('priority.5.beginning_combat', 'players/G/mana/G', '1'))
    control('forge.mana-header-float', 'forge', mutate_cut('priority.5.beginning_combat', 'header/players/G/mana/G', '1'))
    control('forge.survivor-damage', 'forge', mutate_cut('priority.7.postcombat_main', 'cards/G.kalonian-tusker.1/marked_damage', '0'))
    control('forge.cleanup-damage', 'forge', mutate_cut('priority.8.upkeep', 'cards/G.kalonian-tusker.1/marked_damage', '2'))
    control('forge.missing-death', 'forge', mutate_cut('priority.7.postcombat_main', 'cards/W.yoked-ox.1/zone', 'battlefield'))
    control('forge.graveyard-order', 'forge', mutate_cut('terminal', 'players/W/graveyard', ['W.plains.4', 'W.plains.3', 'W.elite-vanguard.1', 'W.yoked-ox.1']))
    control('forge.wrong-library-count', 'forge', mutate_cut('terminal', 'header/players/W/counts/library', 46))
    control('forge.wrong-outcome', 'forge', lambda d: (d['actual_outcome'].__setitem__('winning_seat', 'W'), 'winning_seat=W')[1])
    control('forge.clamped-terminal-life', 'forge', mutate_cut('terminal', 'players/W/life', '0'))
    control('forge.young-attacker-eligibility', 'forge', lambda d: mutate_callback(d, 'declareAttackers', 'entry', ['arguments', 'eligible'], ['W.yoked-ox.1', 'W.elite-vanguard.1']))
    control('forge.mulligan-charge', 'forge', lambda d: mutate_callback(d, 'mulliganKeepHand', 'entry', ['arguments', 'charged'], 9))
    control('forge.bottom-choice', 'forge', lambda d: mutate_callback(d, 'tuckCardsViaMulligan', 'return', ['reply', 'selected'], ['G.forest.1']))
    control('forge.printing-edition', 'forge', lambda d: (d['paper_cards']['Plains'].__setitem__('set', 'UNKNOWN'), 'Plains edition=UNKNOWN')[1])
    control('forge.printing-color', 'forge', lambda d: (d['paper_cards']['Kalonian Tusker'].__setitem__('color_mask', 1), 'Tusker color_mask=1')[1])
    control('forge.missing-observation', 'forge', lambda d: (d['observations'].pop(0), 'remove observation0')[1])
    control('forge.extra-observation', 'forge', lambda d: (d['observations'].append(dict(d['observations'][0], key='unknown.extra.selector')), 'append unknown selector')[1])
    control('forge.duplicate-observation', 'forge', lambda d: (d['observations'].append(copy.deepcopy(d['observations'][0])), 'append duplicate selector')[1])
    control('forge.active-role-at-common-cut','forge',mutate_cut('priority.5.beginning_combat','header/active','W'))
    control('forge.priority-holder-at-common-cut','forge',mutate_cut('priority.5.beginning_combat','header/raw_priority','W'))
    control('forge.draw-inconsistent-with-natural-win','forge',lambda d:(d['actual_outcome'].__setitem__('draw',True),'actual_outcome.draw=true')[1])
    control('forge.winner-flag-inconsistent','forge',lambda d:(d['actual_outcome']['players']['G'].__setitem__('won',False),'actual winnerG won=false')[1])
    control('forge.loser-flag-inconsistent','forge',lambda d:(d['actual_outcome']['players']['W'].__setitem__('lost',False),'actual loserW lost=false')[1])
    control('forge.nonordinary-end-reason','forge',lambda d:(d['actual_outcome'].__setitem__('end_reason','Conceded'),'actual end_reason=Conceded')[1])
    def green_payment(document:dict[str,Any],kind:str)->dict[str,Any]:
        return next(row for row in document['callbacks'] if row['method']=='applyManaToCost' and row['actor']=='G' and row['kind']==kind)
    def mutate_green_payment(document:dict[str,Any],field:str,value:Any)->str:
        if field=='payer':
            row=green_payment(document,'entry');row['arguments']['payer']=value
        else:
            row=green_payment(document,'return');row['reply']['spent'][0]['token'][field]=value
        return 'first actual green applyManaToCost '+field+'='+repr(value)
    control('forge.payment-actual-payer','forge',lambda d:mutate_green_payment(d,'payer','W'))
    control('forge.payment-named-source','forge',lambda d:mutate_green_payment(d,'source','W.plains.1'))
    control('forge.payment-production-operation','forge',lambda d:mutate_green_payment(d,'production_operation_id','RD02.ordinary.1'))
    control('forge.payment-source-incarnation','forge',lambda d:mutate_green_payment(d,'source_timestamp','999999'))
    def swap_alias_binding(document:dict[str,Any])->str:
        aliases=document['physical_aliases'];aliases['W.plains.1'],aliases['W.plains.2']=aliases['W.plains.2'],aliases['W.plains.1']
        return 'swap physical_aliases W.plains.1/W.plains.2 IDs'
    control('forge.physical-alias-identity-swap','forge',swap_alias_binding)
    control('forge.registered-player-ID-binding','forge',lambda d:(d['registered_players']['W'].__setitem__('player_id',999),'registered W player_id=999')[1])
    def damage_change(document: dict[str, Any]) -> str:
        row = next(r for r in document['events'] if r['kind'] == 'GameEventCardDamaged')
        row['payload']['amount'] = '9'
        return f'event {row["ordinal"]} damage amount=9'
    control('forge.damage-quantity', 'forge', damage_change)
    def shuffle_change(document: dict[str, Any]) -> str:
        row = next(r for r in document['random'] if r['kind'] == 'operation_commit')
        row['actual_committed'][0], row['actual_committed'][1] = row['actual_committed'][1], row['actual_committed'][0]
        return 'swap first committed shuffle cards'
    control('forge.shuffle-permutation', 'forge', shuffle_change)
    def native_header_change(document: dict[str, Any], group: str, value: Any) -> str:
        row = document['accepted'][2] if group == 'counts' else document['accepted'][-1]
        if group == 'counts':
            row['header']['counts_by_owner']['41']['library'] = value
        else:
            row['header']['life']['41'] = value
        return f'accepted ordinal{row["ordinal"]} header {group}={value}'
    control('native.missing-input', 'native', lambda d: (d['accepted'].pop(0), 'remove accepted ordinal1')[1])
    control('native.duplicate-input', 'native', lambda d: (d['accepted'].append(copy.deepcopy(d['accepted'][-1])), 'append duplicate accepted last')[1])
    control('native.count-corruption', 'native', lambda d: native_header_change(d, 'counts', 1))
    control('native.life-corruption', 'native', lambda d: native_header_change(d, 'life', '999'))
    control('native.false-replay', 'native', lambda d: (d['rerun'].__setitem__('final_state_equal', False), 'rerun final_state_equal=false')[1])
    control('native.nonatomic-rejection', 'native', lambda d: (d['rejection_probes'][0].__setitem__('state_retained', False), 'probe0 state_retained=false')[1])
    control('native.partial-trajectory', 'native', lambda d: (d.__setitem__('completed', False), 'completed=false')[1])
    control('native.wrong-outcome', 'native', lambda d: (d['final']['outcome'].__setitem__('seat', 41), 'winner seat=41')[1])
    return controls


def file_sha(path: Path) -> str:
    """Hash large retained packets without allocating a second packet-sized buffer."""
    with path.open('rb') as stream:
        return hashlib.file_digest(stream,'sha256').hexdigest()


def envelope_guard(document: dict[str, Any]) -> tuple[Any, ...]:
    """Guard mutated branches and shared inventory references without copying snapshots."""
    def digest(value: Any) -> str:
        return hashlib.sha256(json.dumps(value,sort_keys=True,separators=(',',':')).encode()).hexdigest()
    if document.get('profile')!='teaching':
        return (digest(document),)
    witness=document['type_context_witness']
    return (digest(document['observations']),digest(witness['registry']),digest(witness['coverage']),tuple((point['point_id'],id(point)) for point in witness['snapshots']),tuple((call['call_id'],id(call)) for call in witness['calls']))


def isolated_envelope_mutation(document: dict[str, Any], name: str) -> tuple[dict[str, Any], str]:
    """Copy only branches a named control mutates; share unchanged full witness values."""
    if document.get('profile')!='teaching':
        return copy.deepcopy(document),'uncaptured observations only'
    mutated=dict(document)
    witness=dict(document['type_context_witness'])
    mutated['type_context_witness']=witness
    scope=['outer packet','witness header']
    if name.startswith('packet.') and name!='packet.capture-failure':
        mutated['observations']=copy.deepcopy(document['observations'])
        scope.append('observations')
    if name.startswith('registry.') or name=='capture.not-applicable-registry':
        witness['registry']=copy.deepcopy(witness['registry'])
        scope.append('selected20 registry')
    if name=='capture.incomplete':
        witness['coverage']=dict(witness['coverage'])
        scope.append('coverage header')
    if name=='capture.missing-point':
        witness['snapshots']=list(witness['snapshots'])
        scope.append('snapshot inventory list; immutable point values shared')
    if name=='capture.duplicate-call':
        witness['calls']=list(witness['calls'])
        scope.append('call inventory list; immutable call values shared')
    return mutated,'; '.join(scope)


def envelope_controls(document: dict[str, Any], allow_uncaptured: bool,manifest:dict[str,Any]) -> list[dict[str, Any]]:
    """Exercise case-set and outer failure guards using the actual source packet."""
    output = []
    original_guard=envelope_guard(document)
    def control(name: str, change: Callable[[dict[str, Any]], None]) -> None:
        mutated,copy_scope=isolated_envelope_mutation(document,name)
        change(mutated)
        same(envelope_guard(document),original_guard,'control isolation preserves original packet branches')
        try:
            report, _ = unwrap_native(mutated, allow_uncaptured,manifest)
            runs = unique(report['runs'], 'fixture_ref', 'native case')
            same(set(runs), {'RD01', 'RD02'}, 'native strict case set')
        except EvidenceError as error:
            output.append(dict(name=name, rejected=True, reason=str(error),copy_scope=copy_scope,original_packet_unchanged=True))
        else:
            raise EvidenceError('ineffective envelope control ' + name)
    def inner(value: dict[str, Any]) -> dict[str, Any]:
        return value['observations'] if value.get('profile') == 'teaching' else value
    control('packet.missing-case', lambda d: inner(d)['data']['runs'].pop())
    control('packet.extra-case', lambda d: inner(d)['data']['runs'].append(dict(inner(d)['data']['runs'][0], fixture_ref='RD03')))
    control('packet.duplicate-case', lambda d: inner(d)['data']['runs'].append(copy.deepcopy(inner(d)['data']['runs'][0])))
    for status in ['fail', 'error', 'unsupported', 'partial']:
        control('packet.' + status, lambda d, status=status: inner(d).__setitem__('status', status))
    control('packet.capture-failure', lambda d: d.__setitem__('capture_failure', 'synthetic rejected corruption'))
    if document.get('profile') == 'teaching':
        control('capture.not-applicable-registry', lambda d: d['type_context_witness'].__setitem__('registry', {'tag': 'NotApplicable'}))
        control('capture.incomplete', lambda d: d['type_context_witness']['coverage'].__setitem__('complete', False))
        control('capture.missing-point', lambda d: d['type_context_witness']['snapshots'].pop())
        control('capture.duplicate-call', lambda d: d['type_context_witness']['calls'].append(copy.deepcopy(d['type_context_witness']['calls'][0])))
        def registry(value:dict[str,Any])->dict[str,Any]:
            return value['type_context_witness']['registry']['coverage']
        def registry_card(value:dict[str,Any],land:bool|None=None)->dict[str,Any]:
            rows=registry(value)['cards']
            return rows[0] if land is None else next(row for row in rows if (row['name'] in ['Plains','Forest'])==land)
        control('registry.foreign-Oracle-identity',lambda d:registry_card(d).__setitem__('oracleId','00000000-0000-0000-0000-000000000000'))
        control('registry.wrong-name',lambda d:registry_card(d).__setitem__('name','Foreign Authored Card'))
        control('registry.wrong-semantic-signature',lambda d:registry_card(d)['binding'].__setitem__('semanticSha256','0'*64))
        control('registry.land-key-swapped-to-creature',lambda d:registry_card(d,True)['binding'].__setitem__('program','teaching-vanilla-creature-v1'))
        control('registry.creature-key-swapped-to-land',lambda d:registry_card(d,False)['binding'].__setitem__('program','teaching-basic-land-v1'))
        control('registry.wrong-scope',lambda d:registry_card(d)['binding'].__setitem__('scope','seeded-vanilla-battlefield-only'))
        control('registry.required-false',lambda d:registry_card(d).__setitem__('required',False))
        control('registry.wrong-partial-status',lambda d:registry_card(d)['binding'].__setitem__('status','blocked'))
        control('registry.duplicate-identity',lambda d:registry(d)['cards'].append(copy.deepcopy(registry_card(d))))
        control('registry.missing-identity',lambda d:registry(d)['cards'].pop())
        control('registry.wrong-vocabulary',lambda d:registry(d).__setitem__('creatureVocabularyId','0'*64))
        control('registry.wrong-source-record-count',lambda d:registry(d).__setitem__('sourceRecords',20))
        control('registry.wrong-selected-count',lambda d:registry(d).__setitem__('requiredPartialBindings',19))
        control('registry.overclaimed-qualification',lambda d:registry(d).__setitem__('fullyQualifiedCards',20))
    return output


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--reference-run', type=Path, required=True)
    parser.add_argument('--native', type=Path, required=True)
    parser.add_argument('--allow-uncaptured', action='store_true')
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    out = args.reference_run
    compare(out / 'cases-v1.json', out / 'first-pair.json', out / 'evidence', args.native, args.allow_uncaptured)
    native_source_sha_before=file_sha(args.native)
    document = read(args.native)
    spec, manifest = read(out / 'cases-v1.json'), read(out / 'first-pair.json')
    native, provenance = unwrap_native(document, args.allow_uncaptured,manifest)
    controls = exercise_controls(spec, manifest, read(out / 'evidence/RD02.json'), next(r for r in native['runs'] if r['fixture_ref'] == 'RD02'))
    controls += envelope_controls(document, args.allow_uncaptured,manifest)
    require(all(c['rejected'] for c in controls), 'ineffective control')
    same(file_sha(args.native),native_source_sha_before,'retained native source file unchanged by controls')
    result = dict(control_isolation='Only named mutation branches are copied; unchanged full witness values are shared. Original observation/Registry/coverage digests and point/call identity inventories verified after each mutation. Retained source file digest unchanged.',schema='teaching-reference-reader-controls-v1', baseline_equal=True, native_capture=provenance, controls=controls, count=len(controls), all_rejected=True, source_receipts={str(p):file_sha(p) for p in [Path(__file__), Path(__file__).with_name('teaching_reference_compare.py'), args.native, out / 'evidence/RD02.json']})
    with args.out.open('x') as stream:
        json.dump(result, stream, indent=2)
        stream.write('\n')
    print(json.dumps(dict(controls=len(controls), all_rejected=True)))


if __name__ == '__main__':
    main()
