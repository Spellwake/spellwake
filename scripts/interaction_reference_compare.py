#!/usr/bin/env python3
"""Strict raw readers and source-qualified joins for finite Blue, mandatory and optional profiles."""
from __future__ import annotations

import argparse
import hashlib
import json
import math
from dataclasses import dataclass
import re
from datetime import datetime
from pathlib import Path
from typing import Any, Callable, TypeVar, IO, Iterator

from interaction_forge_reference import (AUTHORITY, PIN, PROFILES, MANDATORY_PINS, sha,
    mandatory_authorities, make_mandatory_inputs, reference_alias, strict_json, classpath_receipts)
from teaching_reference_compare import EvidenceError, read, require, same, unique, pointer, cost_tuple

ROLES={'41':'U','3':'G'}
MANA={'W':'white','U':'blue','B':'black','R':'red','G':'green','C':'colorless'}
METHODS={'applyManaToCost','assignCombatDamage','autoPassCancel','awaitNextInput','cheatShuffle','chooseSpellAbilityToPlay','chooseStartingPlayer','chooseTargetsFor','complainCardsCantPlayWell','declareAttackers','declareBlockers','getAbilityToPlay','getCostDecisionMaker','mulliganKeepHand','payManaCost','playChosenSpellAbility','resetAtEndOfTurn','revealUnsupported','visit_CostPartMana','visit_CostTap'}
NATIVE_INPUT_PINS={
    'metadata':'77a7ffc2250306197da78c0c4faf35fb4fae01b4dbdd50bf8eb45f502e4737ad',
    'snapshot':'c1c9dc28fa9a7a4bae61c02330ba5a1190205aab1a93361e590ff5d9a146af08',
    'vocabulary':'dd16c29df30b69a72ea9021f0e60429a8537914e9e85187d36559e42ce59cd3c',
    'publication':'29950c2ce036e1bcee6b9a86d9947294a11ef297a2cdcdc75edbcc0b5de40253',
}
SUPPLEMENTAL_PROBES={'IF13.divination-in-response','IF13.divination-opponent-turn','IF13.unsummon-creature-spell'}
NATIVE_CHECK_COMMANDS={
    'fmt':['cargo','fmt','--all','--check'],
    'check':['cargo','check','--locked','--workspace','--all-targets'],
    'clippy':['cargo','clippy','--locked','--workspace','--all-targets','--','-D','warnings'],
    'workspace-tests':['cargo','test','--locked','--workspace'],
    'teaching-tests':['cargo','test','--locked','-p','spellwake-reference-tools','--test','teaching_decks','--','--include-ignored'],
    'interaction-tests':['cargo','test','--locked','-p','spellwake-reference-tools','--test','interaction_games','--','--include-ignored'],
    'interaction-registry-tests':['cargo','test','--locked','-p','spellwake-card-programs','--test','interaction_registry','--','--include-ignored'],
    'cli-build':['cargo','build','--locked','-p','spellwake-reference-tools','--bin','teaching-game','--bin','interaction-game'],
    'python-guards':['uv','run','--no-project','python','-m','unittest','discover','-s','tests/reference','-p','test_*.py'],
}
MANDATORY_CHECK_COMMANDS = {'fmt': ['cargo', 'fmt', '--all', '--check'],
 'check': ['cargo', 'check', '--locked', '--workspace', '--all-targets'],
 'clippy': ['cargo', 'clippy', '--locked', '--workspace', '--all-targets', '--', '-D', 'warnings'],
 'workspace-tests': ['cargo', 'test', '--locked', '--workspace'],
 'teaching-tests': ['cargo',
                    'test',
                    '--locked',
                    '-p',
                    'spellwake-reference-tools',
                    '--test',
                    'teaching_decks',
                    '--',
                    '--include-ignored'],
 'interaction-tests': ['cargo',
                       'test',
                       '--locked',
                       '-p',
                       'spellwake-reference-tools',
                       '--test',
                       'interaction_games',
                       '--',
                       '--include-ignored'],
 'interaction-registry-tests': ['cargo',
                                'test',
                                '--locked',
                                '-p',
                                'spellwake-card-programs',
                                '--test',
                                'interaction_registry',
                                '--',
                                '--include-ignored'],
 'mandatory-registry-tests': ['cargo',
                              'test',
                              '--locked',
                              '-p',
                              'spellwake-card-programs',
                              '--test',
                              'mandatory_registry',
                              '--',
                              '--include-ignored'],
 'mandatory-full-tests': ['cargo',
                          'test',
                          '--locked',
                          '-p',
                          'spellwake-reference-tools',
                          '--test',
                          'mandatory_games',
                          'mandatory_full_source_games_and_reduced_cases',
                          '--',
                          '--ignored',
                          '--exact',
                          '--nocapture'],
 'mandatory-probe-tests': ['cargo',
                           'test',
                           '--locked',
                           '-p',
                           'spellwake-reference-tools',
                           '--test',
                           'mandatory_games',
                           'mandatory_decision_authority_probes',
                           '--',
                           '--ignored',
                           '--exact',
                           '--nocapture'],
 'mandatory-synthetic-tests': ['cargo',
                               'test',
                               '--locked',
                               '-p',
                               'spellwake-reference-tools',
                               '--test',
                               'mandatory_games',
                               'mandatory_llanowar_control_change_is_a_separate_synthetic_mechanism',
                               '--',
                               '--ignored',
                               '--exact',
                               '--nocapture'],
 'mandatory-foreign-tests': ['cargo',
                             'test',
                             '--locked',
                             '-p',
                             'spellwake-reference-tools',
                             '--lib',
                             'ordinary_game::mandatory::foreign_instance_tests::mandatory_foreign_trigger_instance_rejects_and_replays_with_real_foreign_handle',
                             '--',
                             '--ignored',
                             '--exact',
                             '--nocapture'],
 'mandatory-observer-tests': ['cargo',
                              'test',
                              '--locked',
                              '-p',
                              'spellwake-reference-tools',
                              '--lib',
                              'ordinary_game::record::observer_tests::token_marks_and_counters_survive_shared_observation_and_input_rerun',
                              '--',
                              '--exact',
                              '--nocapture'],
 'historical-api-tests': ['cargo',
                          'test',
                          '--locked',
                          '-p',
                          'spellwake-reference-tools',
                          '--test',
                          'predecessor_api',
                          'historical_game_api_bytes_are_preserved',
                          '--',
                          '--ignored',
                          '--exact',
                          '--nocapture'],
 'teaching-interaction-api-tests': ['cargo',
                                    'test',
                                    '--locked',
                                    '-p',
                                    'spellwake-reference-tools',
                                    '--test',
                                    'predecessor_api',
                                    'teaching_and_interaction_api_bytes_are_preserved',
                                    '--',
                                    '--ignored',
                                    '--exact',
                                    '--nocapture'],
 'cli-build': ['cargo',
               'build',
               '--locked',
               '-p',
               'spellwake-reference-tools',
               '--bin',
               'teaching-game',
               '--bin',
               'interaction-game',
               '--bin',
               'mandatory-trigger-game'],
 'python-guards': ['uv',
                   'run',
                   '--no-project',
                   'python',
                   '-m',
                   'unittest',
                   'discover',
                   '-s',
                   'tests/reference',
                   '-p',
                   'test_*.py']}

MANDATORY_HELPER_HASHES={'run_checks.py':'a73402f84128cb49cd1e78e9fd8be982ce36b88499ada6f3936bb1f8365e9025','capture_native.py':'aa1ec851137564ba0ed8aa649a35ac1c5abc95d902095ab355b522b398aff003','run_mechanisms.py':'2c30072d5ee743c4ce69d37222c48d8867558fabf8c7a7b8f26f2421f4570d0c'}

class JsonStreamError(ValueError):
    """Invalid JSON, a resource bound, or incorrect iterator consumption."""


@dataclass
class _Frame:
    kind: str
    state: str = "scanning"


_WHITESPACE = " \t\r\n"
_BOUNDARY = " \t\r\n,]}"
_NUMBER_END = re.compile(r"[ \t\r\n,\]}]")
_NUMBER = re.compile(r"-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?\Z")


def _unique_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise JsonStreamError(f"duplicate object key {key!r}")
        result[key] = value
    return result


def _finite_float(token: str) -> float:
    value = float(token)
    if not math.isfinite(value):
        raise JsonStreamError("non-finite JSON number")
    return value


def _reject_constant(token: str) -> Any:
    raise JsonStreamError(f"non-JSON constant {token}")


class JsonStream:
    """Strict incremental JSON over a text source.

    ``open`` supplies strict UTF-8 decoding for files. A caller-supplied text source
    must already decode bytes strictly. Every yielded object key/array index must
    be followed by exactly one fully consumed value. ``finish`` verifies complete
    consumption and EOF; a successful context manager exit calls it automatically.
    All expected input/consumption failures raise ``JsonStreamError``.
    """

    def __init__(
        self,
        source: IO[str],
        *,
        chunk_chars: int = 65_536,
        max_value_chars: int = 128 * 1024 * 1024,
        owns_source: bool = False,
    ) -> None:
        if chunk_chars < 1 or max_value_chars < 1:
            raise ValueError("JSON resource bounds must be positive")
        self._source = source
        self._owns_source = owns_source
        self._chunk_chars = min(chunk_chars, max_value_chars)
        self._max_value_chars = max_value_chars
        self._buffer = ""
        self._position = 0
        self._consumed = 0
        self._eof = False
        self._closed = False
        self._root_started = False
        self._root_complete = False
        self._frames: list[_Frame] = []
        self._decoder = json.JSONDecoder(
            object_pairs_hook=_unique_object,
            parse_float=_finite_float,
            parse_constant=_reject_constant,
        )
        self.max_buffer_chars = 0
        self.read_calls = 0
        self.decode_attempts = 0

    @classmethod
    def open(cls, path: Path | str, **bounds: Any) -> JsonStream:
        """Open a file with strict UTF-8, preserving all JSON whitespace."""
        source = Path(path).open("r", encoding="utf-8", errors="strict", newline="")
        try:
            return cls(source, owns_source=True, **bounds)
        except BaseException:
            source.close()
            raise

    @property
    def offset(self) -> int:
        """Consumed Unicode character offset, not a byte offset."""
        return self._consumed + self._position

    def _fail(self, message: str) -> JsonStreamError:
        return JsonStreamError(f"{message} at character {self.offset}")

    def _read_more(self, count: int) -> None:
        if self._closed:
            raise self._fail("reader is closed")
        if self._eof:
            return
        if self._position:
            self._consumed += self._position
            self._buffer = self._buffer[self._position:]
            self._position = 0
        try:
            chunk = self._source.read(count)
        except (UnicodeError, OSError) as error:
            raise self._fail("cannot read strict UTF-8 JSON") from error
        self.read_calls += 1
        if not isinstance(chunk, str):
            raise self._fail("JSON source must yield decoded text")
        if not chunk:
            self._eof = True
        else:
            self._buffer += chunk
            self.max_buffer_chars = max(self.max_buffer_chars, len(self._buffer))

    def _peek(self) -> str:
        if self._closed:
            raise self._fail("reader is closed")
        if self._position == len(self._buffer) and not self._eof:
            self._read_more(self._chunk_chars)
        return self._buffer[self._position:self._position + 1]

    def _space(self) -> None:
        while True:
            char = self._peek()
            if not char or char not in _WHITESPACE:
                return
            self._position += 1

    def _expect(self, char: str) -> None:
        self._space()
        if self._peek() != char:
            raise self._fail(f"expected {char!r}")
        self._position += 1

    def _begin_value(self) -> None:
        if self._frames:
            parent = self._frames[-1]
            if parent.state != "pending":
                raise self._fail("exactly one value is required for each yielded item")
            parent.state = "in_progress"
        else:
            if self._root_started:
                raise self._fail("root value was already started")
            self._root_started = True

    def _end_value(self) -> None:
        if self._frames:
            parent = self._frames[-1]
            if parent.state != "in_progress":
                raise self._fail("invalid container ownership")
            parent.state = "consumed"
        else:
            self._root_complete = True

    def _boundary(self, *, key: bool = False) -> None:
        char = self._peek()
        allowed = _WHITESPACE + ":" if key else _BOUNDARY
        if char and char not in allowed:
            raise self._fail("invalid character following JSON value")

    def _decode_number(self) -> Any:
        scanned = self._position
        while True:
            match = _NUMBER_END.search(self._buffer, scanned)
            end = match.start() if match else len(self._buffer)
            length = end - self._position
            if length > self._max_value_chars:
                raise self._fail("JSON value exceeds character limit")
            if match or self._eof:
                token = self._buffer[self._position:end]
                if not _NUMBER.fullmatch(token):
                    raise self._fail("invalid JSON number")
                try:
                    value = self._decoder.decode(token)
                except (ValueError, RecursionError) as error:
                    raise self._fail("invalid or unsupported JSON number") from error
                self._position = end
                return value
            if length == self._max_value_chars:
                # One lookahead character distinguishes an exact-bound value
                # from a token which exceeds the configured bound.
                more = 1
            else:
                more = min(max(length, self._chunk_chars), self._max_value_chars - length)
            scanned = length
            self._read_more(more)

    def _decode_value(self, *, key: bool = False) -> Any:
        self._space()
        char = self._peek()
        if not char:
            raise self._fail("expected JSON value")
        if not key and char in "-0123456789":
            return self._decode_number()
        if key and char != '"':
            raise self._fail("object key must be a string")
        while True:
            available = len(self._buffer) - self._position
            self.decode_attempts += 1
            try:
                value, end = self._decoder.raw_decode(self._buffer, self._position)
            except json.JSONDecodeError as error:
                if self._eof:
                    raise self._fail("invalid or incomplete JSON value") from error
                if available >= self._max_value_chars:
                    raise self._fail("JSON value exceeds character limit") from error
                # Exponential refill avoids reparsing a large incomplete row
                # after every fixed-size chunk.
                more = min(max(available, self._chunk_chars), self._max_value_chars - available)
                self._read_more(more)
                continue
            except (ValueError, RecursionError) as error:
                raise self._fail("invalid or unsupported JSON value") from error
            if end - self._position > self._max_value_chars:
                raise self._fail("JSON value exceeds character limit")
            self._position = end
            self._boundary(key=key)
            return value

    def read_value(self) -> Any:
        """Decode exactly one complete value, including all nested fields."""
        self._begin_value()
        value = self._decode_value()
        self._end_value()
        return value

    def _consumed_item(self, frame: _Frame) -> None:
        if not self._frames or self._frames[-1] is not frame or frame.state != "consumed":
            raise self._fail("yielded item was not fully consumed")
        frame.state = "scanning"

    def iter_object(self) -> Iterator[str]:
        """Yield each unique key; consume its value before requesting another."""
        self._begin_value()
        self._expect("{")
        frame = _Frame("object")
        self._frames.append(frame)
        keys: set[str] = set()
        self._space()
        if self._peek() != "}":
            while True:
                key = self._decode_value(key=True)
                if key in keys:
                    raise self._fail(f"duplicate object key {key!r}")
                keys.add(key)
                self._expect(":")
                frame.state = "pending"
                yield key
                self._consumed_item(frame)
                self._space()
                if self._peek() == "}":
                    break
                self._expect(",")
        self._expect("}")
        self._frames.pop()
        self._end_value()

    def iter_array(self) -> Iterator[int]:
        """Yield row indices; fully consume each value before requesting another."""
        self._begin_value()
        self._expect("[")
        frame = _Frame("array")
        self._frames.append(frame)
        self._space()
        index = 0
        if self._peek() != "]":
            while True:
                frame.state = "pending"
                yield index
                self._consumed_item(frame)
                index += 1
                self._space()
                if self._peek() == "]":
                    break
                self._expect(",")
                self._space()
                if self._peek() == "]":
                    raise self._fail("trailing array comma")
        self._expect("]")
        self._frames.pop()
        self._end_value()

    def finish(self) -> None:
        """Require one completed root value, no abandoned containers and EOF."""
        if self._frames or not self._root_complete:
            raise self._fail("JSON root was not fully consumed")
        self._space()
        if self._peek():
            raise self._fail("trailing data after JSON root")

    def __enter__(self) -> JsonStream:
        return self

    def __exit__(self, kind: Any, value: Any, traceback: Any) -> None:
        try:
            if kind is None:
                self.finish()
        finally:
            self._closed = True
            if self._owns_source:
                self._source.close()


T=TypeVar('T')


def read(path:Path)->Any:
    """Share the strict boundary parser; malformed numbers/members never become evidence."""
    try:return strict_json(path)
    except (ValueError,OSError) as error:raise EvidenceError('malformed evidence '+str(path)+': '+str(error)) from error


def evidence_boundary(function: Callable[...,T]) -> Callable[...,T]:
    """Malformed/dangling raw structures have the same typed rejection boundary as mismatches."""
    def guarded(*args:Any,**kwargs:Any)->T:
        try:return function(*args,**kwargs)
        except EvidenceError:raise
        except (KeyError,IndexError,TypeError,AttributeError,ValueError,OSError,StopIteration) as error:
            raise EvidenceError(function.__name__+' malformed evidence: '+str(error)) from error
    return guarded


def same(actual:Any,expected:Any,label:str)->None:
    """Compare exact wire values, including strict boolean/integer types and precise failure paths."""
    if type(actual) is not type(expected):raise EvidenceError(label+' type: '+type(actual).__name__+' != '+type(expected).__name__)
    if isinstance(actual,dict):
        if set(actual)!=set(expected):raise EvidenceError(label+' exact member set differs')
        for key in actual:same(actual[key],expected[key],label+'/'+str(key))
    elif isinstance(actual,list):
        if len(actual)!=len(expected):raise EvidenceError(label+' length '+str(len(actual))+' != '+str(len(expected)))
        for index,(a,b) in enumerate(zip(actual,expected,strict=True)):same(a,b,label+'/'+str(index))
    elif actual!=expected:raise EvidenceError(label+': actual='+str(actual)[:300]+' expected='+str(expected)[:300])


def integer(value:Any,label:str)->int:
    """Require a canonical decimal, rejecting booleans and floats."""
    require(isinstance(value,str) and re.fullmatch(r'-?(0|[1-9][0-9]*)',value) is not None and str(int(value))==value,label+' canonical integer')
    return int(value)


def target_semantics(target:dict[str,Any])->dict[str,Any]:
    """Keep semantic target class/physical alias; validate incarnation and stack identity separately."""
    if target['kind']=='object':return dict(kind='object',card=target['alias'])
    require(target['kind']=='stack','unknown actual target kind')
    return dict(kind='stack',spell=target['source'])


@evidence_boundary
def world_projection(world:dict[str,Any],inventory:dict[str,Any],physical:dict[str,Any],registered:dict[str,Any])->dict[str,Any]:
    """Validate complete actual physical membership, headers and current-card bindings."""
    same(set(world['cards']),set(inventory),'world exact physical inventory')
    same(set(world['players']),{'U','G'},'world roles')
    all_seen=[];players={};cards={}
    stack=world['stack'];stack_aliases=[row['ability']['source'] for row in stack]
    require(len(set(stack_aliases))==len(stack_aliases),'physical source cannot occupy two live entries')
    require(len({row['stack_id'] for row in stack})==len(stack),'duplicate live stack ID')
    require(len({row['ability']['ability_id'] for row in stack})==len(stack),'duplicate live ability ID')
    for role,player in world['players'].items():
        membership=[]
        for zone in ['library','hand','battlefield','graveyard','exile']:
            aliases=player[zone]
            same(len(aliases),world['header']['players'][role]['counts'][zone],'header pile count '+role+zone)
            for alias in aliases:
                require(alias in inventory,'unknown pile alias')
                same(world['cards'][alias]['zone'],zone,'actual card/pile zone')
                same(world['cards'][alias]['owner'],role,'actual card/pile owner')
            membership+=aliases
        membership += [alias for alias in stack_aliases if inventory[alias]['role']==role]
        same(len(membership),60,'per-owner60 conservation')
        require(len(set(membership))==60,'duplicate actual physical membership')
        all_seen+=membership
        same(player['life'],world['header']['players'][role]['life'],'header life')
        same(player['mana'],world['header']['players'][role]['mana'],'header mana')
        integer(player['life'],'life')
        for color,amount in player['mana'].items():require(color in MANA and integer(amount,'mana')>=0,'valid mana pool')
        players[role]={key:(sorted(player[key]) if key in ['hand','battlefield','exile'] else player[key]) for key in ['life','mana','library','hand','battlefield','graveyard','exile']}
    same(set(all_seen),set(inventory),'every actual card in exactly one zone')
    same({a for a,c in world['cards'].items() if c['zone']=='stack'},set(stack_aliases),'actual physical stack membership')
    for alias,card in world['cards'].items():
        same(card['physical_id'],str(physical[alias]),'actual physical alias ID')
        same(card['owner'],inventory[alias]['role'],'source owner');same(card['owner_player_id'],registered[card['owner']]['player_id'],'actual card owner/player ID')
        require(card['controller'] is None or card['controller'] in registered,'current controller domain')
        integer(card['timestamp'],'actual current timestamp')
        if card['zone']=='battlefield':require(type(card['tapped']) is bool,'actual battlefield tapped flag')
        for key in ['power','toughness','marked_damage','plus_one_counters','minus_one_counters']:
            if card[key] is not None:integer(card[key],key)
        cards[alias]={key:card[key] for key in ['owner','controller','zone','tapped','power','toughness','marked_damage','sick','plus_one_counters','minus_one_counters']}
    for row in stack:
        actual=row['ability'];alias=actual['source'];same(actual['source_physical_id'],str(physical[alias]),'actual stack physical source');same(actual['source_timestamp'],world['cards'][alias]['timestamp'],'actual stack source incarnation')
        for target in actual['targets']:
            require(target_semantics(target).get('card',target_semantics(target).get('spell')) in inventory,'stack target known physical card')
    header=world['header']
    same(len(stack),header['stack_size'],'actual stack header count')
    return dict(turn=header['individual_turn'],step=header['step'],active=header['active'],priority=None if header['game_over'] else header['raw_priority'],players=players,cards=cards,
        stack=[dict(source=row['ability']['source'],targets=[target_semantics(t) for t in row['ability']['targets']]) for row in stack],combat=sorted([dict(row,blockers=sorted(row['blockers'])) for row in world['combat']],key=lambda row:row['attacker']))


@evidence_boundary
def validate_streams(raw:dict[str,Any],methods:set[str]|None=None,roles:tuple[str,str]=('U','G'))->dict[int,dict[str,Any]]:
    """Validate every actual callback/operation/event, parent pairing and source call context."""
    combined=[]
    for stream in ['callbacks','events','random','operations','failures']:
        rows=raw[stream];same([r['ordinal'] for r in rows],list(range(len(rows))),stream+' ordinals')
        require(all(r['case_id']==raw['case_id'] for r in rows),'stream case binding')
        require([r['global_sequence'] for r in rows]==sorted(r['global_sequence'] for r in rows),'stream causal order')
        combined+=rows
    same(sorted(r['global_sequence'] for r in combined),list(range(1,len(combined)+1)),'complete unique global event stream')
    pending=[];entries={}
    for row in raw['callbacks']:
        require(row['method'] in (METHODS if methods is None else methods),'unknown callback method')
        if row['kind']=='entry':
            require(row['callback_id'] not in entries,'duplicate callback ID')
            same(row['parent_callback_id'],pending[-1] if pending else None,'actual callback parent nesting')
            require(row['actor'] in roles,'callback actor')
            same(row['controller_class'],'forge.ai.SpellwakeBlueInteractionReferenceTest$StrictController','strict controller class')
            require(bool(row['actual_stack_frames']) and bool(row['actual_stack_tags']),'actual callback source frames')
            same([r['class']+'.'+r['method'] for r in row['actual_stack_frames']],row['actual_stack_tags'],'actual source-frame tags')
            entries[row['callback_id']]=row;pending.append(row['callback_id'])
        else:
            same(row['kind'],'return','failure callback forbidden in completed run')
            require(bool(pending),'unpaired callback return')
            same(pending.pop(),row['callback_id'],'actual callback return nesting')
            same(row['method'],entries[row['callback_id']]['method'],'callback method identity')
            same(row['actor'],entries[row['callback_id']]['actor'],'callback actor identity')
    same(pending,[],'all callback leaves paired');same(list(entries),list(range(1,len(entries)+1)),'actual sequential callback allocation')
    return entries


@evidence_boundary
def validate_printings(raw:dict[str,Any],manifest:dict[str,Any],inputs:dict[str,Any])->None:
    """Require actual loaded printing and complete saved semantic tuple, not declared names alone."""
    cards={c['name']:c for c in manifest['cards']};same(set(raw['paper_cards']),set(cards),'all20 loaded actual printings')
    for name,card in cards.items():
        actual=raw['paper_cards'][name];definition=inputs['definitions'][name];face=card['faces'][0]
        for key,expected in [('name',name),('set',definition['set']),('collector_number',definition['collector_number']),('foil',False),('oracle_id',card['oracleId']),('oracle_text',face['oracle_text']),('mana_cost',face['mana_cost'] or 'no cost'),('printed_power',face['power']),('printed_toughness',face['toughness']),('type',face['type_line'].replace('—','-')),('color_mask',definition['color_mask'])]:same(actual[key],expected,'printing '+name+' '+key)
        same(actual['source_receipt']['saved_card'],card,'actual printing saved source binding')
        same(actual['source_receipt']['forge_resources'],definition['source_receipt']['forge_resources'],'actual printing resources binding')
        require(type(actual['art_index']) is int and actual['art_index']>0,'actual art index')
        same(actual['basic_land'],face['type_line'].startswith('Basic Land'),'actual basic flag')
        same(actual['creature'],face['type_line'].startswith('Creature'),'actual creature flag')
        same(sorted(actual['subtypes']),sorted(card['creatureSubtypes'] if actual['creature'] else [name] if actual['basic_land'] else []),'actual complete subtypes')
        same(actual['supertypes'],['Basic'] if actual['basic_land'] else [],'actual complete supertypes')


@evidence_boundary
def validate_forge_settlement_events(raw:dict[str,Any],cast_rows:dict[str,Any])->None:
    """One actual event-owned cast-disposition validator for both maintained profiles."""
    same(set(raw['settlements']),set(cast_rows),'all cast entries settled exactly once')
    for action,status in raw['settlements'].items():
        require(status in ['resolved','countered','all_targets_illegal_nonresolution'],'explicit entry disposition')
        events=[r for r in raw['settlement_events'] if r['action_id']==action]
        if status in ['resolved','all_targets_illegal_nonresolution']:
            evidence=[e for e in events if e['raw_event']=='GameEventSpellResolved'];same(len(evidence),1,'one actual resolved/fizzled event');same(evidence[0]['payload']['hasFizzled'],status=='all_targets_illegal_nonresolution','actual fizzled flag')
        else:
            require(any(e['raw_event']=='GameEventSpellRemovedFromStack' and e['disposition']=='countered' and any(t.startswith('forge.game.ability.effects.CounterEffect.') for t in e['actual_stack_tags']) for e in events),'countered actual source-path removal')
            same([e for e in events if e['raw_event']=='GameEventSpellResolved'],[],'countered entry never resolved')
    events={row['global_sequence']:row for row in raw['events']};seen=set()
    for row in raw['settlement_events']:
        require(row['action_id'] in cast_rows,'actual settlement belongs to a known cast');sequence=row['event_sequence'];require(sequence not in seen,'one actual settlement receipt per emitted event');seen.add(sequence);require(sequence in events,'actual settlement emitted event exists');event=events[sequence];same(row['raw_event'],event['kind'],'actual settlement emitted event family');same(row['payload'],event['payload'],'actual settlement complete emitted payload');require(row['raw_event'] in ('GameEventSpellResolved','GameEventSpellRemovedFromStack'),'closed actual cast settlement event');ability=row['payload']['spell' if row['raw_event']=='GameEventSpellResolved' else 'sa'];cast=cast_rows[row['action_id']];same(ability['ability_id'],cast['ability_id'],'actual settlement cast ability identity');same(ability['host']['alias'],cast['source'],'actual settlement cast physical source');same(ability['host']['physical_id'],cast['source_physical_id'],'actual settlement cast physical identity')


@evidence_boundary
def forge_projection(raw:dict[str,Any],case:dict[str,Any],full:dict[str,Any],manifest:dict[str,Any],inputs:dict[str,Any])->dict[str,Any]:
    """Read actual raw source-bound execution, including all original-cast/target/payment relations."""
    same(raw['schema'],'interaction-game-forge-v1','raw schema');same(raw['schema_version'],1,'raw schema version');same(raw['forge_commit'],PIN,'Forge pin');same(raw['case_id'],case['case_id'],'actual case identity');same(raw['fixture'],case,'actual frozen input recipe')
    same(raw['failures'],[],'actual execution has no sticky failure')
    inventory=full['inventory'];same(set(raw['physical_aliases']),set(inventory),'all120 physical aliases')
    require(len(set(raw['physical_aliases'].values()))==120,'physical ID bijection')
    same(set(raw['registered_players']),{'U','G'},'registered roles');require(len({r['player_id'] for r in raw['registered_players'].values()})==2,'registered player ID bijection')
    entries=validate_streams(raw);validate_printings(raw,manifest,inputs)
    for entry in entries.values():same(entry['actor_player_id'],raw['registered_players'][entry['actor']]['player_id'],'actual callback actor/player ID')
    completion=raw['completion']
    for field in ['actions','decisions','opening','random_operations']:same(completion[field+'_consumed'],completion[field+'_planned'],'complete '+field)
    same(completion['actions_planned'],len(case['actions']),'recipe action count');same(completion['opening_planned'],len(case['opening_policy']),'recipe opening count');same(completion['random_operations_planned'],len(case['forge_operation_queue']),'recipe entropy count')
    same(completion['callback_depth'],0,'actual callback depth');same(completion['sticky_failure_count'],0,'sticky failures')
    points=unique(raw['observations'],'key','observation selector');checkpoints=raw['checkpoints'];worlds={}
    global_ids={r['global_sequence'] for stream in ['callbacks','events','random','operations'] for r in raw[stream]}
    for key,point in points.items():
        require(point['source_global_sequence'] in global_ids,'observation actual source sequence');same(point['observation_id'],'observation-'+str(point['source_global_sequence']),'actual observation source-ID binding');same(point['checkpoint_ref'],point['observation_id'],'actual observation source-checkpoint binding')
        require(point['checkpoint_ref'] in checkpoints,'observation actual checkpoint pointer')
        world=checkpoints[point['checkpoint_ref']];worlds[key]=world_projection(world,inventory,raw['physical_aliases'],raw['registered_players'])
        allowed=key in ['setup.predraw','opening.complete','prepared.initialized','prepared.first_ready','terminal','terminal.return','bounded.stop'] or re.fullmatch(r'(priority\.[0-9]+\.[a-z_]+|action\.[0-9]+|settle\..+|attack-event\.[0-9]+|block-event\.[0-9]+|damage\.[0-9]+)',key)
        require(bool(allowed),'unknown observation selector')
        if key.startswith('priority.'):
            _,number,step=key.split('.');same(worlds[key]['turn'],int(number),'priority selector actual turn');same(worlds[key]['step'],step,'priority selector actual step')
            require(int(number)<=case['watchdog']['maximum_individual_turn'],'priority observation finite turn bound')
    casts=[a for a in case['actions'] if a['kind']=='cast'];cast_rows=unique(raw['cast_operations'],'action_id','cast operation');same(set(cast_rows),{a['policy_id'] for a in casts},'every actual named cast operation')
    required={'action.'+str(i) for i in range(len(case['actions']))}|{'settle.'+a['policy_id'] for a in casts}
    prepared=case['initialization']['kind']=='scheduled'
    bounded=case['stop']['kind']=='ready_priority'
    required|={'prepared.initialized','prepared.first_ready'} if prepared else {'opening.complete'}
    required|={'bounded.stop'} if bounded else {'terminal','terminal.return'}
    require(required<=set(points),'required actual selected observations missing')
    all_operations=unique([r for r in raw['operations'] if r['kind']=='ordinary_entry'],'operation_id','ordinary entry')
    returns=unique([r for r in raw['operations'] if r['kind']=='ordinary_return'],'operation_id','ordinary return');same(set(returns),set(all_operations),'actual operation entry/return coverage')
    ordinary=[a for a in case['actions'] if a['kind'] not in ['pass','declare_attackers','declare_blockers']]
    same(len(all_operations),len(ordinary),'ordinary action count')
    operations_by_action={r['policy']['policy_id']:r for r in all_operations.values()};same(set(operations_by_action),{a['policy_id'] for a in ordinary},'actual action operation ID binding')
    production={}
    actions=[];payments=[]
    for index,action in enumerate(case['actions']):
        cut=worlds['action.'+str(index)] if 'action.'+str(index) in worlds else None
        if action['kind'] in ['declare_attackers','declare_blockers']:continue
        require(cut is not None,'actual scripted action observation')
        same(cut['turn'],action['individual_turn'],'action actual turn');same(cut['step'],action['step'],'action actual phase');same(cut['active'],action.get('active_role',action['seat']),'action actual active role')
        if action['kind']=='pass':continue
        operation=operations_by_action[action['policy_id']];out=returns[operation['operation_id']];same(operation['policy'],action,'actual ordinary policy');same(out['policy'],action,'actual ordinary return policy')
        call=entries[out['callback_return_sequence'] and next(r['callback_id'] for r in raw['callbacks'] if r['global_sequence']==out['callback_return_sequence'])]
        same(call['method'],'playChosenSpellAbility','actual ordinary play callback');same(call['operation_id'],None,'outer ordinary callback occurs before internal operation allocation')
        same(call['actor'],action['seat'],'actual play actor');require(out['global_sequence']>operation['global_sequence'],'ordinary return after entry')
        before=operation['context']['players'];after=out['context']['players'];color='U' if action['seat']=='U' else 'G';delta=1 if action['kind']=='activate_mana' else -action['mana_units_paid'] if action['kind']=='cast' else 0
        for role in ['U','G']:
            for mana in MANA:same(integer(after[role]['mana'][mana],'after mana')-integer(before[role]['mana'][mana],'before mana'),delta if role==action['seat'] and mana==color else 0,'actual mana conservation per operation')
        if action['kind']=='activate_mana':
            card=checkpoints[points['action.'+str(index)]['checkpoint_ref']]['cards'][action['alias']];same(card['tapped'],True,'actual mana source tapped');same(card['owner'],action['seat'],'actual mana source belongs to payer')
            production[operation['operation_id']]=dict(source=action['alias'],color=MANA[color],production_operation_id=operation['operation_id'],source_timestamp=card['timestamp'],output_index=0)
        if action['kind']=='cast':
            receipt=cast_rows[action['policy_id']];same(receipt['operation_id'],operation['operation_id'],'cast named operation');same(receipt['source'],action['alias'],'cast physical source');same(receipt['payer'],action['seat'],'cast actual payer');same(receipt['paid_sources'],action['paid_sources'],'cast full named payment sources');same(receipt['required_cost'],action['required_cost'],'cast full ordinary cost')
            same([r['ability']['source'] for r in operation['stack_before']],action['expected_stack_bottom_first'],'actual live stack before admission')
            same(receipt['stack_bottom_first'],action['expected_stack_bottom_first']+[action['alias']],'actual bottom-first admitted stack');same(cut['stack'][-1]['source'],action['alias'],'admitted card is actual top entry')
            same(receipt['actual_ability']['source_timestamp'],receipt['source_timestamp'],'cast actual source timestamp');same(receipt['source_physical_id'],str(raw['physical_aliases'][action['alias']]),'cast physical identity')
            targets=receipt['actual_ability']['targets'];same([target_semantics(t) for t in targets],[] if action.get('target') is None else [action['target']],'exact actual chosen target plan')
            if targets:
                target=targets[0]
                if target['kind']=='object':
                    target_state=checkpoints[points['action.'+str(index)]['checkpoint_ref']]['cards'][target['alias']];same(target_state['zone'],'battlefield','target actual initial battlefield');same(target['physical_id'],str(raw['physical_aliases'][target['alias']]),'target actual physical ID');same(target['timestamp'],target_state['timestamp'],'target actual current incarnation')
                else:
                    live=[r for r in operation['stack_before'] if r['ability']['source']==target['source']];same(len(live),1,'target actual live entry domain');same(target['stack_id'],live[0]['stack_id'],'chosen actual StackEntryId');same(target['ability_id'],live[0]['ability']['ability_id'],'chosen actual ability identity')
            paid=[r for r in raw['callbacks'] if r['kind']=='return' and r['method']=='applyManaToCost' and entries[r['callback_id']]['operation_id']==operation['operation_id']];same(len(paid),1,'exact actual full mana application')
            result=paid[0];entry=entries[result['callback_id']];parent=entries[entry['parent_callback_id']];same(parent['method'],'payManaCost','actual payment callback parent');same(entry['actor'],action['seat'],'payment actual actor');same(entry['arguments']['payer'],action['seat'],'actual payer');same(parent['arguments']['payer'],action['seat'],'parent actual payer');same(parent['arguments']['cost'],action['required_cost'],'actual payment printed cost');same(result['reply']['paid'],True,'actual full paid completion');same(result['reply']['result'],True,'actual payment accepted')
            spent=result['reply']['spent'];same([r['token']['source'] for r in spent],action['paid_sources'],'actual source debit sequence');same(len(spent),action['mana_units_paid'],'actual payment amount');same(result['reply']['paying_mana'],[r['token'] for r in spent],'actual payment list consistency')
            for debit in spent:
                token=debit['token'];require(token['production_operation_id'] in production,'actual production precedes debit');same(token,production[token['production_operation_id']],'actual mana source/operation/incarnation/color provenance')
            payments.append(dict(action_id=action['policy_id'],card=action['alias'],cost=action['required_cost'],payer=action['seat'],paid_sources=action['paid_sources'],amount=str(action['mana_units_paid'])))
        actions.append(dict(action_id=action['policy_id'],kind=action['kind'],card=action['alias'],turn=action['individual_turn'],step=action['step'],actor=action['seat']))
    validate_forge_settlement_events(raw,cast_rows)
    for action,status in raw['settlements'].items():
        same(worlds['settle.'+action]['cards'][cast_rows[action]['source']]['zone'],'battlefield' if 'Creature' in inputs['definitions'][inventory[cast_rows[action]['source']]['name']]['face']['type_line'] and status=='resolved' else 'graveyard','actual settled source zone')
    final=world_projection(raw['final_world'],inventory,raw['physical_aliases'],raw['registered_players'])
    terminal_result=None
    if bounded:
        same(completion['kind'],'bounded','honest bounded completion');same(completion['bounded_ready_exit'],True,'actual bounded callback exit');same(completion['natural_start_game_return'],False,'bounded not natural start return');same(completion['natural_prepared_loop_return'],False,'bounded not natural loop return');same(completion['game_over'],False,'bounded no game over');same(raw['actual_outcome'],None,'bounded has no invented outcome');same(final,worlds['bounded.stop'],'final exact bounded checkpoint')
        for key,actual in [('individual_turn',final['turn']),('step',final['step']),('active_role',final['active']),('priority_holder',final['priority'])]:same(actual,case['stop'][key],'actual bounded '+key)
        same(final['stack'],[],'bounded empty stack')
    else:
        same(completion['kind'],'natural','natural completion');same(completion['game_over'],True,'actual natural game over');same(completion['bounded_ready_exit'],False,'natural no bounded exit')
        same(completion['natural_prepared_loop_return'] if prepared else completion['natural_start_game_return'],True,'actual natural loop return')
        if not prepared:same(completion['game_finished_event'],True,'actual full game finished')
        require(raw['actual_outcome'] is not None,'actual natural outcome')
        outcome=raw['actual_outcome'];same(outcome['draw'],False,'actual not draw');same(outcome['end_reason'],'AllOpponentsLost','ordinary result reason');require(outcome['winning_seat'] in ['U','G'],'winner registered role')
        for role,result in outcome['players'].items():
            won=role==outcome['winning_seat'];same(result['won'],won,'actual won flag');same(result['lost'],not won,'actual lost flag');same(result['game_outcome_is_winner'],won,'actual winner identity flag');same(raw['final_world']['players'][role]['won'],won,'settled winner flag');same(raw['final_world']['players'][role]['lost'],not won,'settled loser flag')
            same(raw['final_world']['header']['players'][role]['lost'],not won,'actual terminal header/world loser flag')
        terminal_result=dict(winning_role=outcome['winning_seat'],losing_roles=sorted(role for role in ['U','G'] if raw['final_world']['players'][role]['lost']))
    actual=actual_reference_facts(raw,case,worlds,inputs,entries,cast_rows)
    return dict(actual_facts=actual,case_id=raw['case_id'],worlds=worlds,final=final,actions=actions,payments=payments,settlements=raw['settlements'],cast_operations=cast_rows,completion_kind=completion['kind'],outcome=raw['actual_outcome'],terminal_result=terminal_result)


def assert_expected_facts(projected:dict[str,Any],raw:dict[str,Any],full_case:dict[str,Any]|None,focus_case:dict[str,Any]|None)->None:
    """Adjudicate selected actual values against immutable primary specifications."""
    final=projected['final']
    if full_case is not None:
        expected=full_case['terminal']
        same(final['turn'],expected['individual_turn'],'frozen terminal turn');same(final['step'],expected['step'],'frozen terminal step');same(projected['outcome']['winning_seat'],expected['winner'],'frozen winner')
        for role in ['U','G']:
            same(final['players'][role]['life'],expected['life'][role],'frozen signed life');same(len(final['players'][role]['library']),expected['library_count'][role],'frozen library count')
            for source,dest in [('hand','hand'),('battlefield','battlefield'),('graveyard_top_first','graveyard'),('library_top_first','library')]:same(final['players'][role][dest],sorted(expected[source][role]) if dest in ['hand','battlefield'] else expected[source][role],'frozen complete terminal '+role+dest)
            same(sorted(a for a in final['players'][role]['battlefield'] if final['cards'][a]['tapped']),sorted(expected['tapped_battlefield'][role]),'frozen terminal tapped domain')
            same(final['players'][role]['mana'],{k:'0' for k in MANA},'frozen terminal mana')
        same(final['stack'],[],'frozen terminal empty stack')
        statuses={a['id']:'resolved' for a in full_case['actions'] if a['kind']=='cast'}
        if full_case['id']=='BI01':
            statuses[next(a['id'] for a in full_case['actions'] if a['kind']=='cast' and a['card']=='G.grizzly-bears.1')]='countered'
            statuses[next(a['id'] for a in full_case['actions'] if a['kind']=='cast' and a['card']=='U.unsummon.1')]='all_targets_illegal_nonresolution'
        else:statuses[next(a['id'] for a in full_case['actions'] if a['kind']=='cast' and a['card']=='U.counterspell.1')]='countered'
        same(projected['settlements'],statuses,'frozen fullgame entry dispositions')
        for combat in full_case['combat']:
            if 'life_after' not in combat:continue
            number=2*combat['own_turn']-(1 if combat['active_role']==full_case['starting_role'] else 0);key='priority.'+str(number)+'.postcombat_main';world=projected['worlds'].get(key,final if number==expected['individual_turn'] else None);require(world is not None,'actual required settled combat cut')
            for role,life in combat['life_after'].items():same(world['players'][role]['life'],life,'frozen intermediate combat life')
        if full_case['id']=='BI01':
            top=next(a for a in full_case['actions'] if a['kind']=='cast' and a['card']=='U.unsummon.2');bottom=next(a for a in full_case['actions'] if a['kind']=='cast' and a['card']=='U.unsummon.1');world=projected['worlds']['settle.'+top['id']];same([r['source'] for r in world['stack']],['U.unsummon.1'],'frozen lower bounce remains after top bounce');same(world['cards']['G.kalonian-tusker.1']['zone'],'hand','frozen original bounce target left battlefield')
        else:
            cancel=next(a for a in full_case['actions'] if a['kind']=='cast' and a['card']=='U.cancel.1');world=projected['worlds']['settle.'+cancel['id']];same([r['source'] for r in world['stack']],['U.unsummon.2'],'frozen original bounce remains after counter-counter');same(world['cards']['G.grizzly-bears.1']['zone'],'battlefield','frozen lower bounce has not executed at top completion')
        recast_aliases=['G.kalonian-tusker.1'] if full_case['id']=='BI01' else ['U.merfolk-of-the-pearl-trident.1','G.grizzly-bears.1']
        for alias in recast_aliases:
            casts=[a for a in full_case['actions'] if a['kind']=='cast' and a['card']==alias];same(len(casts),2,'frozen two ordinary incarnations')
            snapshots=[raw['checkpoints'][next(o['checkpoint_ref'] for o in raw['observations'] if o['key']=='settle.'+cast['id'])]['cards'][alias] for cast in casts]
            require(snapshots[0]['timestamp']!=snapshots[1]['timestamp'],'fresh recast battlefield incarnation');same(snapshots[0]['physical_id'],snapshots[1]['physical_id'],'same physical card across recast');same(snapshots[1]['sick'],True,'fresh own-turn recast is young')
        # Independent eligible domains at every actually offered declaration frame.
        for row in raw['callbacks']:
            if row['kind']=='entry' and row['method']=='declareAttackers':
                recipe=next(c for c in full_case['combat'] if c['active_role']==row['actor'] and c['own_turn']==(row['context']['individual_turn']+1)//2)
                same(sorted(row['arguments']['eligible']),sorted(recipe['expected_eligible_attackers']),'frozen full attacker eligibility')
        if full_case['id']=='BI01':
            div=next(a for a in full_case['actions'] if a.get('card')=='U.divination.1' and a['kind']=='cast')
            settled=projected['worlds']['settle.'+div['id']];same(len(settled['players']['U']['library']),49,'frozen Divination effect library49')
            same(sorted(settled['players']['U']['hand']),sorted(['U.unsummon.1','U.unsummon.2','U.coral-merfolk.1','U.maritime-guard.1','U.island.4','U.island.5']),'frozen Divination resulting hand')
    else:
        require(focus_case is not None,'focused authority required')
        expected=focus_case
        for row in expected['required_settlement_facts']:
            status='resolved' if row['disposition']=='resolved_before_failed_draw_loss' else row['disposition'];same(projected['settlements'][row['cast_action']],status,'frozen required disposition '+row['cast_action'])
        if 'expected_bounded_state' in expected:
            world=expected['expected_bounded_state']
            for role in ['U','G']:
                for source,dest in [('hand','hand'),('battlefield','battlefield'),('graveyard_top_first','graveyard'),('library_top_first','library')]:same(final['players'][role][dest],sorted(world[source][role]) if dest in ['hand','battlefield'] else world[source][role],'frozen bounded full '+role+dest)
                same(final['players'][role]['life'],world['life'][role],'frozen no combat life loss')
                same(sorted(a for a in final['players'][role]['battlefield'] if final['cards'][a]['tapped']),sorted(world['tapped_battlefield'][role]),'frozen bounded tapped domain')
            same([e for e in raw['events'] if e['kind'] in ['GameEventCardDamaged','GameEventPlayerDamaged']],[],'frozen no damage after combat bounce')
        if expected['id'] in ['IF01','IF02','IF03','IF04','IF05','IF06','IF07','IF15']:
            start=expected['setup'];zones={key:{r:list(start[key][r]) for r in ['U','G']} for key in ['hand','battlefield','library_top_first','graveyard_top_first']}
            for action in expected['actions']:
                if action['kind']=='ordinary_paid_hand_cast':zones['hand'][action['actor_role']].remove(action['card'])
            grave_orders={'IF01':{'U':['U.counterspell.1','U.unsummon.1']},'IF02':{'U':['U.cancel.1'],'G':['G.grizzly-bears.1']},'IF03':{'U':['U.counterspell.1','U.divination.1']},'IF04':{'U':['U.cancel.1','U.divination.1']},'IF05':{'U':['U.cancel.1','U.unsummon.1']},'IF06':{'U':['U.unsummon.1','U.counterspell.1','U.cancel.1']},'IF07':{'U':['U.unsummon.1','U.counterspell.2','U.counterspell.1']},'IF15':{'U':['U.counterspell.1','U.counterspell.2','U.unsummon.1']}}
            for role,grave in grave_orders[expected['id']].items():zones['graveyard_top_first'][role]=grave+zones['graveyard_top_first'][role]
            if expected['id'] in ['IF06','IF07']:zones['battlefield']['G'].remove('G.grizzly-bears.1');zones['hand']['G'].append('G.grizzly-bears.1')
            for role in ['U','G']:
                for source,dest in [('hand','hand'),('battlefield','battlefield'),('graveyard_top_first','graveyard'),('library_top_first','library')]:same(final['players'][role][dest],sorted(zones[source][role]) if dest in ['hand','battlefield'] else zones[source][role],'frozen representative full '+role+dest)
                same(final['players'][role]['life'],'20','frozen no direct life effect')
                tapped=sorted({source for action in expected['actions'] if action['kind']=='ordinary_paid_hand_cast' and action['actor_role']==role for source in action['paid_sources']})
                same(sorted(a for a in final['players'][role]['battlefield'] if final['cards'][a]['tapped']),tapped,'frozen full paid source tapped domain')
        if expected['id'] in ['IF09','IF10']:
            action=next(a for a in expected['actions'] if a['kind']=='ordinary_paid_hand_cast');settled=projected['worlds']['settle.'+action['id']];require(bool(settled['combat']),'actual bounced-blocker combat remains');same(settled['combat'][0]['blocked'],True,'blocked status survives blocker departure');same(settled['combat'][0]['blockers'],[],'departed blocker no longer blocks')
        if expected['id'] in ['IF11','IF12']:
            n=1 if expected['id']=='IF11' else 0;same(final['players']['U']['hand'],['U.fugitive-wizard.1'] if n else [],'frozen successful edge draws');same(final['players']['U']['library'],[],'frozen empty library');same(final['players']['U']['graveyard'][0],'U.divination.1','Divination completes before failed draw loss');same(projected['outcome']['winning_seat'],'G','frozen failed draw winner');same(projected['outcome']['players']['U']['loss_reason'],'Milled','frozen actual failed draw loss');same(final['players']['U']['life'],'20','failed draw no life cause')
        if expected['id']=='IF15':
            same(final['players']['U']['graveyard'],['U.counterspell.1','U.counterspell.2','U.unsummon.1'],'frozen stale counter grave order');same(final['cards']['G.grizzly-bears.1']['zone'],'battlefield','frozen no bounce');same(final['players']['G']['hand'],[],'frozen no returned creature');same(sorted(a for a in final['players']['U']['battlefield'] if final['cards'][a]['tapped']),['U.island.'+str(i) for i in range(1,6)],'frozen five U spent Island6 untapped')


@evidence_boundary
def validate_runtime(raw:dict[str,Any],run_dir:Path,commands:dict[str,Any])->None:
    """Join each actually loaded class's copied bytes to its pinned classpath origin."""
    from urllib.parse import urlparse,unquote
    import zipfile
    rows=unique(raw['runtime']['loader_classes'],'class','actual loaded class')
    required={'forge.ai.SpellwakeBlueInteractionReferenceTest','forge.ai.SpellwakeBlueInteractionReferenceTest$StrictController','forge.ai.SpellwakeBlueInteractionReferenceTest$Script','forge.ai.SpellwakeBlueInteractionReferenceTest$StrictCosts','forge.game.ability.effects.CounterEffect','forge.game.ability.effects.ChangeZoneEffect','forge.game.ability.effects.DrawEffect','forge.game.zone.MagicStack','forge.game.player.PlaySpellAbility'}
    require(required<=set(rows),'actual required interaction runtime owners')
    cp={str(Path(row['path']).resolve()):row for row in commands['classpath']}
    for name,row in rows.items():
        member=name.replace('.','/')+'.class';copied=run_dir/'evidence/loader-classes'/member;same(sha(copied),row['sha256'],'actual retained loaded class bytes '+name)
        if row['code_source'] is None:require(name.startswith('java.') and row['resource'].startswith('jrt:'),'declared JDK loaded-class boundary');continue
        source=Path(unquote(urlparse(row['code_source']).path)).resolve()
        if source==(run_dir/'classes').resolve():same(sha(source/member),row['sha256'],'actual isolated producer class origin');continue
        require(str(source) in cp,'actual loaded class has pinned classpath origin');component=cp[str(source)]
        if component['kind']=='directory':
            files={r['path']:r['sha256'] for r in component['files']};require(member in files,'actual loaded directory member pinned');same(files[member],row['sha256'],'actual loaded class vs classpath member pin');same(sha(source/member),row['sha256'],'actual current loaded class origin')
        else:
            same(sha(source),component['sha256'],'actual loaded archive pin')
            resource_member=row['resource'].split('!/',1)[1]
            with zipfile.ZipFile(source) as archive:same(hashlib.sha256(archive.read(resource_member)).hexdigest(),row['sha256'],'actual loaded archive member origin')
    java=read(run_dir/'java-toolchain.json');same(sha(Path(java['path'])),java['sha256'],'actual selected Java executable');same(str(Path(java['path']).parent.parent),raw['runtime']['java_home'],'actual JVM home')
    javac=read(run_dir/'javac-toolchain.json');same(sha(Path(javac['path'])),javac['sha256'],'actual selected Java compiler')



@dataclass(frozen=True)
class OptionalRetainedProducerAuthority:
    """Explicit historical optional wire1 authority, never a live-source waiver."""
    manifest:Path
    case_id:str


def optional_retained_producer_binding(authority:OptionalRetainedProducerAuthority,run_dir:Path)->dict[str,Any]:
    """Bind only the root-reviewed real24 capture epoch, sources and images."""
    require(type(authority) is OptionalRetainedProducerAuthority,'typed optional retained producer authority required')
    same(sha(authority.manifest),'ae6076ddf58a17074db94ba76b84465a852f09c41d302503dbf38a1dd3f22b2f','exact root-qualified retained optional24 manifest')
    manifest=read(authority.manifest);same(manifest['profile'],'optional-payment-v1','retained optional authority profile');same(manifest['forge_commit'],PIN,'retained optional pinned Forge');same(manifest['case_count'],24,'retained exact reduced24 authority')
    for field in ('all_commands_exit_zero','all_source_runtime_resource_stability_checks_pass','all_twenty_four_loaded_class_byte_sets_identical','actual_complete_reduced_inventory'):same(manifest[field],True,'actual qualified retained '+field)
    cases=unique(manifest['cases'],'case_id','qualified retained optional case');same(set(cases),{f'OP-R{n:02}' for n in range(1,25)},'exact retained reduced24 identities');require(authority.case_id in cases,'actual case in qualified retained optional authority');case=cases[authority.case_id]
    same(Path(case['raw']['path']).resolve(),(run_dir/'evidence'/(authority.case_id+'.json')).resolve(),'retained raw output belongs to exact qualified case/run');same(sha(Path(case['raw']['path'])),case['raw']['sha256'],'actual frozen qualified raw bytes');same(Path(case['raw']['path']).stat().st_size,case['raw']['bytes'],'actual frozen qualified raw byte length')
    attempts=unique(manifest['attempts'],'attempt','qualified retained producer attempt');attempt=attempts[case['attempt']];require(authority.case_id in attempt['selected_cases'],'qualified case selected by actual producer attempt')
    for receipt in [case['inputs'],*attempt['receipts'],manifest['reviewed_application'],manifest['writer_contract'],manifest['actual_source_and_class_retention']]:same(sha(Path(receipt['path'])),receipt['sha256'],'actual qualified retained producer receipt/source/class closure')
    for field,value in attempt['source_stability'].items():same(value,True,'actual qualified retained attempt '+field)
    for field in ('compile','run'):same(attempt['commands'][field]['exit'],0,'actual qualified retained producer '+field)
    stability=read(run_dir/'source-stability.json');same(stability['original_source_before'],stability['original_source_after'],'actual qualified original source before/after')
    drift=[]
    for path,digest in stability['original_source_after'].items():
        same(sha(run_dir/'source'/Path(path).name),digest,'actual qualified retained original source copy')
        live=sha(Path(path)) if Path(path).is_file() else None
        if live!=digest:drift.append(dict(path=path,retained_producer_sha256=digest,observed_live_sha256=live))
    return dict(kind='QualifiedRetainedOptionalProducerWire1',manifest=dict(path=str(authority.manifest.resolve()),sha256=sha(authority.manifest)),case=authority.case_id,raw=case['raw'],observed_live_source_drift=drift,current_source_inheritance=False,producer_epoch='real qualified historical wire1 source/class/raw/process closure')


@dataclass(frozen=True)
class OptionalWire2ProducerAuthority:
    """Actual reviewed development wire2 full captures, separate from final26."""
    manifest:Path
    case_id:str


def optional_wire2_producer_binding(authority:OptionalWire2ProducerAuthority,run_dir:Path)->dict[str,Any]:
    """Admit the exact actual corrected full producer/source/class epoch."""
    require(type(authority) is OptionalWire2ProducerAuthority,'typed actual optional wire2 producer authority')
    same(sha(authority.manifest),'a4c78697ce1e726226946a95cf6d922b2860bfb89c36e85cd6a30bcdbb3bf8d3','exact reviewed actual full wire2 capture manifest');manifest=read(authority.manifest);same(manifest['optional_schema_version'],2,'actual producer wire2 epoch');same(manifest['forge_commit'],PIN,'actual corrected producer pinned Forge');same(manifest['compile_and_run_exit'],dict(compile=0,run=0),'actual full wire2 compiler/runtime exits')
    cases=unique(manifest['cases'],'case_id','actual corrected full case');same(set(cases),{'OP-FG01','OP-FG02'},'actual corrected two full cases');require(authority.case_id in cases,'actual case in corrected wire2 authority');case=cases[authority.case_id]
    same(Path(case['raw']['path']).resolve(),(run_dir/'evidence'/(authority.case_id+'.json')).resolve(),'actual wire2 raw/run source binding')
    for field,value in manifest['source_stability'].items():same(value,True,'actual corrected wire2 source/class/runtime stability '+field)
    for receipt in [case['raw'],manifest['source_java'],manifest['source_python'],*manifest['input_bindings'].values(),manifest['actual_commands'],manifest['launcher_entry'],manifest['launcher_return'],manifest['actual_process_record'],manifest['source_class_image_process_wire_audit']]:
        same(sha(Path(receipt['path'])),receipt['sha256'],'actual corrected wire2 immutable receipt/source/class/raw closure');same(Path(receipt['path']).stat().st_size,receipt['bytes'],'actual corrected wire2 receipt length')
    same(manifest['source_java']['sha256'],'540e13db558027b239dcd1dfe676d442993fb43331af935a88e4404fb9aed080','actual corrected Java wire2 class source');same(manifest['source_python']['sha256'],'c455502b0b21d0f07d8d19b7777e691103b7aa51b55e41146d1057cf11836ccd','actual maintained Python writer source')
    return dict(kind='ActualDevelopmentOptionalProducerWire2',manifest=dict(path=str(authority.manifest.resolve()),sha256=sha(authority.manifest)),raw=case['raw'],java_source=manifest['source_java'],python_source=manifest['source_python'],current_image26_credit=False,native_epoch_inheritance=False)


@evidence_boundary
def validate_reference_run(run_dir:Path,*,retained_optional:OptionalRetainedProducerAuthority|None=None)->dict[str,Any]:
    """Validate the one maintained compiler/process/source/runtime guard protocol."""
    if retained_optional is not None:optional_retained_producer_binding(retained_optional,run_dir)
    stability=read(run_dir/'source-stability.json');same(stability['unchanged'],True,'actual producer source stable');same(stability['classpath_unchanged'],True,'actual classpath stable');same(stability['classes_unchanged'],True,'actual producer classes stable');same(stability['resources_unchanged'],True,'actual selected Forge resources stable');same(stability['retained_resources_unchanged'],True,'actual retained resources stable')
    for before,after in [('before','after'),('classes_before','classes_after'),('resources_before','resources_after'),('retained_resources_before','retained_resources_after')]:same(stability[before],stability[after],'actual reference pre/post '+before+' equality')
    same(read(run_dir/'commands.json')['source_before'],stability['before'],'actual reference command/source guard binding');same(read(run_dir/'classes-before-run.json'),stability['classes_before'],'actual compiled classes before run binding')
    for relative,digest in stability['after'].items():same(sha(run_dir/relative),digest,'current retained producer source '+relative)
    for relative,digest in stability['classes_after'].items():same(sha(run_dir/relative),digest,'current retained producer class '+relative)
    for path,digest in stability['resources_after'].items():same(sha(Path(path)),digest,'current actual selected Forge resource')
    for path,digest in stability['retained_resources_after'].items():same(sha(run_dir/path),digest,'current retained selected Forge resource')
    results=read(run_dir/'results.json');same(results['compile']['exit'],0,'actual compile success');same(results['run']['exit'],0,'actual behavioral process success')
    if 'original_source_before' in stability:
        same(stability['original_source_unchanged'],True,'actual original reference helper sources stable');same(stability['original_source_before'],stability['original_source_after'],'actual original helper source pre/post equality')
        for path,digest in stability['original_source_after'].items():
            if retained_optional is None:same(sha(Path(path)),digest,'actual current helper source bytes')
            same(sha(run_dir/'source'/Path(path).name),digest,'actual copied helper source bytes')
    elif retained_optional is not None:raise EvidenceError('qualified retained optional producer requires original source before/after guards')
    return read(run_dir/'commands.json')


@evidence_boundary
def compare_reference(run_dir:Path,selected:list[str]|None=None)->dict[str,Any]:
    """Check actual closed reference capture; no native agreement is inferred."""
    inputs=read(run_dir/'inputs.json');manifest=read(run_dir/'manifest.json');full=read(run_dir/'cases-v1.json');focused=read(run_dir/'focused-cases-v3.json')
    for relative,digest in AUTHORITY.items():same(sha(run_dir/Path(relative).name),digest,'immutable authority '+relative)
    validate_reference_run(run_dir)
    cases=unique(inputs['cases'],'case_id','frozen case');names=selected or list(cases)
    if selected is None:same(set(p.stem for p in (run_dir/'evidence').glob('*.json')),set(cases),'exact15 raw case inventory')
    reports={}
    for name in names:
        raw=read(run_dir/'evidence'/f'{name}.json');same(raw['inputs_sha256'],sha(run_dir/'inputs.json'),'actual input digest');validate_runtime(raw,run_dir,read(run_dir/'commands.json'))
        projected=forge_projection(raw,cases[name],full,manifest,inputs)
        assert_expected_facts(projected,raw,next((c for c in full['cases'] if c['id']==name),None),next((c for c in focused['scenarios'] if c['id']==name),None))
        reports[name]=dict(completion=projected['completion_kind'],actions=len(projected['actions']),casts=len(projected['cast_operations']),worlds=len(projected['worlds']),raw_sha256=sha(run_dir/'evidence'/f'{name}.json'))
    return dict(status='reference_vs_frozen_primary_checked_native_join_pending',cases=reports,case_count=len(reports),native_numeric_agreement_claim=False,reader_sha256=sha(Path(__file__)))


@evidence_boundary
def actual_reference_facts(raw:dict[str,Any],case:dict[str,Any],worlds:dict[str,Any],inputs:dict[str,Any],entries:dict[int,Any],cast_rows:dict[str,Any])->dict[str,Any]:
    """Check real declarations, immutable targets, draw order, prepared prefixes and shuffle receipts."""
    allowed={'schema','schema_version','case_id','reference','forge_commit','inputs_sha256','fixture','paper_cards','actual_outcome','physical_aliases','registered_players','callbacks','events','random','operations','cast_operations','settlement_events','settlements','failures','observations','checkpoints','runtime','completion','final_world'}
    same(set(raw),allowed,'raw exact supported field set, no fabricated native observations')
    by_ability={r['ability_id']:r for r in cast_rows.values()}
    for checkpoint in raw['checkpoints'].values():
        for live in checkpoint['stack']:
            ability=live['ability'];require(ability['ability_id'] in by_ability,'actual stack entry belongs to a named cast');captured=by_ability[ability['ability_id']]['actual_ability']['targets']
            same([target_semantics(t) for t in ability['targets']],[target_semantics(t) for t in captured],'live entry retains its originally captured target, including disappeared target')
            for current,original in zip(ability['targets'],captured,strict=True):
                if current['kind']=='stack':
                    same(current['ability_id'],original['ability_id'],'captured original target ability identity')
                    if current['stack_id'] is not None:same(current['stack_id'],original['stack_id'],'captured original live target StackEntryId')
    frames=[]
    for row in raw['callbacks']:
        if row['kind']!='return' or row['method'] not in ['declareAttackers','declareBlockers']:continue
        entry=entries[row['callback_id']];number=entry['context']['individual_turn'];recipe=next(c for c in case['combat'] if c['individual_turn']==number)
        if row['method']=='declareAttackers':
            same(row['reply']['selected'],recipe['attackers'],'actual chosen attacker input');same(sorted(entry['arguments']['eligible']),sorted(recipe['expected_eligible_attackers']),'actual complete eligible attacker domain')
            frames.append(dict(kind='declare_attackers',turn=number,actor=row['actor'],domain=sorted(entry['arguments']['eligible']),selected=sorted(row['reply']['selected'])))
        else:
            selected=sorted(row['reply']['selected'],key=lambda b:b['blocker']);same(selected,sorted(recipe['blockers'],key=lambda b:b['blocker']),'actual chosen blockers')
            frames.append(dict(kind='declare_blockers',turn=number,actor=row['actor'],domain=sorted(entry['arguments']['available_creatures']),selected=selected))
    if case['initialization']['kind']=='opening':
        plans={r['operation_id']:r['target_aliases'] for r in case['shuffle_operations']}
        for operation in case['forge_operation_queue']:
            if operation['role']=='initial_shuffle':
                commit=[r for r in raw['random'] if r['kind']=='operation_commit' and r['operation_id']==operation['operation_id']];same(len(commit),1,'actual unique shuffle commit');same(commit[0]['actual_committed'],plans[operation['operation_id']],'actual full60 shuffle permutation')
                same(worlds['setup.predraw']['players'][operation['planned_owner']]['library'],plans[operation['operation_id']],'actual predraw shuffled library')
                same(worlds['opening.complete']['players'][operation['planned_owner']]['hand'],sorted(plans[operation['operation_id']][:7]),'actual seven-card kept opener')
        same(len([r for r in raw['callbacks'] if r['kind']=='return' and r['method']=='mulliganKeepHand']),2,'actual keep declarations')
    else:
        setup=case['initialization']['setup'];initialized=next(r for r in raw['operations'] if r['kind']=='prepared_initialized');same(initialized['global_battlefield_order'],setup['initial_battlefield_order'],'actual global prepared battlefield order')
        initial=raw['checkpoints'][next(r['checkpoint_ref'] for r in raw['observations'] if r['key']=='prepared.initialized')]
        timestamps=[integer(initial['cards'][a]['timestamp'],'initial BF timestamp') for a in setup['initial_battlefield_order']];require(all(a<b for a,b in zip(timestamps,timestamps[1:])),'actual BF timestamps follow exact global authored order')
        if setup['step']=='untap':
            began=[e for e in raw['events'] if e['kind']=='GameEventTurnBegan'];same(len(began),1,'actual real turn-begin event');same(began[0]['payload']['turnNumber'],setup['individual_turn'],'actual numbered turn begin');same(began[0]['payload']['turnOwner'],setup['active_role'],'actual turn-begin active')
            same(worlds['prepared.first_ready']['step'],'upkeep','actual first Ready after Untap');same(worlds['prepared.first_ready']['priority'],setup['active_role'],'actual first Ready holder')
            draws=[e for e in raw['events'] if e['kind']=='GameEventCardChangeZone' and e['payload']['from'] is not None and e['payload']['from']['zone_type']=='library' and e['payload']['to']['zone_type']=='hand' and e['context']['step']=='draw']
            same([e['payload']['card']['alias'] for e in draws],[setup['library_top_first'][setup['active_role']][0]],'actual one mandatory prepared prefix draw')
    draw_sequences={}
    for event in raw['events']:
        if event['kind']!='GameEventCardChangeZone':continue
        payload=event['payload'];card=payload['card'];same(card['physical_id'],str(raw['physical_aliases'][card['alias']]),'actual event physical alias binding')
        if payload['from'] is None or payload['to'] is None or payload['from']['zone_type']!='library' or payload['to']['zone_type']!='hand' or event['context']['step']!='precombat_main':continue
        resolving=payload['resolving_abilities'];same(len(resolving),1,'actual draw has exactly one resolving spell cause');ability=resolving[0];require(ability['ability_id'] in by_ability,'actual draw cause named entry');receipt=by_ability[ability['ability_id']];same(receipt['source'],'U.divination.1','actual effect draw comes from Divination');require(any(f['class']=='forge.game.ability.effects.DrawEffect' and f['method']=='resolve' for f in payload['actual_stack_frames']),'actual effect draw source path')
        draw_sequences.setdefault(receipt['action_id'],[]).append(dict(alias=card['alias'],sequence=event['global_sequence']))
    for action_id,receipt in cast_rows.items():
        if receipt['source']!='U.divination.1':continue
        values=draw_sequences.get(action_id,[])
        expected=[] if raw['settlements'][action_id]=='countered' or case['case_id']=='IF12' else ['U.fugitive-wizard.1'] if case['case_id']=='IF11' else ['U.unsummon.2','U.maritime-guard.1']
        same([v['alias'] for v in values],expected,'actual ordered successful Divination draws')
        if values:
            require(not any(c['kind']=='entry' and c['method']=='chooseSpellAbilityToPlay' and values[0]['sequence']<c['global_sequence']<values[-1]['sequence'] for c in raw['callbacks']),'no actual priority callback between sequential draws')
        if case['case_id'] in ['IF11','IF12']:
            resolved=next(e['event_sequence'] for e in raw['settlement_events'] if e['action_id']==action_id and e['raw_event']=='GameEventSpellResolved');exits=[e['global_sequence'] for e in raw['events'] if e['kind']=='GameEventCardChangeZone' and e['payload']['card']['alias']==receipt['source'] and e['payload']['to']['zone_type']=='graveyard'];same(len(exits),1,'actual completed Divination graveyard exit');outcome=next(e['global_sequence'] for e in raw['events'] if e['kind']=='GameEventGameOutcome');require(resolved<exits[0]<outcome,'actual Divination completion/exit precedes failed-draw loss')
    for key,world in worlds.items():
        if key.startswith('settle.'):
            for role in ['U','G']:same(world['players'][role]['mana'],{k:'0' for k in MANA},'actual settled paid costs remain spent')
            if world['priority'] is not None:same(world['priority'],world['active'],'actual active player receives priority after resolution')
    damage=ordinary_forge_damage(raw)
    return dict(damage=sorted(damage,key=lambda r:(r['turn'],r['source'],r['target'])),declaration_frames=frames,draw_sequences={a:[v['alias'] for v in values] for a,values in draw_sequences.items()})



@evidence_boundary
def validate_registry(registry:dict[str,Any],manifest:dict[str,Any],profile:str=PROFILES[0])->None:
    """Bind actual registry6 rows to every immutable saved identity/signature and program family."""
    if profile==PROFILES[1]:
        same(registry['registryVersion'],7,'actual mandatory registry7');same(registry['sourceRecords'],39196,'actual full source record count');same(registry['requiredPartialBindings'],19,'selected19 partial bindings');same(registry['requiredRecords'],19,'required19 source definitions');same(registry['requiredWithoutBinding'],0,'zero missing source bindings');same(registry['fullyQualifiedCards'],0,'zero global full qualifications');same(registry['behaviorEvidence'],'not-evaluated-by-registry','registry admission scope');same(registry['sourceSha256'],manifest['catalogue']['sha256'],'actual retained catalogue');same(registry['creatureVocabularyId'],manifest['vocabulary']['id'],'actual retained vocabulary')
        rows=unique(registry['cards'],'oracleId','registry7 source identity');same(set(rows),{card['oracleId'] for card in manifest['cards']},'exact19 mandatory registry identities')
        for card in manifest['cards']:
            row=rows[card['oracleId']];same(row['name'],card['name'],'actual registry7 source name');same(row['required'],True,'actual required source');same(row['behaviorEvidence'],'not-evaluated-by-registry','actual binding qualification boundary');binding=row['binding'];same(binding['semanticSha256'],card['registrySignatureV1Sha256'],'actual whole source/Program signature');same(binding['status'],'partial','all19 bindings remain Partial');same(binding['scope'],'mandatory-trigger-ordinary-card-v1','finite mandatory scope')
            contract=card['programContract'];family='basic-land' if contract['land_subtypes'] else 'triggered-creature' if contract['triggered'] else 'token-creation-spell' if card['name']=='Raise the Alarm' else 'bounce-creature' if card['name']=='Unsummon' else 'counter-spell' if card['name']=='Counterspell' else 'vanilla-creature';same(binding['program'],'mandatory-'+family+'-v1','actual exact finite mandatory ProgramKey')
        return
    same(profile,PROFILES[0],'strict registry profile')
    same(registry['registryVersion'],6,'actual interaction registry6');same(registry['sourceRecords'],39196,'actual full catalogue count');same(registry['requiredPartialBindings'],20,'selected20 partial rows');same(registry['fullyQualifiedCards'],0,'no global qualification uplift')
    same(registry['sourceSha256'],manifest['provenance']['compressedSha256'],'actual catalogue bytes');same(registry['creatureVocabularyId'],manifest['provenance']['creatureVocabularyId'],'actual vocabulary')
    rows=unique(registry['cards'],'oracleId','registry Oracle row');same(set(rows),{c['oracleId'] for c in manifest['cards']},'all20 exact registry identities')
    for card in manifest['cards']:
        row=rows[card['oracleId']];same(row['name'],card['name'],'registry name');same(row['required'],True,'registry required');binding=row['binding'];same(binding['semanticSha256'],card['registrySignatureV1Sha256'],'registry semantic signature');same(binding['scope'],'interaction-ordinary-card-v1','registry finite scope');same(binding['status'],'partial','registry honest partial status')
        name=card['name'];family='basic-land' if name in ['Island','Forest'] else 'bounce-creature' if name=='Unsummon' else 'counter-spell' if name in ['Counterspell','Cancel'] else 'draw-two' if name=='Divination' else 'vanilla-creature'
        same(binding['program'],'interaction-'+family+'-v1','registry exact authored program')


@evidence_boundary
def unwrap_native(document:dict[str,Any],allow_uncaptured:bool,manifest:dict[str,Any],profile:str=PROFILES[0])->tuple[dict[str,Any],dict[str,Any]]:
    """Require the real current9/witness7 interaction envelope; development opt-in stays explicit."""
    require(profile in PROFILES,'strict finite native profile');require(not allow_uncaptured or profile==PROFILES[0],'mandatory final reading forbids uncaptured reports');registry_version=7 if profile==PROFILES[1] else 6
    if profile==PROFILES[1]:
        require(not allow_uncaptured,'mandatory full capture required');index=MandatoryWitnessIndex();index.observe_report(document['observations']);witness=document['type_context_witness'];same(set(witness),{'calls','coverage','registry','schema','snapshots','source_inputs','vocabulary'},'exact actual mandatory witness fields')
        for call in witness['calls']:index.add_call(call)
        for point in witness['snapshots']:index.add_point(point)
        return index.finish(document,witness,manifest)
    require('capture_failure' not in document,'actual capture_failure')
    if document.get('profile')==('mandatory-trigger' if profile==PROFILES[1] else 'interaction'):
        same(document['schema_version'],9,'current outer schema9');witness=document['type_context_witness'];same(witness['schema'],'spellwake.current-type-context-witness.v7','actual witness7');same(witness['registry']['tag'],'Registry','actual Registry witness');validate_registry(witness['registry']['coverage'],manifest,profile)
        coverage=witness['coverage'];same(coverage['complete'],True,'complete current witness');same(coverage['uncovered'],[],'no uncovered witness fields');points=unique(witness['snapshots'],'point_id','witness point');calls=unique(witness['calls'],'call_id','witness call')
        same(set(coverage['required_points']),set(points),'actual complete point inventory');same(len(coverage['required_points']),len(points),'no duplicate required points');same(set(coverage['required_calls']),set(calls),'actual complete call inventory');same(len(coverage['required_calls']),len(calls),'no duplicate required calls')
        for call_id,call in calls.items():
            for field in ['before_point','after_point']:
                if call[field] is not None:require(call[field] in points,'actual call references known '+field)
            require(type(call['adopted']) is bool,'actual call adoption flag')
            if call['adopted']:require(call['after_point'] is not None,'actual adopted result has successor point')
            replay=call['replay'];same(replay['full_result_eq'],True,'actual witness full native Result rerun');same(replay['full_record_eq'],True,'actual witness full projected record rerun');same(replay['input_storage'],dict(tag='SavedTypedCall',call_id=call_id),'actual saved typed-call identity')
            initializer=call['before_point'] is not None and points[call['before_point']]['phase']=='InitializeInput';same(replay['source_eq'],None if initializer else True,'actual retained native source equality');same(replay['source_snapshot_before_invocation'],'InputOnlyForInitializer' if initializer else True,'actual source snapshot timing')
        joins=unique(coverage['observation_to_points'],'pointer','witness pointer')
        for path,join in joins.items():
            pointer(document,path)
            for point_id in join['points']:require(point_id in points and path in points[point_id]['observation_pointers'],'actual pointer/point reciprocal binding')
        for point_id,point in points.items():
            require(bool(point['observation_pointers']),'unbound actual point')
            for path in point['observation_pointers']:require(path in joins and point_id in joins[path]['points'],'actual complete point join')
            if point['projection']['tag']=='RecordPointer':pointer(document,point['projection']['pointer'])
        for path in coverage['record_pointers']:pointer(document,path)
        sources=unique(witness['source_inputs'],'path','witness source input')
        if profile==PROFILES[1]:same(len(sources),34,'actual complete mandatory source inventory count')
        for source in sources.values():require(re.fullmatch('[0-9a-f]{64}',source['sha256']) is not None and source['bytes']>0,'witness actual source receipt')
        inner=document['observations'];provenance=dict(captured=True,complete=True,points=len(points),calls=len(calls),registry_version=registry_version)
    else:
        require(allow_uncaptured,'development report requires explicit --allow-uncaptured')
        require('profile' not in document and 'observations' not in document,'foreign outer profile')
        inner=document;provenance=dict(captured=False,complete=False,qualification=False)
    same(inner['status'],'success','native successful actual report');report=inner['data'];same(report['schema_version'],1,'interaction collection schema');same(report['profile'],profile,'actual finite native profile');same(report['registry_version'],registry_version,'actual native registry epoch');same(report['catalogue_sha256'],manifest['catalogue']['sha256'] if profile==PROFILES[1] else manifest['provenance']['compressedSha256'],'actual native catalogue')
    if profile==PROFILES[1]:same(set(report),{'schema_version','profile','registry_version','catalogue_sha256','runs'},'exact mandatory aggregate members')
    unique(report['runs'],'fixture_ref','native case');return report,provenance


def native_role(value:Any)->str:
    require(str(value) in ROLES,'unknown native seat');return ROLES[str(value)]


@evidence_boundary
def ordinary_native_combat(header:dict[str,Any])->list[dict[str,Any]]:
    """The maintained combat observer shared by old and optional report epochs."""
    combat=[]
    if header['combat']:
        for row in header['combat']['attackers']:
            attacker=row['combatant']['object']['alias'];blockers=[b['combatant']['object']['alias'] for b in header['combat']['blockers'] if b['attacker']['alias']==attacker]
            combat.append(dict(attacker=attacker,blocked=row['blocked']=='blocked',blockers=sorted(blockers)))
    return sorted(combat,key=lambda row:row['attacker'])


def ordinary_forge_damage(raw:dict[str,Any])->list[dict[str,Any]]:
    """Observe actual positive damage notifications, with no derived game effects."""
    damage=[]
    for event in raw['events']:
        if event['kind'] in ['GameEventCardDamaged','GameEventPlayerDamaged'] and integer(event['payload']['amount'],'actual dealt damage')>0:
            p=event['payload'];damage.append(dict(turn=event['context']['individual_turn'],source=p['source']['alias'],target=p['card']['alias'] if event['kind']=='GameEventCardDamaged' else p['target'],amount=p['amount']))
    return sorted(damage,key=lambda r:(r['turn'],r['source'],r['target']))


def optional_declaration_damage_join(native:dict[str,Any],raw:dict[str,Any],case:dict[str,Any],entries:dict[int,Any])->dict[str,Any]:
    """Join actual declaration publications, issued domains and damage notifications."""
    declarations={};damage=[];history=native['history'];rows=native['rows']
    def alias(value:dict[str,Any]|str)->str:return history[value] if isinstance(value,str) else value['alias'] if 'alias' in value else history[value['object']]
    for row in rows:
        for event in row['current_facts']['rules_journal']:
            if event['tag']!='Event':continue
            for occurrence in event['occurrences']:
                value=occurrence['value'];tag=value['tag']
                if tag in ('AttackersDeclared','BlockersDeclared'):
                    turn=row['header']['turn']['position']['number'];key=(tag,turn);require(key not in declarations,'one actual two-player declaration per turn/family');declarations[key]=dict(value=value,row=row)
                elif tag=='DamageDealt':
                    for applied in value['damage']:
                        if integer(applied['amount'],'actual positive native dealt damage')<=0:continue
                        source=history[applied['source']['object']['object']];target=applied['target'];recipient=mandatory_role(target['player']) if target['kind']=='player' else history[target['object']]
                        turn=value['origin']['position']['number'] if value['origin']['kind']=='combat' else row['header']['turn']['position']['number'];damage.append(dict(turn=turn,source=source,target=recipient,amount=applied['amount']))
    forge_events={('AttackersDeclared' if event['kind']=='GameEventAttackersDeclared' else 'BlockersDeclared',event['context']['individual_turn']):event for event in raw['events'] if event['kind'] in ('GameEventAttackersDeclared','GameEventBlockersDeclared')};same(set(declarations),set(forge_events),'complete actual native/Forge declaration publication inventory')
    for key,actual in declarations.items():
        value=actual['value'];event=forge_events[key];payload=event['payload']
        if key[0]=='AttackersDeclared':
            selected=sorted((alias(v['attacker']),mandatory_role(v['defender'])) for v in value['selected']);right=sorted((v['attacker']['alias'],v['defender']['seat']) for v in payload['attackersMap']);same(selected,right,'actual complete native/Forge declared attackers and defenders')
        else:
            selected=sorted((alias(v['attacker']),alias(v['blocker'])) for v in value['selected']);right=sorted((v['attacker']['alias'],v['value']['alias']) for v in payload['blockers'] if not v['unblocked_self_sentinel']);same(selected,right,'actual complete native/Forge declared blockers')
    returns={r['callback_id']:r for r in raw['callbacks'] if r['kind']=='return'};domains=0
    for call in entries.values():
        if call['method'] not in ('declareAttackers','declareBlockers'):continue
        family='AttackersDeclared' if call['method']=='declareAttackers' else 'BlockersDeclared';actual=declarations[(family,call['context']['individual_turn'])];value=actual['value'];row=actual['row'];same(value['origin'],'PlayerReply','actual issued Forge declaration has actual native PlayerReply');same(row['input']['command']['kind'],'reply','actual native declaration reply owner');before=rows[row['ordinal']-1];frame=before['header']['pending']['frame']['frame'];same(mandatory_role(row['input']['actor']),call['actor'],'actual declaration callback/native chooser')
        if family=='AttackersDeclared':same(sorted(alias(v) for v in frame['eligible_attackers']),sorted(call['arguments']['eligible']),'complete actual native/Forge eligible attacker domain')
        else:same(sorted(alias(v) for v in frame['eligible_blockers']),sorted(call['arguments']['available_creatures']),'complete actual native/Forge blocker domain')
        domains+=1
    retirements=optional_retirement_join(raw,case,entries)
    for retired in retirements:
        event=next(e for e in raw['events'] if e['global_sequence']==retired['actual_event_sequence']);actual=declarations[('BlockersDeclared',event['context']['individual_turn'])];same(actual['value']['origin'],'ForcedEmpty','unissued Forge blocker slot joins actual native ForcedEmpty publication');same(actual['value']['selected'],[],'actual forced empty native blocker declaration');row=actual['row'];require(not(row['input']['command']['kind']=='reply' and row['input']['command']['choice']['kind']=='declare_blockers'),'no fabricated native blocker reply for unissued Forge slot')
    same(sorted(damage,key=lambda r:(r['turn'],r['source'],r['target'])),ordinary_forge_damage(raw),'complete actual native/Forge positive damage publications')
    return dict(declaration_publications=len(declarations),issued_declaration_domains=domains,forced_empty_retirement_joins=len(retirements),damage_publications=len(damage))


def native_world_projection(cards:dict[str,Any],piles:list[dict[str,Any]],header:dict[str,Any],objects:dict[str,str],inventory:dict[str,Any],entry_history:dict[str,str])->dict[str,Any]:
    """Project actual deltas/full snapshots; raw native object and entry bindings stay separately checked."""
    same({c['alias'] for c in cards.values()},set(inventory),'actual native120 physical inventory')
    players={r:dict(life=header['life'][seat],mana={short:header['mana'][seat][long] for short,long in MANA.items()},library=[],hand=[],battlefield=[],graveyard=[],exile=[]) for seat,r in ROLES.items()};out={}
    for card in cards.values():
        obj=card['object'];alias=card['alias'];objects[obj['object']]=alias;zone=obj['zone']['kind'];owner=native_role(obj['owner']);same(owner,inventory[alias]['role'],'actual native card owner')
        if zone in ['hand','battlefield','exile']:players[owner][zone].append(alias)
        creature=card['creature'];counters=card['counters'];out[alias]=dict(owner=owner,controller=native_role(obj['controller']) if obj['controller'] is not None else None,zone=zone,tapped=obj['tapped'],power=creature['power'] if creature else None,toughness=creature['toughness'] if creature else None,marked_damage=creature['marked_damage'] if creature else None,sick=not obj['continuous_control_since_turn_start'] if creature else None,plus_one_counters=counters['plus_one'] if creature else None,minus_one_counters=counters['minus_one'] if creature else None)
    for pile in piles:players[native_role(pile['pile']['owner'])][pile['pile']['kind']]=[r['alias'] for r in pile['objects_top_to_bottom']]
    for player in players.values():
        for key in ['hand','battlefield','exile']:player[key].sort()
    rich=header['interaction_execution'];require(rich is not None and rich['resolution'] is None,'selected Ready frames have no hidden active resolution')
    flat=header['stack'];same([r['entry'] for r in rich['stack']],[r['entry'] for r in flat],'canonical rich and flat entry identity/order')
    entry_sources={r['entry']:r['kind']['card']['alias'] for r in flat};entry_history.update(entry_sources);stack=[]
    for raw,canonical in zip(flat,rich['stack'],strict=True):
        same(raw['kind']['kind'],'card_spell','ordinary actual physical spell');same(canonical['kind']['tag'],'CardSpell','canonical actual physical spell');same(canonical['kind']['object'],raw['kind']['card']['object'],'canonical actual source object');same(objects[canonical['kind']['object']],raw['kind']['card']['alias'],'actual stack source alias')
        same(canonical['controller'],raw['controller'],'canonical actual controller');same(len(raw['targets']),len(canonical['targets']),'canonical actual target count');targets=[]
        for target,rich_target in zip(raw['targets'],canonical['targets'],strict=True):
            same(target['clause'],rich_target['clause'],'canonical target clause');t=target['target'];rt=rich_target['target']
            if t['kind']=='card':same(rt['tag'],'Object','canonical object target tag');same(rt['object'],t['object']['object'],'canonical selected object ID');same(objects[rt['object']],t['object']['alias'],'actual target physical alias');targets.append(dict(kind='object',card=t['object']['alias']))
            else:same(t['kind'],'stack','selected stack target kind');same(rt['tag'],'Stack','canonical stack target tag');same(rt['entry'],t['entry'],'canonical selected live/stale entry ID');require(t['entry'] in entry_history,'stack target has known actual entry history');targets.append(dict(kind='stack',spell=entry_history[t['entry']]))
        stack.append(dict(source=raw['kind']['card']['alias'],targets=targets))
    seen=[]
    for role,player in players.items():
        for zone in ['library','hand','battlefield','graveyard','exile']:
            same(len(player[zone]),header['counts_by_owner']['41' if role=='U' else '3'][zone],'native header/pile count')
            for alias in player[zone]:same(out[alias]['zone'],zone,'native actual pile/card zone')
            seen+=player[zone]
        seen +=[r['source'] for r in stack if inventory[r['source']]['role']==role]
    same(set(seen),set(inventory),'native all120 actual membership');same(len(seen),120,'native no duplicate actual membership')
    combat=ordinary_native_combat(header)
    position=header['turn']['position'] if header['turn'] else None
    return dict(turn=position['number'] if position else 0,step=position['step'] if position else None,active=native_role(header['active']),priority=native_role(header['priority_holder']) if header['priority_holder'] is not None else None,players=players,cards=out,stack=stack,combat=sorted(combat,key=lambda row:row['attacker']))


# Independently decoded from the actual source-qualified D06 bytes dd16c29d...cd3c, not native output.
VOCABULARY_ORDINALS = {'Bear': '25', 'Beast': '26', 'Centaur': '43', 'Crab': '53', 'Elemental': '83', 'Human': '130', 'Merfolk': '165', 'Soldier': '265', 'Warrior': '309', 'Wizard': '314', 'Wurm': '320'}


@evidence_boundary
def validate_native_definitions(definitions:list[dict[str,Any]],manifest:dict[str,Any])->None:
    """Check all20 actual compiled programs/printing identities against the saved source and D06."""
    rows=unique(definitions,'oracle_id','actual native definition');same(set(rows),{c['oracleId'] for c in manifest['cards']},'all20 actual native definitions')
    for card in manifest['cards']:
        row=rows[card['oracleId']];name=card['name'];face=card['faces'][0]
        for field,value in [('reference',card['oracleId']),('name',name),('printing_id',card['scryfallId']),('set',card['sourcePrinting']['set']),('collector',card['sourcePrinting']['collectorNumber']),('language',card['sourcePrinting']['language'])]:same(row[field],value,'actual native definition source '+field)
        require(any(r['sha256']==manifest['provenance']['compressedSha256'] and r['path']==manifest['provenance']['sourcePath'] for r in row['sources']),'actual definition source-qualified catalogue receipt')
        program=row['program'];land=name in ['Island','Forest'];creature=face['type_line'].startswith('Creature');same(program['names'],[name],'actual compiled name');same(program['card_types'],['Land' if land else 'Creature' if creature else face['type_line']],'actual compiled card type');same(program['supertypes'],['Basic'] if land else [],'actual compiled supertypes');same(program['land_subtypes'],[name] if land else [],'actual compiled land subtype')
        subtypes=sorted([dict(ordinal=VOCABULARY_ORDINALS[n],vocabulary=manifest['provenance']['creatureVocabularyId']) for n in card['creatureSubtypes']],key=lambda n:int(n['ordinal']));same(program['creature_subtypes'],subtypes,'actual exact D06 creature subtype members')
        for field in ['activated','triggered','mana']:same(program[field],[],'no extra authored '+field)
        for field in ['continuous','color_indicator','effect_defined_colors']:same(program[field],None,'no extra selected '+field)
        kind=program['kind'];same(kind['tag'],'Permanent' if land or creature else 'Spell','actual complete program class')
        if land:same(kind['casting_cost'],None,'actual land has no casting cost')
        else:
            expected=cost_tuple(face['mana_cost']);same(kind['casting_cost'],dict(generic=expected['generic'],required={k.capitalize():value for k,value in expected['required'].items()}),'actual full printed cast cost')
        if land or creature:same(kind['printed_pt'],dict(power=dict(tag='Number',value=face['power']),toughness=dict(tag='Number',value=face['toughness'])) if creature else None,'actual printed creature PT')
        else:
            expected_body=dict(instructions=[dict(op='ReturnToOwnersHand')],target=dict(tag='SingleCreature')) if name=='Unsummon' else dict(instructions=[dict(op='CounterSpell')],target=dict(support_gate='AnySpell',tag='SingleSpell')) if name in ['Counterspell','Cancel'] else dict(instructions=[dict(amount=dict(tag='Literal',value='2'),op='DrawController')],target=dict(tag='None'))
            same(kind['body'],expected_body,'actual complete authored spell body')



@evidence_boundary
def native_probe_contexts(run:dict[str,Any],states:dict[int,Any],inventory:dict[str,Any])->dict[str,list[dict[str,Any]]]:
    """Count requested IF13 kinds only through their real attached state/input/continuation."""
    result={};rows=[run['initialization']]+run['accepted'];case=run['fixture_ref'];known_entries={};history={};types={r['oracle_id']:r['program']['card_types'] for r in run['static_inputs']['definitions']}
    for row in rows:
        for entry in row['header']['stack']:known_entries.setdefault(entry['entry'],row['ordinal'])
        for card in states[row['ordinal']]['cards'].values():
            if card['object']['zone']['kind']=='battlefield':history.setdefault(card['alias'],{}).setdefault(card['object']['object'],row['ordinal'])
    for probe_index,probe in enumerate(run['rejection_probes']):
        family=probe['expectation_ref'];ordinal=probe['attached_candidate_ordinal'];require(type(ordinal) is int and ordinal in states,'actual probe attached ordinal');source=rows[ordinal];header=source['header'];cards={c['alias']:c for c in states[ordinal]['cards'].values()};attempt=probe['attempted_input'];command=attempt['command'];continuation=rows[probe['continuation_ordinal']]
        same(source['ordinal'],ordinal,'actual probe source row');same(probe['original_basis'],header['basis'],'actual probe original basis');same(probe['continuation_ordinal'],ordinal+1,'actual next successful continuation');same(continuation['input']['basis'],header['basis'],'actual probe continuation basis')
        route=dict(case_id=case,probe_id=probe['probe_id'],probe_pointer='/observations/data/runs/0/rejection_probes/'+str(probe_index),source_ordinal=ordinal,continuation_ordinal=probe['continuation_ordinal'],error=probe['actual_error'])
        if family=='IF13.stale-seat-revision':
            expected=dict(header['basis'],revision=str(integer(header['basis']['revision'],'actual source revision')-1));same(attempt['basis'],expected,'actual stale revision only');same(attempt['actor'],continuation['input']['actor'],'actual stale valid actor');same(command,continuation['input']['command'],'actual stale otherwise valid command');error=['StaleRevision']
        else:
            same(attempt['basis'],header['basis'],'actual probe current basis');error=None
            if family=='IF13.foreign-actor':
                require(attempt['actor'] in header['participants'] and attempt['actor']!=header['priority_holder'],'actual registered nonholder');same(header['phase'],dict(kind='priority',phase=dict(kind='ready',holder=header['priority_holder'])),'actual wrong-player Ready phase');same(header['pending'],None,'actual no pending decision for wrong priority');same(command,dict(kind='pass'),'actual wrong-holder Pass');same(continuation['input']['actor'],header['priority_holder'],'actual holder successful continuation');same(continuation['input']['command'],dict(kind='pass'),'actual valid Pass continuation');error=['Effect','Window','Priority','WrongPlayer']
            elif family in {'IF13.counter-insufficient-payment','IF13.cancel-insufficient-payment','IF13.unavailable-mana-color'}:
                same(attempt['actor'],41,'actual requested payment actor');same(header['priority_holder'],41,'actual payment priority');same(command['kind'],'cast_paid','actual attempted ordinary paid cast');same(command['target']['kind'],'stack','actual requested live spell target');require(any(e['entry']==command['target']['entry'] and e['kind']['kind']=='card_spell' for e in header['stack']),'actual payment target is a live spell');counter=family=='IF13.counter-insufficient-payment';name='Counterspell' if counter else 'Cancel';same(inventory[command['spell']['alias']]['name'],name,'actual requested payment spell');units=2 if counter else 3;same(header['mana']['41'],{color:str(units) if color=='blue' else '0' for color in MANA.values()},'actual full pool before short spend')
                spend=[dict(amount='2',mana_type='blue'),dict(amount='1',mana_type='green')] if family=='IF13.unavailable-mana-color' else [dict(amount=str(units-1),mana_type='blue')];same(command['payment']['spend'],spend,'actual requested short spend or unavailable Green');error=['Effect','InsufficientMana'] if family=='IF13.unavailable-mana-color' else ['Effect','InvalidManaPayment','MissingRequired' if counter else 'WrongTotal']
                require(ordinal>0,'actual payment prior accepted tap');same(source['input']['command']['kind'],'activate_mana','actual prior accepted mana input');same(source['input']['actor'],41,'actual prior tap actor');tap=source['input']['command']['source'];same(source['current_facts']['mana_production']['source'],tap,'actual prior tap/credit source');same(source['current_facts']['mana_production']['produced'],dict(blue='1'),'actual prior Blue credit');same(cards[tap['alias']]['object']['tapped'],True,'actual prior source remains tapped');same(cards[tap['alias']]['object']['object'],tap['object'],'actual prior tapped incarnation')
                valid=continuation['input']['command'];same(valid,dict(kind='cast_paid',spell=command['spell'],target=command['target'],payment=dict(spend=[dict(amount=str(units),mana_type='blue')])),'actual full-cost successful continuation');same(continuation['header']['mana']['41']['blue'],'0','actual valid continuation consumes full pool');after={c['alias']:c for c in states[probe['continuation_ordinal']]['cards'].values()};same(after[tap['alias']]['object']['tapped'],True,'actual earlier accepted tap retained through valid continuation');route['context']=dict(source_mana=header['mana']['41'],attempted_spend=spend,live_target=command['target']['entry'],prior_accepted_tap=tap,valid_continuation=valid)
            elif family in {'IF13.counter-creature-permanent','IF13.counter-land-permanent','IF13.counter-card-in-hand','IF13.unsummon-land','IF13.unsummon-card-in-hand','IF13.unsummon-stale-incarnation'}:
                name=inventory[command['spell']['alias']]['name'];require(name in (['Counterspell','Cancel'] if family.startswith('IF13.counter-') else ['Unsummon']),'actual invalid-target source spell');same(command['target']['kind'],'card','actual invalid object target');target=command['target']['object'];card=cards[target['alias']];zone=card['object']['zone']['kind'];creature='Creature' in types[inventory[target['alias']]['oracleId']]
                if family.endswith('stale-incarnation'):
                    require(target['object']!=card['object']['object'],'actual stale target differs from current incarnation');require(history.get(target['alias'],{}).get(target['object'],ordinal)<ordinal,'actual stale target was a prior battlefield incarnation')
                else:
                    same(target['object'],card['object']['object'],'actual target current incarnation');same(zone,'hand' if family.endswith('card-in-hand') else 'battlefield','actual invalid target zone')
                    if family.endswith('land') or family.endswith('land-permanent'):require(inventory[target['alias']]['name'] in ['Island','Forest'],'actual noncreature basic-land target')
                    elif family.endswith('creature-permanent') or family=='IF13.unsummon-card-in-hand':same(creature,True,'actual creature target')
                error=['Effect','IllegalTarget']
            elif family=='IF13.counter-completed-entry':
                same(command['target']['kind'],'stack','actual completed entry target');entry=command['target']['entry'];require(entry in known_entries and not any(e['entry']==entry for e in header['stack']),'actual departed historical spell entry');require(known_entries[entry]<ordinal,'actual target entry existed before attempted input');error=['Effect','IllegalTarget']
            elif family.startswith('IF13.wrong-target-count.') and family.endswith('.missing'):
                name=family.split('.')[2];same(inventory[command['spell']['alias']]['name'].lower(),name,'actual targeted spell missing its target');same(command['kind'],'cast_untargeted','actual missing-target command');require('target' not in command,'actual missing target omitted');error=['Effect','TargetRequired']
            elif family=='IF13.divination-extra-target':
                same(inventory[command['spell']['alias']]['name'],'Divination','actual untargeted sorcery');same(command['kind'],'cast_paid','actual unsolicited-target cast');require(command.get('target') is not None,'actual unsolicited target supplied');error=['Effect','UnexpectedTarget']
            elif family=='IF13.creature-recast-before-lower-bounce-completes':
                same(command['kind'],'cast_untargeted','actual attempted creature recast');alias=command['spell']['alias'];same(cards[alias]['object']['zone']['kind'],'hand','actual returned creature in hand');require('Creature' in types[inventory[alias]['oracleId']],'actual recast creature');require(bool(header['stack']) and inventory[header['stack'][0]['kind']['card']['alias']]['name']=='Unsummon','actual lower bounce remains');same(header['active'],attempt['actor'],'actual active player sorcery timing');require(any(n<ordinal for n in history.get(alias,{}).values()),'actual bounced creature has earlier battlefield incarnation');error=['Effect','Timing','StackNotEmpty']
            elif family.startswith('IF13.recast-young-attacker.'):
                same(command['kind'],'reply','actual illegal attacker reply');pending=header['pending'];same(command['decision'],pending['id'],'actual attacker decision');same(command['choice']['kind'],'declare_attackers','actual attacker declaration');same(len(command['choice']['attackers']),1,'actual one prohibited attacker');target=command['choice']['attackers'][0]['attacker'];card=cards[target['alias']];same(card['object']['zone']['kind'],'battlefield','actual current creature battlefield');require(card['creature'] is not None,'actual young creature');same(card['object']['continuous_control_since_turn_start'],False,'actual young current creature')
                if family.endswith('stale-object'):
                    require(target['object']!=card['object']['object'],'actual departed attacker object');require(history.get(target['alias'],{}).get(target['object'],ordinal)<ordinal,'actual stale attacker was an earlier battlefield object')
                else:same(target['object'],card['object']['object'],'actual fresh current attacker object')
                require(not any(a['object']==target['object'] for a in pending['frame']['frame']['eligible_attackers']),'actual attempted attacker excluded from candidate domain')
                # Earlier first casts can share this diagnostic label; only an actual
                # departed battlefield incarnation establishes the frozen recast case.
                older=any(n<ordinal and object_id!=card['object']['object'] for object_id,n in history.get(target['alias'],{}).items())
                if not older:continue
                error=['Effect','Combat','IllegalAttacker']
            elif family=='IF13.second-land':
                same(command['kind'],'play_land','actual second-land input');land=command['land'];same(cards[land['alias']]['object']['zone']['kind'],'hand','actual second land in hand');same(cards[land['alias']]['object']['object'],land['object'],'actual second land current incarnation');require(inventory[land['alias']]['name'] in ['Island','Forest'],'actual second basic land');require(integer(header['turn']['lands_played'],'actual lands played')>0,'actual land limit already used');position=header['turn']['position'];require(any(row['input']['command']['kind']=='play_land' and row['input']['actor']==attempt['actor'] and row['header']['turn']['position']['number']==position['number'] for row in rows[1:ordinal+1]),'actual first land already accepted this turn');error=['Turn','LandLimitReached']
        if error is not None:
            same(probe['actual_error'],dict(path=error,payload=None),'actual exact requested probe error');result.setdefault(family,[]).append(route)
    return result


@evidence_boundary
def native_projection(run:dict[str,Any],case:dict[str,Any],full:dict[str,Any],manifest:dict[str,Any],focus:dict[str,Any]|None,qualify_probes:bool=False)->dict[str,Any]:
    """Bind actual native inputs, canonical journals and states to the frozen selected operations."""
    same(run['schema_version'],2,'actual interaction RunReport2');same(run['fixture_ref'],case['case_id'],'actual native case');same(run['completed'],True,'actual native completed');same(run['failure'],None,'actual native failure')
    accepted=run['accepted'];same([r['ordinal'] for r in accepted],list(range(1,len(accepted)+1)),'actual accepted sequence')
    rerun=run['rerun']
    for field in ['initialization_equal','final_state_equal','all_inputs_consumed']:same(rerun[field],True,'actual native reexecution '+field)
    same(rerun['compared_ordinals'],[r['ordinal'] for r in accepted],'all actual input reexecution');same(set(rerun['checkpoint_ordinals_compared']),{int(k) for k in run['checkpoints']},'all selected full checkpoint reexecution')
    final=run['final'];same(final['policy_complete'],True,'actual native policy complete');same(final['accepted_inputs'],len(accepted),'actual accepted count');same(final['final_ordinal'],len(accepted),'actual final ordinal');same(final['policy_entries_consumed'],final['policy_entries_total'],'actual recipe consumption')
    probes=unique(run['rejection_probes'],'probe_id','actual native rejection');same(set(rerun['probe_ids_compared']),set(probes),'all actual rejection reexecution')
    for probe in probes.values():same(probe['no_candidate'],True,'actual no rejected candidate');same(probe['state_retained'],True,'actual atomic rejection');same(probe['actual_error'],probe['expected_error'],'actual typed rejected error')
    inventory=full['inventory'];bindings=unique(run['static_inputs']['aliases'],'alias','native physical alias');same(set(bindings),set(inventory),'actual120 static aliases')
    for alias,row in bindings.items():same(row['definition_ref'],inventory[alias]['oracleId'],'actual native Oracle binding');same(native_role(row['owner']),inventory[alias]['role'],'actual native owner binding')
    validate_native_definitions(run['static_inputs']['definitions'],manifest)
    setup=run['static_inputs']['setup'];prepared=case['initialization']['kind']=='scheduled'
    if prepared:
        frozen=focus['setup'];same(setup['initialization'],dict(kind='scheduled',start=frozen['initialization']['turn_start']),'actual scheduled initialization contract');same(setup['initial_battlefield_order'],frozen['initial_battlefield_order'],'exact global native battlefield order');same(native_role(setup['active']),frozen['active_role'],'actual scheduled active role');same(run['initialization']['header']['entropy'],None,'scheduled entropy actually absent')
    else:same(setup['initialization'],dict(kind='opening'),'actual full opening initializer');same(final['entropy_remaining'],0,'actual ordinary entropy consumed')
    initial=run['checkpoints']['0'];current=dict(initial['cards']);piles=list(initial['ordered_piles']);objects={};worlds={};raw_worlds={};entry_map={};entry_actions={};entry_targets={};body_effect_entries=set();statuses={};cuts={};matched=[];next_action=0;payments=[];draws=[];damage=[];declaration_frames=[];journal_events=[];journal_ordinals=[]
    rows=[run['initialization']]+accepted;previous_header=run['initialization']['header'];previous_world=None;funding={'U':[],'G':[]}
    zero_pool={color:'0' for color in MANA.values()}
    for pool in previous_header['mana'].values():same(pool,zero_pool,'actual initial native mana empty; no prepared funding')
    for row in rows:
        for change in row.get('card_changes',[]):same(current[change['card']],change['before'],'actual card delta before');current[change['card']]=change['after']
        for change in row.get('pile_changes',[]):
            found=[i for i,p in enumerate(piles) if p['pile']==change['pile']];same(len(found),1,'actual changed pile domain');piles[found[0]]=change
        if str(row['ordinal']) in run['checkpoints']:
            checkpoint=run['checkpoints'][str(row['ordinal'])];same(checkpoint['cards'],current,'actual complete checkpoint vs card deltas');same(checkpoint['ordered_piles'],piles,'actual complete checkpoint vs ordered deltas');same(checkpoint['tokens'],{},'no selected tokens')
        for card in current.values():objects[card['object']['object']]=card['alias']
        header=row['header'];world=native_world_projection(current,piles,header,objects,inventory,entry_map);worlds[row['ordinal']]=world;raw_worlds[row['ordinal']]=dict(cards=dict(current),header=header)
        if row['ordinal']==0:
            previous_world=world
            if prepared:
                same(world['step'],frozen['initial_frame']['step'] if 'step' in frozen['initial_frame'] else frozen['step'],'actual initialized reached frame');same(world['priority'],frozen['active_role'],'actual initialized holder')
                for role in ['U','G']:
                    for source,dest in [('hand','hand'),('battlefield','battlefield'),('graveyard_top_first','graveyard'),('library_top_first','library')]:same(world['players'][role][dest],sorted(frozen[source][role]) if dest in ['hand','battlefield'] else frozen[source][role],'actual complete prepared setup '+role+dest)
            if prepared:
                stamped=frozen['initialization']['turn_start']['step']=='untap'
                for card in current.values():
                    obj=card['object']
                    if obj['zone']['kind']=='battlefield':same(obj['continuous_control_since_turn_start'],stamped and obj['controller']==header['active'],'actual scheduled control history without synthetic baseline')
                if stamped:
                    same([t['to']['step'] for t in row['current_facts']['turn_transitions']],['untap','upkeep'],'actual Untap initialization then reached Upkeep')
                    # All seeded permanents are already untapped; mandatory work can have no status-change event.
                    # Actual turn transitions, reached frame and epoch stamps remain required.
            journal_events.extend(row['current_facts']['interaction_journal']);journal_ordinals.extend([row['ordinal']]*len(row['current_facts']['interaction_journal']));previous_header=header;continue
        command=row['input']['command'];kind=command['kind'];actor=native_role(row['input']['actor']);before_pos=previous_header['turn']['position'] if previous_header['turn'] else None
        require(kind in ['pass','reply','play_land','activate_mana','cast_paid','cast_untargeted'],'unsupported actual native command')
        same(row['input']['basis'],previous_header['basis'],'actual accepted input attached to prior basis')
        if kind=='reply':
            pending=previous_header['pending'];require(pending is not None,'actual reply has pending frame');same(command['decision'],pending['id'],'actual reply decision identity');same(row['input']['actor'],pending['chooser'],'actual reply chooser');choice=command['choice'];frame=pending['frame']['frame']
            if choice['kind'] in ['declare_attackers','declare_blockers']:
                recipe=next(c for c in case['combat'] if c['individual_turn']==before_pos['number'])
                if choice['kind']=='declare_attackers':
                    selected=sorted(a['attacker']['alias'] for a in choice['attackers']);same(selected,sorted(recipe['attackers']),'native actual frozen attackers');same(sorted(a['alias'] for a in frame['eligible_attackers']),sorted(recipe['expected_eligible_attackers']),'native actual frozen eligible attackers');domain=sorted(a['alias'] for a in frame['eligible_attackers'])
                    for attack in choice['attackers']:same(native_role(attack['defender']),recipe['defender_role'],'native actual frozen defender')
                else:
                    selected=sorted([dict(blocker=b['blocker']['alias'],attacker=b['attacker']['alias']) for b in choice['blockers']],key=lambda b:b['blocker']);same(selected,sorted(recipe['blockers'],key=lambda b:b['blocker']),'native actual frozen blockers');domain=sorted(a['alias'] for a in frame['eligible_blockers'])
                declaration_frames.append(dict(kind=choice['kind'],turn=before_pos['number'],actor=actor,domain=domain,selected=selected))
            elif choice['kind']=='mulligan':same(choice['choice'],'keep','actual frozen keep choice');same(frame['taken'],0,'actual frozen no mulligan charge')
            else:raise EvidenceError('unsupported actual selected decision '+choice['kind'])
        else:same(row['input']['actor'],previous_header['priority_holder'],'actual command priority holder')

        if next_action<len(case['actions']):
            expected=case['actions'][next_action];wanted=expected['kind'];match=False
            if wanted=='pass':match=kind=='pass' and actor==expected['seat'] and before_pos is not None and before_pos['number']==expected['individual_turn'] and before_pos['step']==expected['step']
            elif wanted in ['declare_attackers','declare_blockers']:match=kind=='reply' and command['choice']['kind']==wanted
            else:
                actual_kind='cast' if kind in ['cast_paid','cast_untargeted'] else kind
                field='spell' if actual_kind=='cast' else 'source' if actual_kind=='activate_mana' else 'land'
                match=actual_kind==wanted and field in command and command[field]['alias']==expected['alias']
            if match:
                same(actor,expected['seat'],'actual named action actor');same(before_pos['number'],expected['individual_turn'],'actual named action turn');same(before_pos['step'],expected['step'],'actual named action step');same(native_role(previous_header['active']),expected.get('active_role',expected['seat']),'actual named action active role')
                if wanted not in ['pass','declare_attackers','declare_blockers']:same([v['kind']['card']['alias'] for v in previous_header['stack']],expected['expected_stack_bottom_first'],'actual native live stack before action')
                cuts['action.'+str(next_action)]=previous_world if wanted=='pass' else world;matched.append(expected['policy_id']);next_action+=1
                if wanted=='cast':
                    source=command['spell']['alias'];entry=header['stack'][-1];entry_id=entry['entry'];require(entry_id not in entry_actions,'actual unique native stack entry');entry_actions[entry_id]=expected['policy_id'];entry_map[entry_id]=source
                    same(entry['kind']['card']['alias'],source,'actual native cast top source');same(command['spell']['object'],next(c['object']['object'] for c in raw_worlds[row['ordinal']-1]['cards'].values() if c['alias']==source),'actual native hand source incarnation')
                    target=command.get('target');expected_target=expected.get('target')
                    if expected_target is None:require(target is None,'actual untargeted native cast')
                    elif expected_target['kind']=='object':same(target['kind'],'card','actual card target');same(target['object']['alias'],expected_target['card'],'actual target card alias');same(target['object']['object'],next(c['object']['object'] for c in raw_worlds[row['ordinal']-1]['cards'].values() if c['alias']==expected_target['card']),'actual target current incarnation')
                    else:same(target['kind'],'stack','actual stack target');require(target['entry'] in entry_map,'actual target entry history');same(entry_map[target['entry']],expected_target['spell'],'actual target live spell alias');require(any(e['entry']==target['entry'] for e in previous_header['stack']),'actual target is live at announcement')
                    entry_targets[entry_id]=target
                    payment=row['current_facts']['mana_payment'];same(payment['cost'],cost_tuple(expected['required_cost']),'actual native source-derived cost');same(native_role(payment['payer']),actor,'actual native payer');same(payment['source'],entry_id,'actual native paid entry identity');same(command['payment']['spend'],payment['spend'],'actual native submitted/debited payment')
                    color='blue' if actor=='U' else 'green';amount=sum(integer(s['amount'],'native spend') for s in payment['spend']);same(amount,expected['mana_units_paid'],'actual native payment amount');same(payment['spend'],[dict(amount=str(amount),mana_type=color)],'actual native complete spend color')
                    funded_pool=dict(zero_pool,**{color:str(len(funding[actor]))});same(previous_header['mana'][str(row['input']['actor'])],funded_pool,'actual pre-cast pool equals credited source trace');same(amount,len(funding[actor]),'all actual credited units consumed by this cast');same(header['mana'][str(row['input']['actor'])],zero_pool,'actual post-cast pool empty')
                    # Native stores aggregate mana, not source lots. These finite cases start empty
                    # and consume the complete credited pool, so the funding set is unambiguous.
                    actual_sources=list(funding[actor]);same(actual_sources,expected['paid_sources'],'actual native funding activation trace');funding[actor]=[]
                    debits=[e for e in row['current_facts']['interaction_journal'] if e['cause']['tag']=='ManaDebited'];same(len(debits),1,'actual one-color debit journal');same(debits[0]['cause'],dict(entry=entry_id,tag='ManaDebited'),'actual debit journal entry');values=[o['value'] for o in debits[0]['occurrences']];same(len(values),1,'actual one-color debit occurrence');debit=values[0]
                    same({k:debit[k] for k in ['tag','type','before','amount','after']},dict(tag='ManaDebited',type=color.capitalize(),before=str(amount),amount=str(amount),after='0'),'actual full native color debit');same(debit['payment'],dict(cost=dict(generic=payment['cost']['generic'],required={k.capitalize():v for k,v in payment['cost']['required'].items()}),entry=entry_id,payer=payment['payer'],spend=[dict(amount=str(amount),type=color.capitalize())]),'actual debit journal payment binding')
                    payments.append(dict(action_id=expected['policy_id'],card=source,cost=expected['required_cost'],payer=actor,paid_sources=actual_sources,amount=str(amount)))
                    payload=header['interaction_execution']['stack'][-1];name=inventory[source]['name'];body=payload['body']
                    if name in ['Counterspell','Cancel']:same(body,dict(instructions=[dict(op='CounterSpell')],target=dict(support_gate='AnySpell',tag='SingleSpell')),'actual counter body AnySpell')
                    elif name=='Unsummon':same(body,dict(instructions=[dict(op='ReturnToOwnersHand')],target=dict(tag='SingleCreature')),'actual bounce body')
                    elif name=='Divination':same(body,dict(instructions=[dict(amount=dict(tag='Literal',value='2'),op='DrawController')],target=dict(tag='None')),'actual sequential draw body')
                    else:same(body,None,'actual vanilla permanent has no instruction body')
                elif wanted=='activate_mana':
                    production=row['current_facts']['mana_production'];same(production['source']['alias'],expected['alias'],'actual native mana source');same(native_role(production['controller']),actor,'actual native mana controller');same(production['produced'],{('blue' if actor=='U' else 'green'):'1'},'actual native intrinsic production')
                    same(production['source'],command['source'],'actual native submitted/credited source incarnation');color='blue' if actor=='U' else 'green';same(previous_header['mana'][str(row['input']['actor'])],dict(zero_pool,**{color:str(len(funding[actor]))}),'actual pool before named credit');funding[actor].append(production['source']['alias']);same(header['mana'][str(row['input']['actor'])],dict(zero_pool,**{color:str(len(funding[actor]))}),'actual pool after named credit')
            elif kind in ['play_land','activate_mana','cast_paid','cast_untargeted']:raise EvidenceError('unplanned native ordinary action before '+expected['policy_id'])
        elif kind in ['play_land','activate_mana','cast_paid','cast_untargeted']:raise EvidenceError('extra native ordinary action')
        for seat,pool in header['mana'].items():
            if kind not in ['activate_mana','cast_paid','cast_untargeted'] or seat!=str(row['input']['actor']):same(pool,previous_header['mana'][seat],'no unobserved native mana credit/debit/clear')
        events=row['current_facts']['interaction_journal'];require(isinstance(events,list),'actual canonical instruction journal required')
        for event in events:
            same(event['tag'],'Event','selected actual RulesJournal event');cause=event['cause'];journal_events.append(event);journal_ordinals.append(row['ordinal'])
            if cause['tag']=='CounterSpell':
                require(cause['entry'] in entry_actions and cause['target'] in entry_actions,'actual counter event entry bindings');same(entry_targets[cause['entry']],dict(entry=cause['target'],kind='stack'),'actual counter effect uses originally declared target');body_effect_entries.add(cause['entry']);target_entry=cause['target'];require(any(o['value']['tag']=='ZoneChange' and o['value']['reason']['tag']=='Countered' for o in event['occurrences']),'actual countered zone transition');statuses[entry_actions[target_entry]]='countered'
            if cause['tag']=='ResolutionCompleted':
                require(cause['entry'] in entry_actions,'actual completion entry identity');status={'Resolved':'resolved','AllTargetsIllegal':'all_targets_illegal_nonresolution'}[cause['completion']];statuses[entry_actions[cause['entry']]]=status
            if cause['tag']=='Instruction':body_effect_entries.add(cause['entry'])
            if cause['tag']=='Draw' and cause['origin']['tag']=='Instruction':
                body_effect_entries.add(cause['origin']['entry']);require(cause['origin']['entry'] in entry_actions,'actual body draw entry');draws.append(dict(action_id=entry_actions[cause['origin']['entry']],event=event['event'],values=[o['value'] for o in event['occurrences']]))
        for applied in row['current_facts']['damage_journal']['applied']:
            if integer(applied['amount'],'actual native applied damage')>0:
                source=objects[applied['source']['object']['object']];target=applied['target'];recipient=native_role(target['player']) if target['kind']=='player' else objects[target['object']];damage.append(dict(turn=world['turn'],source=source,target=recipient,amount=applied['amount']))
        permanent=row['current_facts']['permanent_entry']
        if permanent:
            source=permanent['after']['alias'];entry=permanent['entry'];require(entry in entry_actions,'actual permanent entry cast binding');same(entry_map[entry],source,'actual permanent entry source');same(statuses[entry_actions[entry]],'resolved','actual permanent entry completion');same(permanent['before']['zone']['kind'],'stack','actual permanent entry from stack');same(permanent['after']['zone']['kind'],'battlefield','actual permanent entry to battlefield')
        for action in statuses:
            if 'settle.'+action not in cuts and not any(entry_actions.get(e['entry'])==action for e in header['stack']):cuts['settle.'+action]=world
        previous_header=header;previous_world=world
    same(next_action,len(case['actions']),'all native named action inputs consumed')
    same(funding,{'U':[],'G':[]},'no retained ambiguous native mana funding')
    same(set(statuses),{a['policy_id'] for a in case['actions'] if a['kind']=='cast'},'every actual native cast disposition')
    draw_attempt_facts={}
    for entry,action in entry_actions.items():
        if statuses[action] in ['countered','all_targets_illegal_nonresolution']:require(entry not in body_effect_entries,'no instruction body executed for countered/all-illegal entry')
        if inventory[entry_map[entry]]['name']=='Divination' and statuses[action]=='resolved':
            effects=[i for i,e in enumerate(journal_events) if e['cause']['tag']=='Draw' and e['cause']['origin'].get('entry')==entry]
            attempted=0;successful=0;failed_batches=[]
            for index in effects:
                event=journal_events[index];same(event['cause']['origin'],dict(entry=entry,instruction=0,tag='Instruction'),'actual native DrawController instruction cause');same(event['cause']['player'],41,'actual draw controller');same(len(event['occurrences']),1,'actual unreplaced draw transaction occurrence')
                value=event['occurrences'][0]['value']
                if value['tag']=='ZoneChange':
                    require(not failed_batches,'actual successful draws precede remaining empty failures');same(value['before']['object']['zone'],dict(kind='library',player=41),'actual controller library draw source');same(value['after']['object']['zone'],dict(kind='hand',player=41),'actual controller draw destination');successful+=1;attempted+=1
                else:
                    same(value['tag'],'FailedDraw','actual typed unsuccessful draw');same(value['player'],41,'actual failed draw player');amount=integer(value['attempts'],'actual failed draw attempts');require(amount>0,'actual positive failed attempt count');failed_batches.append(value['attempts']);attempted+=amount
            same(attempted,2,'actual native Divination total attempted draws');require(len(failed_batches)<=1,'actual source compresses consecutive empty failures into one remaining batch')
            completion_index=next(i for i,e in enumerate(journal_events) if e['cause']['tag']=='ResolutionCompleted' and e['cause']['entry']==entry)
            require(all(index<completion_index for index in effects),'actual native draw effects precede body completion');same(len({journal_ordinals[index] for index in effects+[completion_index]}),1,'actual complete draw body has no intervening accepted priority input')
            require(not any(journal_events[i]['cause']['tag']=='StateBasedActions' for i in range(effects[0],completion_index)),'no intermediate native state-based check before Divination completes')
            if failed_batches:
                losses=[i for i,e in enumerate(journal_events) if e['cause']['tag']=='StateBasedActions' and any(o['value']['tag']=='PlayerLost' and o['value']['player']==41 and 'FailedDraw' in o['value']['reasons'] for o in e['occurrences'])];require(bool(losses) and all(i>completion_index for i in losses),'actual failed-draw loss follows completed Divination')
            draw_attempt_facts[action]=dict(attempted=str(attempted),successful=str(successful),failed=str(attempted-successful),observed_draw_transactions=len(effects),remaining_failure_batches=failed_batches)
    terminal_result=None
    if case['stop']['kind']=='ready_priority':
        same(final['kind'],'bounded_prefix','actual native bounded classification');same(final['boundary_ref'],case['stop']['boundary_ref'],'actual bounded boundary ref');same(final['position']['number'],case['stop']['individual_turn'],'actual bounded turn');same(final['position']['step'],case['stop']['step'],'actual bounded step');same(native_role(final['active']),case['stop']['active_role'],'actual bounded active');same(native_role(final['holder']),case['stop']['priority_holder'],'actual bounded holder');same(rows[-1]['header']['outcome'],None,'native bounded no invented outcome')
        require('outcome' not in final,'actual bounded final has no invented natural outcome')
    else:
        same(final['outcome']['kind'],'winner','actual natural native winner');same(final['outcome'],rows[-1]['header']['outcome'],'actual final outcome equals terminal native header');winner=final['outcome']['seat'];require(winner in rows[-1]['header']['participants'],'actual terminal winner is a participant');same(rows[-1]['header']['losses'],[seat for seat in rows[-1]['header']['participants'] if seat!=winner],'actual terminal native winner/loss partition')
        terminal_result=dict(winning_role=native_role(winner),losing_roles=sorted(native_role(seat) for seat in rows[-1]['header']['losses']))
    draw_sequences={}
    card_aliases={str(row['card_id']):row['alias'] for row in run['static_inputs']['aliases']}
    for event in draws:
        for value in event['values']:
            if value['tag']=='ZoneChange':same(value['before']['object']['zone']['kind'],'library','actual instruction draw from library');same(value['after']['object']['zone']['kind'],'hand','actual instruction draw to hand');draw_sequences.setdefault(event['action_id'],[]).append(card_aliases[str(value['after']['object']['physical_card'])])
            else:same(value['tag'],'FailedDraw','actual instruction draw result kind');require(integer(value['attempts'],'actual failed instruction attempts')>0,'actual positive failed instruction attempts')
    return dict(draw_sequences=draw_sequences,native_draw_attempts=draw_attempt_facts,damage=sorted(damage,key=lambda r:(r['turn'],r['source'],r['target'])),declaration_frames=declaration_frames,case_id=case['case_id'],worlds=worlds,cuts=cuts,final=worlds[final['final_ordinal']],payments=payments,settlements=statuses,draws=draws,journal_events=journal_events,rerun=rerun,probes=len(probes),probe_families=sorted({p['expectation_ref'] for p in probes.values()}),captured=False,probe_evidence=native_probe_contexts(run,raw_worlds,inventory) if qualify_probes else {},terminal_result=terminal_result)


@evidence_boundary
def join_native_reference(native:dict[str,Any],reference:dict[str,Any],case:dict[str,Any])->dict[str,Any]:
    """Compare actual common facts; preserve native-only and unavailable routes explicitly."""
    same(native['settlements'],reference['settlements'],'actual native/reference entry dispositions');same(native['payments'],reference['payments'],'actual native/reference action-specific full payments');same(native['draw_sequences'],reference['actual_facts']['draw_sequences'],'actual ordered successful instruction draws')
    same(native['terminal_result'],reference['terminal_result'],'actual native/reference terminal winner and losses')
    same(native['damage'],reference['actual_facts']['damage'],'actual native/reference damage quantities and recipients')
    common_frames=0;reference_frames={(r['kind'],r['turn'],r['actor']):r for r in reference['actual_facts']['declaration_frames']};native_frames={(r['kind'],r['turn'],r['actor']):r for r in native['declaration_frames']}
    for key in reference_frames.keys()|native_frames.keys():
        a=native_frames.get(key);b=reference_frames.get(key)
        if a is not None and b is not None:same(a,b,'actual declaration/domain common receipt');common_frames+=1
        else:
            present=a or b;require(not present['selected'] and not present['domain'],'required nonempty actual declaration receipt unavailable')

    count=0
    for key,world in native['cuts'].items():
        require(key in reference['worlds'],'required actual reference cut '+key)
        # Combat attack/block declaration receipt timing is observed separately from Ready state.
        if key.startswith('action.') and case['actions'][int(key.split('.')[1])]['kind']=='declare_blockers':
            a=dict(world,combat=[{k:v for k,v in c.items() if k!='blocked'} for c in world['combat']]);b=dict(reference['worlds'][key],combat=[{k:v for k,v in c.items() if k!='blocked'} for c in reference['worlds'][key]['combat']]);same(a,b,'actual block declaration receipt common fields');count+=1;continue
        same(world,reference['worlds'][key],'actual common full world '+case['case_id']+' '+key);count+=1
    same(native['final'],reference['final'],'actual complete final common world');count+=1
    return dict(case_id=case['case_id'],actual_common_worlds_equal=count,actual_cast_dispositions_equal=len(native['settlements']),actual_action_payments_equal=len(native['payments']),actual_common_declaration_receipts_equal=common_frames,actual_terminal_result_equal=True,terminal_result=native['terminal_result'],native_draw_attempts=native['native_draw_attempts'],native_full_rerun=True,native_rejection_probes=native['probes'],native_only=['exact typed object/entry IDs','complete native candidate/journal/final GameState rerun','failed-draw instruction attempts (consecutive empty failures may share one source-defined journal transaction)'],unavailable_reference=['internal native cursor/failed-draw cardinality','blocked flag at pre-recomposition Forge block-declaration event; required again at actual Ready/cast cuts'],declaration_receipt_timing='Raw real declarations retained; actual Ready/combat/damage facts checked separately, no invented callback for native ForcedEmpty.')


@evidence_boundary
def validate_actual_check_row(row:dict[str,Any],cwd:Path)->list[str]:
    """Inspect actual named execution/counts and Cargo Running paths, independent of producer success flags."""
    same(row['executed'],True,'actual check process executed');same(row['interrupted'],False,'actual check process uninterrupted');same(row['failure'],None,'actual check has no execution/evidence failure');same(row['evidenceValid'],True,'actual check generated evidence admitted');same(row['exitCode'],0,'actual check exit');text=Path(row['log']).read_text();command=row['command'];names=re.findall(r'^test ([^\s]+) \.\.\. ok\s*$',text,re.M);same(row['observedTests'],names,'actual named test log receipt')
    if command[:2]==['cargo','test']:
        summaries=re.findall(r'^test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;',text,re.M);require(bool(names) and bool(summaries) and sum(int(value[0]) for value in summaries)>0 and all(int(value[1])==0 for value in summaries),'actual nonzero passed Cargo tests')
        if '--exact' in command:
            selector=command[command.index('--')-1];same(names,[selector],'actual exact named selector executed once');same(summaries,[('1','0','0')],'actual exact selector one passed zero ignored')
        paths=[]
        for value in re.findall(r'Running [^\n]*?\(([^()\n]+)\)',text):
            path=Path(value);path=path if path.is_absolute() else cwd/path;path=path.resolve();require(path.is_relative_to(cwd/'target/debug/deps'),'actual Cargo test executable origin');paths.append(str(path))
        same([value['path'] for value in row['testExecutables']],paths,'actual complete Cargo Running/test-binary relation');require(bool(paths) and len(paths)==len(set(paths)),'actual unique nonempty test binary observations')
    elif 'unittest' in command:
        counts=re.findall(r'^Ran (\d+) tests? in ',text,re.M);require(len(counts)==1 and int(counts[0])>0 and re.search(r'^OK\s*$',text,re.M) is not None,'actual nonzero successful Python discovery')
    for receipt in row['generated']:bound_file(receipt,'actual check generated artifact')
    return names


@evidence_boundary
def validate_final_reference_sources(run_dir:Path,reference_sources:dict[str,str])->None:
    """Require original and copied six-source pre/post receipts for final-source Forge epochs."""
    stability=read(run_dir/'source-stability.json');before=stability['original_source_before'];same(before,stability['original_source_after'],'actual final six helper sources stable');same(stability['original_source_unchanged'],True,'actual original six-source guard');expected={Path(path).name:digest for path,digest in reference_sources.items()};expected.update({'teaching_reference_compare.py':'04c0e2d0e867eddef679a729cd969b927370ad491abc5d2f2deb6aa4039e1556','teaching_forge_reference.py':'edc43aaee5890dba82ac42dc09bf3cd02d068dd56f1c6f7716baec82ae1d3843'});same({Path(path).name:digest for path,digest in before.items()},expected,'actual exact six original helper sources')
    for path,digest in before.items():same(sha(Path(path)),digest,'actual current final helper bytes');same(sha(run_dir/'source'/Path(path).name),digest,'actual copied final helper bytes')


@evidence_boundary
def validate_image_binding(binding:dict[str,Any],profile:str=PROFILES[0],requirements:dict[str,Any]|None=None)->dict[str,dict[str,Any]]:
    """Bind actual per-case receipts to one checked image/source/input tuple, not index flags alone."""
    same(binding['schemaVersion'],1,'native image binding schema1');require(profile in PROFILES,'strict image profile');mandatory=profile==PROFILES[1];validated_files={}
    def bound(row:dict[str,Any],label:str)->None:
        path=Path(row['path']);require(path.is_absolute() and path.is_file(),label+' absolute file exists');require(re.fullmatch('[0-9a-f]{64}',row['sha256']) is not None,label+' canonical digest');stat=path.stat();identity=(stat.st_ino,stat.st_size,stat.st_mtime_ns,stat.st_ctime_ns);key=(str(path),identity)
        if key not in validated_files:validated_files[key]=sha(path)
        same(validated_files[key],row['sha256'],label+' actual bytes')
        if 'bytes' in row:same(path.stat().st_size,row['bytes'],label+' actual byte count')
    bound(binding['runtime'],'final native runtime');bound(binding['runtime']['buildReceipt'],'actual final native build receipt');bound(binding['sourceManifest'],'final native source manifest')
    same({r['sha256'] for r in binding['inputs']},set(NATIVE_INPUT_PINS.values()),'exact four pinned native inputs');same(len(binding['inputs']),4,'four native inputs');unique(binding['inputs'],'path','native input path')
    for row in binding['inputs']:bound(row,'actual final native input')
    inputs={name:next(r for r in binding['inputs'] if r['sha256']==digest) for name,digest in NATIVE_INPUT_PINS.items()}
    manifest=read(Path(binding['sourceManifest']['path']));require(isinstance(manifest,dict) and bool(manifest),'actual final source manifest nonempty')
    for relative,digest in manifest.items():require(not Path(relative).is_absolute() and '..' not in Path(relative).parts and re.fullmatch('[0-9a-f]{64}',digest) is not None,'actual relative source manifest member')
    build=read(Path(binding['runtime']['buildReceipt']['path']));rows=unique(build,'name','native integrated check')
    cli_command=['cargo','build','--locked','-p','spellwake-reference-tools','--bin','teaching-game','--bin','interaction-game']+(['--bin','mandatory-trigger-game'] if mandatory else [])
    if mandatory:
        require(requirements is not None,'mandatory image requires actual frozen requirements');same(list(rows),list(MANDATORY_CHECK_COMMANDS),'actual exact17 ordered mandatory final check set')
        require(MANDATORY_PINS['compiled-input'] in manifest.values(),'actual checked compiled-input source bytes')
        for name,row in rows.items():
            same(row['exitCode'],0,'actual successful mandatory qualification check');same(row['command'],MANDATORY_CHECK_COMMANDS[name],'actual exact mandatory check '+name);require(Path(row['log']).is_file(),'actual qualification log exists')
        for command in requirements['required_opt_ins']:
            selector=command[command.index('--test')+2]
            matches=[row for row in build if selector in row['command'] or ('mandatory_registry' in row['command'] and selector=='actual_full_catalogue_mandatory_inventory_is_source_qualified')];same(len(matches),1,'one actual required selector command '+selector);require('test '+selector+' ... ok' in Path(matches[0]['log']).read_text(),'actual exact required opt-in selector passed '+selector)
    else:
        same(len(build),9,'actual nine completed integrated checks');same(set(rows),set(NATIVE_CHECK_COMMANDS),'actual complete native check set')
        for name,row in rows.items():same(row['exitCode'],0,'actual successful native check');same(row['command'],NATIVE_CHECK_COMMANDS[name],'actual exact native check '+name)
    build_row=rows['cli-build'];same(build_row['command'],cli_command,'actual explicit final CLI build')
    if mandatory:
        checker_dir=Path(binding['runtime']['buildReceipt']['path']).parent;checked_commands=read(checker_dir/'commands.json');checked_root=Path(checked_commands['cwd'])
        for test in build:validate_actual_check_row(test,checked_root)
        registry_names=rows['mandatory-registry-tests']['observedTests'];require({'actual_full_catalogue_mandatory_inventory_is_source_qualified','actual_full_catalogue_raw_only_traits_fail_at_source_intake'}<=set(registry_names),'both actual registry source opt-ins passed');negatives=Path(checked_commands['explicit_environment']['SPELLWAKE_MANDATORY_NEGATIVE_OUTPUT'])/'mutation-receipts.json';require(any(Path(row['path']).resolve()==negatives.resolve() for row in rows['mandatory-registry-tests']['generated']),'actual bound raw-source negative summary');negative=read(negatives);same(negative['kind'],'actual-full-source-raw-only-negative-admission-evidence','actual raw-only source admission negative kind');same(negative['metadataLengthAndSelectionNegatives'],True,'actual metadata/length/selection negatives');same(negative['originalSourceSha256'],NATIVE_INPUT_PINS['snapshot'],'actual original raw source negative authority');same({row['mutation'] for row in negative['records']},{'colors','color-identity','keywords','color-indicator','produced-mana','multipart'},'actual six raw source-only negative fields');same(len(negative['records']),6,'actual six distinct raw source negatives')
        for row in negative['records']:same(row['result'],'MandatorySourceMismatch','actual source-only negative rejected');require(row['changedCompressedSha256']!=negative['originalSourceSha256'] and row['changedRawLineSha256']!=row['originalRawLineSha256'],'actual compressed and decompressed raw mutation');require(row['after']!=row['before'],'actual raw field changed')
    image_name='mandatory-trigger-game' if mandatory else 'interaction-game';same(Path(binding['runtime']['path']).name,image_name,'actual selected finite CLI image')
    artifacts=[r for r in build_row['artifacts'] if Path(r['path']).name==image_name];same(len(artifacts),1,'actual one built finite-profile image');same(artifacts[0]['sha256'],binding['runtime']['sha256'],'actual build-time image equals copied runtime');same(artifacts[0]['bytes'],Path(binding['runtime']['path']).stat().st_size,'actual built image byte count');bound(artifacts[0],'actual retained build artifact')
    require(all(r['name']=='python-guards' for r in build[build.index(build_row)+1:]),'no later Rust build after actual final CLI build')
    cases=unique(binding['cases'],'caseId','native image case')
    for case,row in cases.items():
        bound(row,'actual final capture '+case);bound(row['executionReceipt'],'actual native execution receipt '+case);same(row['exitCode'],0,'actual native process exit '+case)
        for flag in ['sourceStable','inputStable','binaryStable']:same(row[flag],True,'actual native stable '+flag)
        receipt=read(Path(row['executionReceipt']['path']));same(receipt['schemaVersion'],1,'actual execution receipt schema');same(receipt['kind'],'actual-mandatory-trigger-native-case-execution' if mandatory else 'actual-blue-interaction-native-case-execution','actual final receipt kind');same(receipt['caseId'],case,'actual receipt case');same(receipt['executed'],True,'actual process executed');same(receipt['captureEnabled'],True,'actual native capture enabled');same(receipt['exitCode'],0,'actual receipt exit');same(receipt['interrupted'],False,'actual process uninterrupted');same(receipt['terminationReason'],None,'actual process no forced termination')
        for field in ['runtime','sourceManifest','inputs']:same(receipt[field],binding[field],'actual receipt/index '+field+' tuple')
        same({k:receipt['stdout'][k] for k in ['path','sha256']},{k:row[k] for k in ['path','sha256']},'actual output/receipt/index binding');bound(receipt['stdout'],'actual stdout '+case);bound(receipt['stderr'],'actual stderr '+case)
        same(receipt['command'],[binding['runtime']['path'],'--metadata',inputs['metadata']['path'],'--snapshot',inputs['snapshot']['path'],'--case',case,'--capture'],'actual copied-image command '+case)
        environment=receipt['environment'];same(set(environment),{'PATH','LANG','LC_ALL','TZ','RUST_BACKTRACE','SPELLWAKE_CREATURE_VOCABULARY_INPUT','SPELLWAKE_CREATURE_VOCABULARY_PUBLICATION'},'actual minimal capture environment');require(isinstance(environment['PATH'],str) and bool(environment['PATH']),'actual explicit PATH')
        for name,value in {'LANG':'C.UTF-8','LC_ALL':'C.UTF-8','TZ':'UTC','RUST_BACKTRACE':'0','SPELLWAKE_CREATURE_VOCABULARY_INPUT':inputs['vocabulary']['path'],'SPELLWAKE_CREATURE_VOCABULARY_PUBLICATION':inputs['publication']['path']}.items():same(environment[name],value,'actual capture environment '+name)
        cwd=Path(receipt['cwd']);require(cwd.is_absolute() and cwd.is_dir(),'actual canonical capture cwd');check_dir=Path(binding['runtime']['buildReceipt']['path']).parent
        commands=read(check_dir/'commands.json');same(Path(commands['cwd']).resolve(),cwd.resolve(),'actual build/run cwd');same(commands['commands'],[[r['name'],r['command']] for r in build],'actual checked command plan')
        same(read(check_dir/'source-before.json'),manifest,'actual checked/copied source manifest');same(read(check_dir/'source-after.json'),manifest,'actual checked source successor');stable=read(check_dir/'source-stability.json');same(stable['equal'],True,'actual checked source stability');same(stable['files'],len(manifest),'actual checked source count');same(stable['changed'],[],'actual checked source differences')
        guards={}
        for position in ['before','after']:
            bound(receipt['guards'][position],'actual '+position+' guard '+case);guard=read(Path(receipt['guards'][position]['path']));guards[position]=guard
            same(guard['source'],manifest,'actual '+position+' complete native source');same(guard['retainedManifestSha256'],binding['sourceManifest']['sha256'],'actual '+position+' retained manifest');same(guard['inputs'],NATIVE_INPUT_PINS,'actual '+position+' copied input guards');same(guard['originalInputs'],NATIVE_INPUT_PINS,'actual '+position+' original input guards');same(guard['binary'],dict(retained=binding['runtime']['sha256'],buildArtifact=binding['runtime']['sha256']),'actual '+position+' image guards')
            for flag in ['sourceStable','inputStable','binaryStable']:same(guard[flag],True,'actual '+position+' '+flag);same(receipt[flag],row[flag],'actual receipt/index '+flag)
            authority=guard['authorityFiles'];expected_paths={str(check_dir/name) for name in ['results.json','commands.json','source-before.json','source-after.json','source-stability.json']}|{r['log'] for r in build}
            if mandatory:
                same(set(commands['authorities']),{str(cwd/'work/engine-milestones/mandatory-trigger-v1/implementation-v1/integration-check-command-plan-v1.json'),str(cwd/'work/engine-milestones/mandatory-trigger-v1/integration-tools/run_checks.py')},'actual exact checker/plan authority paths');same(commands['authorities'][str(cwd/'work/engine-milestones/mandatory-trigger-v1/implementation-v1/integration-check-command-plan-v1.json')],'6cf5478e810e1744f97b800343cd3de5ed2f61b3025476d24bcb04153e0d1b01','actual reviewed exact17 command-plan hash')
                same(commands['authorities'][str(cwd/'work/engine-milestones/mandatory-trigger-v1/integration-tools/run_checks.py')],MANDATORY_HELPER_HASHES['run_checks.py'],'actual reviewed final checker helper hash')
                extra=['check-completion.json','additional-stability.json','inputs-before.json','inputs-after.json','reference-before.json','reference-after.json','test-executables.json'];expected_paths|={str(check_dir/name) for name in extra};expected_paths|=set(commands['authorities'])
                reference=read(check_dir/'reference-before.json');same(reference,read(check_dir/'reference-after.json'),'actual reference source pre/post equality');same(set(reference),{'tests/reference/forge/SpellwakeBlueInteractionReferenceTest.java','scripts/interaction_forge_reference.py','scripts/interaction_reference_compare.py','scripts/interaction_reference_controls.py'},'actual exact four maintained reference owners');expected_paths|={str(cwd/path) for path in reference}
                for relative,digest in reference.items():same(sha(cwd/relative),digest,'actual checked current reference source')
                for flag in ['inputs_equal','reference_equal','authorities_equal','test_executables_equal','cli_images_equal']:same(read(check_dir/'additional-stability.json')[flag],True,'actual additional final guard '+flag);same(read(check_dir/'check-completion.json')[flag],True,'actual check completion '+flag)
                for flag in ['success','all_planned_commands_passed','native_source_equal']:same(read(check_dir/'check-completion.json')[flag],True,'actual complete final checker '+flag)
                expected_inputs={Path(row['path']).name:row['sha256'] for row in binding['inputs']};same(read(check_dir/'inputs-before.json'),expected_inputs,'actual checked copied source inputs');same(read(check_dir/'inputs-after.json'),expected_inputs,'actual checked successor source inputs')
                binaries=read(check_dir/'test-executables.json');observed={}
                images=build_row['artifacts'];same(len(images),3,'actual exact three built CLI artifacts');same({Path(image['path']).name for image in images},{'teaching-game','interaction-game','mandatory-trigger-game'},'actual finite three CLI build images')
                for image in images:bound(image,'actual final live three-bin image');same(Path(image['path']).resolve(),(cwd/'target/debug'/Path(image['path']).name).resolve(),'actual canonical CLI artifact origin');expected_paths.add(image['path'])
                for test in build:
                    for generated in test['generated']:bound(generated,'actual checked generated output');expected_paths.add(generated['path'])
                for test in build:
                    if test['command'][:2]!=['cargo','test']:continue
                    require(bool(test['testExecutables']),'actual nonempty observed native test executables')
                    for executable in test['testExecutables']:bound(executable,'actual compiled native test binary');observed[executable['path']]=executable;expected_paths.add(executable['path'])
                same(binaries,observed,'actual complete test executable binding inventory')
                capture_dir=Path(binding['runtime']['path']).parent.parent;orchestrator=read(capture_dir/'orchestrator.json');bound(orchestrator['helper'],'actual capture process helper');same(orchestrator['helper']['sha256'],MANDATORY_HELPER_HASHES['capture_native.py'],'actual reviewed final capture helper hash');same(sha(capture_dir/'orchestrator-source.py'),orchestrator['helper']['sha256'],'actual retained capture helper source');same(sha(capture_dir/'run_checks.py'),commands['authorities'][str(cwd/'work/engine-milestones/mandatory-trigger-v1/integration-tools/run_checks.py')],'actual retained imported checker helper source');same(orchestrator['cwd'],str(cwd),'actual capture/check cwd');same(orchestrator['caseOrder'],list(cases),'actual exact capture case order');expected_paths.add(orchestrator['helper']['path']);same(orchestrator['authorityFiles'],authority,'actual orchestrator/per-case complete authorities')
            same(set(authority),expected_paths,'actual complete check authority guard');same(authority[str(check_dir/'results.json')],binding['runtime']['buildReceipt']['sha256'],'actual build receipt guard')
            for path,digest in authority.items():bound(dict(path=path,sha256=digest),'actual checked authority')
        same(guards['before'],guards['after'],'actual pre/post guard records agree')
    return cases


@evidence_boundary
def mandatory_source_inventory(root:Path,inputs:list[dict[str,Any]])->set[str]:
    """Derive exact code/input owners from the guarded actual CLI and common capture declarations."""
    declarations=[('crates/reference_tools/src/bin/mandatory-trigger-game.rs',r'let mut paths = \[(.*?)\]\s*\.map',20),('crates/reference_tools/src/current_capture.rs',r'for path in \[(.*?)\]\s*\{\s*let path = repo.join',14)];paths=[]
    for relative,pattern,count in declarations:
        text=(root/relative).read_text();blocks=re.findall(pattern,text,re.S);same(len(blocks),1,'actual unique declared capture source array '+relative);members=re.findall(r'"(crates/[^"\n]+)"',blocks[0]);same(len(members),count,'actual declared source owner count '+relative);same(len(set(members)),count,'actual declared source array distinct members')
        residue=re.sub(r'"crates/[^"\n]+"|[\s,]','',blocks[0]);same(residue,'','actual source array contains only explicit path literals')
        for member in members:
            path=(root/member).resolve();require(path.is_relative_to(root.resolve()),'actual capture source contained in root');paths.append(str(path))
    code=set(paths);same(len(code),32,'actual deduplicated CLI/shared code owner inventory');copied=[row['path'] for row in inputs if row['sha256'] in {NATIVE_INPUT_PINS['metadata'],NATIVE_INPUT_PINS['snapshot']}];same(len(copied),2,'actual copied catalogue/metadata source inventory');same(len(set(copied)),2,'actual distinct copied source paths');return code|set(copied)


@evidence_boundary
def validate_captured_sources(document:dict[str,Any]|MandatoryCapture,binding:dict[str,Any],case_row:dict[str,Any])->None:
    """Join actual witness SourceRefs to guarded native sources and the four copied inputs."""
    receipt=read(Path(case_row['executionReceipt']['path']));cwd=Path(receipt['cwd']).resolve();manifest=read(Path(binding['sourceManifest']['path']));inputs={r['path']:r for r in binding['inputs']};witness=document.source_witness if isinstance(document,MandatoryCapture) else document['type_context_witness']
    if isinstance(document,MandatoryCapture):same(document.ingress['sha256'],case_row['sha256'],'actual streamed complete capture/index byte binding');same(document.report['runs'][0]['source_build_ref'],Path(case_row['executionReceipt']['path']).name,'actual report/source execution receipt')
    if Path(binding['runtime']['path']).name=='mandatory-trigger-game':
        sources=unique(witness['source_inputs'],'path','actual mandatory witnessed source');same(set(sources),mandatory_source_inventory(cwd,binding['inputs']),'actual exact complete declared mandatory source inventory');same(len(sources),34,'actual required34 supplied source members')
        for source in sources.values():same(set(source),{'path','sha256','bytes'},'exact mandatory source receipt fields');require(type(source['bytes']) is int and source['bytes']>0,'actual source byte size');require(re.fullmatch(r'[0-9a-f]{64}',source['sha256']) is not None,'actual canonical source digest')
    for source in witness['source_inputs']:
        path=Path(source['path']);require(path.is_absolute() and path.is_file(),'actual witnessed source exists');same(path.stat().st_size,source['bytes'],'actual witnessed source byte count');same(sha(path),source['sha256'],'actual witnessed source bytes')
        if source['path'] in inputs:same(source['sha256'],inputs[source['path']]['sha256'],'actual witness copied input binding')
        else:
            require(path.resolve().is_relative_to(cwd),'actual witnessed source belongs to checked native root');relative=str(path.resolve().relative_to(cwd));require(relative in manifest,'actual witness source included in checked manifest');same(source['sha256'],manifest[relative],'actual witness/final source digest')
    for field,kind in [('input','vocabulary'),('publication','publication')]:
        source=witness['vocabulary'][field];row=next(r for r in binding['inputs'] if r['sha256']==NATIVE_INPUT_PINS[kind]);same({k:source[k] for k in ['path','sha256']},row,'actual witness '+kind+' file binding');same(source['bytes'],Path(row['path']).stat().st_size,'actual witness '+kind+' bytes')


@evidence_boundary
def validate_supplemental_binding(supplemental:dict[str,Any],binding:dict[str,Any],captured_cases:dict[str,Any])->dict[str,Any]:
    """Bind compact actual Rust test summaries, retaining their separate evidence depth."""
    same(supplemental['schemaVersion'],1,'supplemental binding schema');same(supplemental['kind'],'actual-blue-interaction-supplemental-test-binding','actual supplemental binding kind');stamp=datetime.fromisoformat(supplemental['boundAt'].replace('Z','+00:00'));require(stamp.tzinfo is not None and stamp.utcoffset().total_seconds()==0,'supplemental binding UTC timestamp')
    same(supplemental['sourceManifest'],binding['sourceManifest'],'supplemental exact native source epoch');same(supplemental['buildReceipt'],binding['runtime']['buildReceipt'],'supplemental exact native checker epoch');files=unique(supplemental['files'],'name','supplemental file name');same(set(files),{'interaction-actual','interaction-negative'},'exact two supplemental files');unique(supplemental['files'],'path','supplemental file path')
    check_dir=Path(supplemental['buildReceipt']['path']).parent;commands=read(check_dir/'commands.json');build=read(Path(supplemental['buildReceipt']['path']));row=next(r for r in build if r['name']=='interaction-tests');same(row['exitCode'],0,'actual successful supplemental test command');same(row['command'],['cargo','test','--locked','-p','spellwake-reference-tools','--test','interaction_games','--','--include-ignored'],'actual exact interaction-tests command')
    environment=commands['explicit_environment'];summaries={}
    for name,key in [('interaction-actual','SPELLWAKE_INTERACTION_CHECK_RECEIPT'),('interaction-negative','SPELLWAKE_INTERACTION_NEGATIVE_RECEIPT')]:
        receipt=files[name];path=Path(receipt['path']);same(path.resolve(),Path(environment[key]).resolve(),'actual declared checker summary output '+name);require(path.is_file(),'actual checker summary exists');same(sha(path),receipt['sha256'],'actual separately bound summary bytes');summaries[name]=read(path)
    log=Path(row['log']);text=log.read_text()
    for name in ['actual_interaction_games_and_prepared_edges_reexecute_complete_journals_and_state','prepared_negative_timing_completed_target_and_creature_spell_probes_are_atomic']:require('test '+name+' ... ok' in text,'actual successful test log '+name)
    actual=summaries['interaction-actual'];same(actual['kind'],'actual-development-test-receipt','actual normal-case test summary kind');cases=unique(actual['cases'],'case','actual test case summary');same(set(cases),{f'IF{i:02}' for i in range(1,13)}|{'IF15','BI01','BI02'},'actual fifteen test summaries')
    for name,case in cases.items():
        same(case['completed'],True,'actual test case completed')
        if name in captured_cases:
            for field in ['accepted','rerun','final']:same(case[field],captured_cases[name][field],'actual test/captured '+name+' '+field)
    negative=summaries['interaction-negative'];same(negative['kind'],'actual-source-prepared-negative-tests','actual separate prepared-negative summary kind');same(set(negative['root_command_rejections']),SUPPLEMENTAL_PROBES|{'IF13.counter-completed-entry'},'actual compact negative summary coverage');same(len(negative['root_command_rejections']),4,'actual no duplicate summary families');same(negative['all_no_candidate_and_full_state_retained'],True,'actual negative test assertions passed');same(negative['prepared_source_cases'],['IF06','IF02'],'actual disclosed prepared source cases')
    return dict(disposition='separately_bound_actual_test_summary_not_RunReport_or_witness',bound_at=supplemental['boundAt'],binding_time_scope='Hashes bound after successful checker execution, not recorded at test-process exit',files=supplemental['files'],interaction_test_command=row['command'],interaction_test_log=dict(path=str(log),sha256=sha(log)),supplemental_verified_kinds=sorted(SUPPLEMENTAL_PROBES),runtime_effect_depth='Actual typed-error/no-candidate/full-state-retention assertions in the bound Rust test; compact summary records their result, not command-level frames or full states')


@evidence_boundary
def compare_native_files(run_dir:Path,native_paths:list[Path],allow_uncaptured:bool,binding:dict[str,Any]|None=None,selected:list[str]|None=None,supplemental:dict[str,Any]|None=None)->dict[str,Any]:
    """Consume independent per-case files sequentially without retaining aggregate witnesses."""
    inputs=read(run_dir/'inputs.json');manifest=read(run_dir/'manifest.json');full=read(run_dir/'cases-v1.json');focused=read(run_dir/'focused-cases-v3.json');cases=unique(inputs['cases'],'case_id','reference frozen case');indexed=validate_image_binding(binding) if binding is not None else None
    require(indexed is not None or allow_uncaptured,'final native adoption requires image binding')
    if indexed is not None:
        same(set(indexed),set(cases),'final native image exact15 case inventory')
        if not native_paths:native_paths=[Path(row['path']) for row in indexed.values()]
        same({str(p.resolve()) for p in native_paths},{str(Path(r['path']).resolve()) for r in indexed.values()},'indexed exact capture file set')
    seen={};provenances=[];probe_routes={};test_case_facts={};observed_probe_families=set()
    for path in native_paths:
        document=read(path);report,provenance=unwrap_native(document,allow_uncaptured,manifest);provenances.append(provenance)
        for run in report['runs']:
            name=run['fixture_ref'];require(name in cases and name not in seen,'native exact unique selected cases')
            if selected is not None and name not in selected:continue
            if indexed is not None:
                same(str(path.resolve()),str(Path(indexed[name]['path']).resolve()),'actual native indexed case file');same(sha(path),indexed[name]['sha256'],'actual native indexed bytes');same(provenance['captured'],True,'final actual witness required');validate_captured_sources(document,binding,indexed[name])
            case=cases[name];focus=next((c for c in focused['scenarios'] if c['id']==name),None);native=native_projection(run,case,full,manifest,focus,qualify_probes=indexed is not None);raw=read(run_dir/'evidence'/f'{name}.json');reference=forge_projection(raw,case,full,manifest,inputs);assert_expected_facts(reference,raw,next((c for c in full['cases'] if c['id']==name),None),focus);joined=join_native_reference(native,reference,case);joined['native_sha256']=sha(path);joined['captured']=provenance['captured'];seen[name]=joined
            for family,routes in native['probe_evidence'].items():probe_routes.setdefault(family,[]).extend(dict(route,capture_path=str(path),capture_sha256=indexed[name]['sha256']) for route in routes)
            test_case_facts[name]=dict(accepted=len(run['accepted']),rerun=run['rerun'],final=run['final'])
            observed_probe_families.update(native['probe_families'])
        # No packet, point graph or complete run is retained into the next case.
        del document,report
    expected=set(selected) if selected is not None else set(cases);same(set(seen),expected,'actual native/reference full selected case inventory')
    requested={r['id'] for r in focused['negative_probe_suite']['expanded_attempts']};unrepresentable={name for name in requested if name.endswith('.extra-second')};captured_required=requested-SUPPLEMENTAL_PROBES-unrepresentable;same(len(captured_required),20,'frozen captured negative kinds');same(len(unrepresentable),3,'frozen typed-root-unrepresentable kinds');missing=sorted(captured_required-set(probe_routes))
    if indexed is not None and selected is None:same(missing,[],'all20 requested captured negative kinds have actual matching context routes')
    negative_coverage=dict(requested_kinds=26,captured_required_kinds=20,captured_verified_kinds=sorted(set(probe_routes)&captured_required),captured_missing=missing,actual_captured_evidence_routes=probe_routes,supplemental_test_only_pending=sorted(SUPPLEMENTAL_PROBES),typed_root_unrepresentable=[dict(id=name,runtime_attempts=0,reason='Root CastPaid/TargetRef carries one target; no extra-second-target command exists') for name in sorted(unrepresentable)],self_target=dict(disposition='source_only_not_runtime',rule='CR115.5',prospective_entry_not_invented=True))
    if supplemental is not None:
        require(binding is not None,'supplemental evidence requires actual final native binding');negative_coverage['supplemental_test_evidence']=validate_supplemental_binding(supplemental,binding,test_case_facts);negative_coverage['supplemental_test_only_pending']=[]
    negative_coverage['additional_observed_captured_kind_names']=sorted(observed_probe_families-requested);negative_coverage['all23_root_representable_requested_verified']=indexed is not None and selected is None and not missing and supplemental is not None
    qualified=indexed is not None and all(p['captured'] and p['complete'] for p in provenances)
    return dict(status='actual_final_image_join' if qualified else 'development_schema_join_not_final_qualification',final_image_adopted=qualified,cases=seen,case_count=len(seen),source_bound_registry=6,processing='one actual per-case witness at a time, no full-packet copies',negative_coverage=negative_coverage,native_image_binding=hashlib.sha256(json.dumps(binding,sort_keys=True).encode()).hexdigest() if binding is not None else None)



MANDATORY_METHODS=METHODS|{'orderAndPlaySimultaneousSa','playSpellAbilityNoStack','orderMoveToZoneList'}
MANDATORY_UNAVAILABLE=['private waiting queue','identical-token relative timestamp discriminator','private failed-draw flag','dedicated token cessation event']


def mandatory_program_view(contract:dict[str,Any],vocabulary:dict[str,Any])->dict[str,Any]:
    """Encode the reviewed declarative Program contract in the existing canonical observation grammar."""
    def subtype(label:str)->dict[str,str]:return dict(ordinal=str(vocabulary['members'].index(label)),vocabulary=vocabulary['id'])
    def body(value:dict[str,Any])->dict[str,Any]:
        target=value['target'];target=dict(tag=target) if isinstance(target,str) else dict(tag=target['variant'],support_gate=target['filter']);instructions=[]
        for raw in value['instructions']:
            op=raw['variant'];row=dict(op=op)
            if op=='DrawController':row['amount']=dict(tag=raw['amount']['variant'],value=raw['amount']['value'])
            elif op in ('AdjustControllerLife','AdjustTargetLife'):row['adjustment']=dict(tag=raw['adjustment']['variant'],amount=raw['adjustment']['value'])
            elif op=='CreateTokens':
                creations=[]
                for creation in raw['creations']:
                    definition=creation['definition'];name=definition['name'];require(name['variant']=='Default','finite exact default token name contract')
                    token_name=dict(tag='Default',ordered_subtypes=[dict(family=word['variant'],type=subtype(word['label'])) for word in name['ordered_subtypes']]);creations.append(dict(definition=dict(program=mandatory_program_view(definition['program'],vocabulary),name=token_name),amount=dict(tag=creation['amount']['variant'],value=creation['amount']['value']),recipients=creation['recipients']))
                row['creations']=creations
            else:require(op in ('CounterSpell','ReturnToOwnersHand'),'finite source-backed instruction contract')
            instructions.append(row)
        return dict(target=target,instructions=instructions)
    kind=contract['kind'];permanent=kind['variant']=='Permanent'
    if permanent:
        pt=kind['printed_pt'];encoded_kind=dict(tag='Permanent',printed_pt=None if pt is None else dict(power=dict(tag='Number',value=pt['power']),toughness=dict(tag='Number',value=pt['toughness'])),casting_cost=kind['casting_cost'])
    else:encoded_kind=dict(tag='Spell',casting_cost=kind['cost'],body=body(kind['body']))
    return dict(card_types=contract['card_types'],creature_subtypes=sorted([subtype(label) for label in contract['creature_subtypes']],key=lambda row:int(row['ordinal'])),land_subtypes=contract['land_subtypes'],names=contract['names']['values'],supertypes=contract['supertypes'],kind=encoded_kind,color_indicator=contract['color_indicator'],effect_defined_colors=contract['effect_defined_colors'],mana=[dict(id=row['id']['value'],output=dict(tag=row['output']['variant'],produced=row['output']['produced'])) for row in contract['mana']],activated=contract['activated'],triggered=[dict(id=row['id']['value'],condition=dict(tag=row['condition']['variant'],object=row['condition']['object_relation']),grouping=row['grouping'],body=body(row['body'])) for row in contract['triggered']],continuous=contract['continuous'])


@evidence_boundary
def validate_mandatory_native_definitions(definitions:list[dict[str,Any]],manifest:dict[str,Any],vocabulary:dict[str,Any],selected_ids:set[str]|None=None)->dict[str,dict[str,Any]]:
    """Require every actual whole Program/printing/source field for all19 definitions and both nested templates."""
    same(vocabulary['id'],manifest['vocabulary']['id'],'actual source-qualified subtype vocabulary');rows=unique(definitions,'oracle_id','actual mandatory source definition');all_ids={card['oracleId'] for card in manifest['cards']};required=all_ids if selected_ids is None else selected_ids;require(required<=all_ids,'only admitted source19 identities');same(set(rows),required,'exact actually used source definitions; all23 union19 checked separately')
    for card in [card for card in manifest['cards'] if card['oracleId'] in required]:
        row=rows[card['oracleId']]
        for field,value in [('reference',card['oracleId']),('name',card['name']),('printing_id',card['printing']['id']),('set',card['printing']['set']),('collector',card['printing']['collector_number']),('language',card['printing']['lang'])]:same(row[field],value,'actual mandatory source '+field)
        require(any(source['sha256']==manifest['catalogue']['sha256'] for source in row['sources']),'actual source-qualified definition receipt');same(row['program'],mandatory_program_view(card['programContract'],vocabulary),'actual complete mandatory Program '+card['name'])
    return rows


@evidence_boundary
def validate_mandatory_process(run_dir:Path,commands:dict[str,Any],results:dict[str,Any],stability:dict[str,Any],binding:dict[str,Any])->None:
    """Bind completed compiler/JVM processes, emitted bytes, streams and frozen authority guards."""
    same(set(results),{'compile','run'},'actual mandatory compiler and JVM process inventory')
    for name,row in results.items():
        same(row['exit'],0,'actual '+name+' process exit');same(row['command'],commands[name],'actual process/command receipt');same(row['cwd'],commands['cwd'],'actual process working directory');same(row['interrupted'],False,'actual process uninterrupted');same(row['termination_reason'],None,'actual no forced termination');require(type(row['started_unix']) in (int,float) and type(row['completed_unix']) in (int,float) and row['started_unix']<=row['completed_unix'],'actual process timestamps')
        for stream in ['stdout','stderr']:
            receipt=row[stream];path=Path(receipt['path']);same(path.resolve(),(run_dir/(name+('.log' if stream=='stdout' else '.stderr'))).resolve(),'actual process stream path');same(sha(path),receipt['sha256'],'actual process stream bytes');same(path.stat().st_size,receipt['bytes'],'actual process stream byte count')
    require(results['compile']['completed_unix']<=results['run']['started_unix'],'actual compile before runtime')
    same(commands['forge_commit'],PIN,'actual compiler/runtime Forge source commit');same(commands['compile'][-1],str((run_dir/'source/SpellwakeBlueInteractionReferenceTest.java').resolve()),'actual maintained compiler source');same(commands['compile'][-2],str((run_dir/'classes').resolve()),'actual compiler emitted-class directory')
    for flag,value in [('spellwake.inputs.path',str((run_dir/'inputs.json').resolve())),('spellwake.inputs.sha256',sha(run_dir/'inputs.json')),('spellwake.evidence.dir',str((run_dir/'evidence').resolve())),('spellwake.selected.cases',','.join(commands['selected']))]:same([part for part in commands['run'] if part.startswith('-D'+flag+'=')],['-D'+flag+'='+value],'actual explicit JVM '+flag)
    require('org.testng.TestNG' in commands['run'] and commands['run'][-1]==str((run_dir/'suite.xml').resolve()),'actual TestNG process source')
    same(set(results['run']['evidence']),{str(path.relative_to(run_dir)) for path in (run_dir/'evidence').glob('*.json')},'actual process exact raw output inventory')
    for relative,receipt in results['run']['evidence'].items():same(sha(run_dir/relative),receipt['sha256'],'actual as-produced raw evidence bytes');same((run_dir/relative).stat().st_size,receipt['bytes'],'actual as-produced raw byte count')
    expected={row['path']:row['sha256'] for row in [*binding['authorities'].values(),*binding['supplied'].values(),*binding['primary_payloads']]};same(stability['authority_before'],expected,'actual complete authority guard inventory');same(stability['authority_after'],expected,'actual complete authority postguard inventory');same(stability['authority_unchanged'],True,'actual authority pre/post flag')
    for path,digest in expected.items():same(sha(Path(path)),digest,'actual authority current bytes')


@evidence_boundary
def mandatory_world(world:dict[str,Any],raw:dict[str,Any],case:dict[str,Any])->dict[str,Any]:
    """Check actual physical conservation and live-token endpoint identity without inventing cessation."""
    physical=unique(case['inventory'],'alias','physical input');tokens=unique(raw['token_receipts'],'alias','token birth');known=set(physical)|set(tokens)
    same(set(world['players']),{'P0','P1'},'mandatory world seats');require(set(physical)<=set(world['cards'])<=known,'mandatory world complete physical/known token inventory')
    seen=[];card_ids=[]
    for role,player in world['players'].items():
        same(player['life'],world['header']['players'][role]['life'],'actual world/header life');same(player['mana'],world['header']['players'][role]['mana'],'actual world/header mana');integer(player['life'],'actual life')
        same(set(player['mana']),set('WUBRGC'),'actual mana colors')
        for amount in player['mana'].values():require(integer(amount,'actual mana')>=0,'nonnegative actual mana')
        for zone in ['library','hand','battlefield','graveyard','exile']:
            same(len(player[zone]),world['header']['players'][role]['counts'][zone],'mandatory header/pile count')
            for alias in player[zone]:
                require(alias in world['cards'],'pile object actually present');card=world['cards'][alias];same(card['owner'],role,'actual owner partition');same(card['zone'],zone,'actual zone partition');seen.append(alias)
        same(len([alias for alias in physical if physical[alias]['owner']==role]),sum(1 for alias in world['cards'] if alias in physical and world['cards'][alias]['owner']==role),'complete owner physical count')
    for entry in world['stack']:
        ability=entry['ability'];require(ability['source'] in known,'known stack source');same(entry['controller'],next(r['captured_controller'] for r in raw['trigger_receipts'] if r['actual_ability']['ability_id']==ability['ability_id']) if not ability['spell'] else physical[ability['source']]['owner'],'actual stack controller')
        if ability['spell'] and world['cards'][ability['source']]['zone']=='stack':seen.append(ability['source'])
        elif ability['spell']:same(entry['resolving'],True,'departed spell source has actual resolving entry');require(world['cards'][ability['source']]['zone'] in ('battlefield','graveyard'),'ordinary resolving spell source destination')
    same(len(seen),len(set(seen)),'no duplicate actual zone membership');same(set(seen),set(world['cards']),'actual world complete membership')
    for alias,card in world['cards'].items():
        same(card['physical_id'],str(raw['physical_aliases'][alias]) if alias in physical else tokens[alias]['physical_id'],'actual stable within-engine physical ID');card_ids.append(card['physical_id']);integer(card['timestamp'],'actual appearance timestamp')
        same(card['token'],alias in tokens,'physical/token distinction');same(card['owner_player_id'],raw['registered_players'][card['owner']]['player_id'],'actual owner ID')
        same(card['counter_inventory'],sorted(card['counter_inventory'],key=lambda row:json.dumps(row,sort_keys=True)),'canonical actual counter inventory')
        require(len({r['kind'] for r in card['counter_inventory']})==len(card['counter_inventory']),'unique actual counters')
        for row in card['counter_inventory']:require(integer(row['amount'],'actual stored counters')>0,'positive stored counter entry')
        if card['power'] is not None:
            for key in ['power','toughness','marked_damage','plus_one_counters','minus_one_counters']:integer(card[key],'actual creature '+key)
            require(type(card['sick']) is bool,'actual creature control-age boolean')
        if alias in tokens:
            same(card['name'],tokens[alias]['name'],'token exact name');same(card['owner'],tokens[alias]['creator'],'token owner vs creation recipient')
            same(card['power'],tokens[alias]['state']['power'],'actual finite token power');same(card['toughness'],tokens[alias]['state']['toughness'],'actual finite token toughness');same(card['counter_inventory'],[],'finite unmodified token counter inventory')
        else:same(card['name'],physical[alias]['definition_ref'],'actual physical source name')
    same(len(card_ids),len(set(card_ids)),'unique actual physical/token identities')
    return dict(life={role:player['life'] for role,player in world['players'].items()},cards=world['cards'],players=world['players'],stack=world['stack'])


def selected_forge_printings(raw:dict[str,Any],case:dict[str,Any],inputs:dict[str,Any])->None:
    """The shared complete selected-printing observer, moved from the maintained mandatory reader."""
    same(set(raw['physical_aliases']),{r['alias'] for r in case['inventory']},'exact declared physical inventory')
    same(set(raw['paper_cards']),set(inputs['definitions']),'exact19 loaded source definitions')
    for name,definition in inputs['definitions'].items():
        actual=raw['paper_cards'][name];face=definition['face']
        # Optional inputs retain the saved lowercase set code; PaperCard exposes
        # Forge's uppercase edition code. Historical input conventions stay exact.
        selected_set=definition['set'].upper() if raw['profile']=='optional-payment-v1' else definition['set']
        for key,value in [('name',name),('set',selected_set),('collector_number',definition['collector_number']),('oracle_id',definition['oracle_id']),('foil',False),('oracle_text',face['oracle_text']),('mana_cost',face['mana_cost'] or 'no cost'),('printed_power',face['power']),('printed_toughness',face['toughness']),('type',face['type_line'].replace('—','-')),('color_mask',definition['color_mask']),('source_receipt',definition['source_receipt'])]:same(actual[key],value,'mandatory printing '+name+'/'+key)
        require(type(actual['art_index']) is int and actual['art_index']>0,'actual selected art index');same(actual['functional_variant'],'','actual no functional variant')
        before,_,after=face['type_line'].partition(' — ')
        same(actual['supertypes'],['Basic'] if before.startswith('Basic ') else [],'actual complete supertypes');same(actual['core_types'],[before.removeprefix('Basic ')],'actual complete core types');same(actual['subtypes'],sorted(after.split()) if after else [],'actual complete subtype set');same(actual['basic_land'],before=='Basic Land','actual basic land flag');same(actual['creature'],before=='Creature','actual creature flag')


def selected_forge_stack_target(raw:dict[str,Any],selector:str,target:dict[str,Any])->dict[str,Any]:
    """Resolve actual source labels for optional entries, preserving old physical selectors."""
    if raw['profile']!='optional-payment-v1':
        return next(row for row in raw['cast_operations'] if row['source']==selector and row['ability_id']==target['ability_id'])
    casts=[row for row in raw['cast_operations'] if row['actual_ability']['entry_label']==selector and row['ability_id']==target['ability_id']]
    if casts:same(len(casts),1,'actual target cast entry unique');return casts[0]
    copies=[row for row in raw['spell_copies'] if row['copy_label']==selector and row['copy_ability']['ability_id']==target['ability_id']]
    same(len(copies),1,'actual exact copied target entry')
    copy=copies[0]
    return dict(stack_id=copy['copy_stack_id'],source_timestamp=copy['copy_ability']['source_timestamp'])


def selected_forge_trigger_callback(raw:dict[str,Any],receipt:dict[str,Any],declaration:dict[str,Any],group:dict[str,Any],entries:dict[int,Any],returns:dict[int,Any],player_choices:list[dict[str,Any]]|None=None)->dict[str,Any]:
    """One actual placement callback/complete source-selector relation owner."""
    same(receipt['input_selector'],declaration,'actual trigger complete source selector');same(receipt['group'],group['name'],'actual trigger declared group');call=entries[receipt['callback_id']];same(call['method'],'orderAndPlaySimultaneousSa','actual trigger placement callback owner');same(call['actor'],declaration['captured_controller'],'actual trigger placement captured actor');reply=returns[receipt['callback_id']]['reply'];same(reply['group'],group['name'],'actual trigger callback/source group association');expected=[name for name in group['order_bottom_first'] if next(row for row in group['triggers'] if row['name']==name)['captured_controller']==call['actor']];same(reply['selected_bottom_first'],expected,'actual complete source callback placement order');same([row['trigger'] for row in reply['admitted']],expected,'actual complete admitted source trigger order')
    offers=[row for row in call['arguments']['domain'] if row['ability_id']==receipt['actual_ability']['ability_id']];same(len(offers),1,'actual source trigger in this exact offered callback domain');offered=offers[0]
    for field in ('source','source_physical_id','source_timestamp','spell','mana_ability','land_ability'):same(offered[field],receipt['actual_ability'][field],'actual trigger offered/captured complete '+field)
    installed=[row for row in reply['admitted'] if row['trigger']==receipt['name']];same(len(installed),1,'actual trigger exact callback installed member');ability=installed[0]['ability'];same({k:v for k,v in ability.items() if k!='target_players'},{k:v for k,v in receipt['actual_ability'].items() if k!='target_players'},'actual complete callback-installed captured ability before player target selection')
    choices=[choice for choice in (player_choices or []) if choice['trigger']==receipt['name']]
    if choices:
        same(len(choices),1,'actual one source player-target choice');choice=choices[0];target_calls=[row for row in entries.values() if row['method']=='chooseTargetsFor' and row['arguments'].get('trigger')==receipt['name']];same(len(target_calls),1,'actual source trigger player-target callback');target=target_calls[0];same(target['actor'],choice['chooser'],'actual source player-target chooser');same(set(target['arguments']['actual_player_domain']),set(choice['domain_roles']),'actual complete source player-target domain');same(len(target['arguments']['actual_player_domain']),len(choice['domain_roles']),'actual distinct player-target domain');same(returns[target['callback_id']]['reply']['actual_ability']['target_players'],[choice['target']],'actual source player target selected');same(ability['target_players'],[choice['target']],'actual installed player target preserves selection')
    else:same(ability['target_players'],receipt['actual_ability']['target_players'],'actual no undeclared installed player-target change')
    return call


def selected_forge_opening(raw:dict[str,Any],case:dict[str,Any],entries:dict[int,Any],returns:dict[int,Any])->dict[str,Any]:
    """The shared actual keep/starter/shuffle observer for source-owned openings."""
    complete=raw['completion']
    opening_calls=[row for row in entries.values() if row['method']=='mulliganKeepHand'];same([row['actor'] for row in opening_calls],[row['chooser'] for row in case['opening_policy']],'actual starter-first keep callback order')
    for call,policy in zip(opening_calls,case['opening_policy'],strict=True):same(returns[call['callback_id']]['reply']['input_policy'],policy,'actual opening reply policy');same(returns[call['callback_id']]['reply']['keep'],True,'actual keep-seven choice');same(call['arguments']['charged'],policy['taken'],'actual opening charge')
    if case['initialization']['kind']=='opening':
        same(complete['natural_start_game_return'],True,'actual full-game natural return');same(complete['game_finished_event'],True,'actual full-game completion notification')
        starters=[row for row in entries.values() if row['method']=='chooseStartingPlayer'];same(len(starters),1,'actual starting chooser');same(starters[0]['actor'],'P0','actual entropy-selected chooser');same(returns[starters[0]['callback_id']]['reply']['selected'],case['starting_role'],'actual chosen starter')
        operations=[row for row in raw['random'] if row['kind']=='operation_commit'];same(len(operations),2,'actual two initial library shuffles')
        for op,policy in zip(operations,case['shuffle_operations'],strict=True):same(op['operation_id'],policy['operation_id'],'actual shuffle operation identity');same(op['actual_committed'],policy['target_aliases'],'actual complete library permutation')
    return dict(keep_callbacks=len(opening_calls),initial_shuffles=2 if case['initialization']['kind']=='opening' else 0)


def selected_forge_casts(raw:dict[str,Any],case:dict[str,Any],entries:dict[int,Any])->tuple[list[dict[str,Any]],list[dict[str,Any]],dict[str,Any]]:
    """The maintained source/cost/physical/target/debit/funding cast observer."""
    returns={row['callback_id']:row for row in raw['callbacks'] if row['kind']=='return'}
    actions=[row for row in raw['operations'] if row['kind']=='ordinary_entry'];planned=[row for row in case['actions'] if row['kind'] in ('play_land','activate_mana','cast')]
    same([row['policy'] for row in actions],planned,'complete actual paid-action sequence');unique(actions,'operation_id','actual ordinary operation')
    casts=unique(raw['cast_operations'],'action_id','actual paid cast');same(set(casts),{r['policy_id'] for r in planned if r['kind']=='cast'},'all actual cast operations')
    for action in planned:
        if action['kind']!='cast':continue
        cast=casts[action['policy_id']];same(cast['source'],action['alias'],'actual cast source');same(cast['payer'],action['seat'],'actual cast payer');same(cast['required_cost'],action['required_cost'],'actual printed cost');same(cast['paid_sources'],action['paid_sources'],'actual funding-source names')
        same(cast['actual_ability']['ability_id'],cast['ability_id'],'actual cast/ability ID');same(cast['actual_ability']['source_timestamp'],cast['source_timestamp'],'actual cast source incarnation');same(cast['actual_ability']['source_physical_id'],cast['source_physical_id'],'actual cast physical source');same(cast['source_physical_id'],str(raw['physical_aliases'][action['alias']]),'actual cast physical alias binding')
        notifications=[row for row in raw['events'] if row['kind']=='GameEventSpellAbilityCast' and row['payload']['sa']['ability_id']==cast['ability_id']];same(len(notifications),1,'actual single cast notification');same(notifications[0]['payload']['si']['stack_id'],cast['stack_id'],'actual emitted stack ID binding')
        actual_targets=cast['actual_ability']['targets']
        if 'target' not in action:same(actual_targets,[],'untargeted actual cast')
        elif action['target']['kind']=='object':same(len(actual_targets),1,'one actual object target');same(actual_targets[0]['kind'],'object','actual target class');same(actual_targets[0]['alias'],action['target']['card'],'actual requested object target');same(actual_targets[0]['physical_id'],str(raw['physical_aliases'][action['target']['card']]),'actual target physical identity')
        elif action['target']['kind']=='stack':
            same(len(actual_targets),1,'one actual stack target');same(actual_targets[0]['kind'],'stack','actual target class');target=selected_forge_stack_target(raw,action['target']['spell'],actual_targets[0]);same(actual_targets[0]['stack_id'],target['stack_id'],'actual target entry binding');same(actual_targets[0]['source_timestamp'],target['source_timestamp'],'actual target stack source incarnation')
        admission=[r for r in raw['callbacks'] if r['kind']=='entry' and r['method']=='playChosenSpellAbility' and r['arguments']['policy']['policy_id']==action['policy_id']];same(len(admission),1,'one actual cast admission');call=admission[0]
        paid=[r for r in raw['callbacks'] if r['kind']=='entry' and r['method']=='applyManaToCost' and call['global_sequence']<r['global_sequence']<returns[call['callback_id']]['global_sequence'] and r['arguments']['ability']['ability_id']==cast['ability_id']];same(len(paid),1,'one actual mana debit callback');debit=paid[0];reply=returns[debit['callback_id']]['reply'];same(reply['paid'],True,'actual mana payment completion');same(len(reply['spent']),action['mana_units_paid'],'actual paid amount');same([r['token']['source'] for r in reply['spent']],action['paid_sources'],'actual spent source receipts');same(reply['spent'][-1]['unpaid_after'],'0','actual no unpaid remainder')
        for spent in reply['spent']:
            token=spent['token'];production=[r for r in actions if r['operation_id']==token['production_operation_id']];same(len(production),1,'actual mana-production operation');same(production[0]['policy']['alias'],token['source'],'actual mana source join');same(production[0]['policy']['mana_color'],next(k for k,v in MANA.items() if v==token['color']),'actual produced color');require(production[0]['global_sequence']<call['global_sequence'],'production precedes payment')
            same(token['source_timestamp'],production[0]['ability']['source_timestamp'],'actual paid mana source incarnation');same(token['output_index'],0,'actual finite mana output index')
    return actions,planned,casts


def selected_forge_token_births(raw:dict[str,Any],casts:dict[str,Any],triggers:dict[str,Any])->dict[str,Any]:
    """Observe complete real source-coordinate births, including printing and cohort."""
    events={row['global_sequence']:row for row in raw['events']}
    token_rows=unique(raw['token_receipts'],'alias','actual token births')
    for alias,token in token_rows.items():
        source=token['creation_entry'];require(source in casts or source in triggers,'actual known creating entry');same(alias,token_birth_alias(source,token['instruction'],token['creator'],token['member']),'actual creation coordinate');same(token['instruction'],0,'finite token instruction');same(token['state']['physical_id'],token['physical_id'],'birth identity');same(token['state']['timestamp'],token['timestamp'],'birth timestamp');same(token['state']['owner'],token['creator'],'actual recipient/owner');same(token['state']['controller'],token['creator'],'actual token controller');same(token['keywords'],[],'token no keyword abilities');same(token['triggered_count'],0,'token no trigger abilities');same(token['activated_count'],0,'token no activated or mana abilities');same(token['core_types'],['Creature'],'token exact core types');same(token['supertypes'],[],'token no supertypes');same(token['mana_cost'],'no cost','token absent casting cost')
        soldier=token['name']=='Soldier Token';same(token['name'],'Soldier Token' if soldier else 'Zombie Token','exact default token name');same(token['color_mask'],1 if soldier else 4,'token exact color');same(token['subtypes'],['Soldier'] if soldier else ['Zombie'],'token exact subtypes');same(token['state']['power'],'1' if soldier else '2','token actual power');same(token['state']['toughness'],'1' if soldier else '2','token actual toughness');same(token['printing']['name'],token['name'],'actual token printing name');same(token['printing']['foil'],False,'actual nonfoil token printing');require(type(token['printing']['art_index']) is int and token['printing']['art_index']>0,'actual token art index')
        event=events[token['observed_before_sequence']];same(event['kind'],'GameEventCardChangeZone','actual birth notification');same(event['payload']['card']['alias'],alias,'actual token birth event join');same(event['payload']['from'],None,'actual token creation from no zone');same(event['payload']['to']['zone_type'],'battlefield','actual birth destination')
        same(token['state']['marked_damage'],'0','actual zero stored damage at birth');same(token['state']['plus_one_counters'],'0','actual zero stored positive counters at birth');same(token['state']['minus_one_counters'],'0','actual zero stored negative counters at birth');same(token['state']['counter_inventory'],[],'actual complete empty birth counter inventory')
    for source in {row['creation_entry'] for row in token_rows.values()}:
        cohort=[row for row in token_rows.values() if row['creation_entry']==source];same([r['member'] for r in cohort],list(range(len(cohort))),'complete creation cohort ordinals');same(len(cohort),2 if cohort[0]['name']=='Soldier Token' else 1,'source-exact token amount')
    return token_rows


@evidence_boundary
def mandatory_forge_projection(raw:dict[str,Any],case:dict[str,Any],inputs:dict[str,Any])->dict[str,Any]:
    """Independently bind callback/action/payment/trigger/token/body relationships in actual Forge output."""
    same(raw['profile'],PROFILES[1],'actual mandatory profile');same(raw['schema'],'interaction-game-forge-v1','maintained raw schema');same(raw['schema_version'],1,'raw schema1');same(raw['forge_commit'],PIN,'actual pinned Forge');same(raw['case_id'],case['case_id'],'actual case identity');same(raw['fixture'],case,'actual input-only case binding');same(raw['failures'],[],'no sticky failure');same(raw['unavailable'],MANDATORY_UNAVAILABLE,'honest unavailable fields')
    entries=validate_streams(raw,MANDATORY_METHODS,('P0','P1'));returns={r['callback_id']:r for r in raw['callbacks'] if r['kind']=='return'};events={r['global_sequence']:r for r in raw['events']}
    selected_forge_printings(raw,case,inputs)
    complete=raw['completion']
    for a,b in [('actions_consumed','actions_planned'),('decisions_consumed','decisions_planned'),('opening_consumed','opening_planned'),('random_operations_consumed','random_operations_planned')]:same(complete[a],complete[b],'actual complete '+a)
    same(complete['actions_planned'],len(case['actions']),'declared action denominator');same(complete['decisions_planned'],len(case['decisions']),'declared decision denominator');same(complete['sticky_failure_count'],0,'actual no sticky failures');same(complete['callback_depth'],0,'all actual callbacks complete')
    selected_forge_opening(raw,case,entries,returns)
    actions,planned,casts=selected_forge_casts(raw,case,entries)
    declarations={row['name']:row for group in case['trigger_groups'] for row in group['triggers']};triggers=unique(raw['trigger_receipts'],'name','actual trigger');same(set(triggers),set(declarations),'exact mandatory trigger inventory, including each occurrence')
    for name,receipt in triggers.items():
        declaration=declarations[name];same(receipt['input_selector'],declaration,'actual trigger input selector');same(receipt['captured_controller'],declaration['captured_controller'],'actual captured controller');same(receipt['actual_ability']['source'],reference_alias(declaration['source']),'actual source relation');same(receipt['subject'],reference_alias(declaration['subject']),'actual occurrence subject');same(receipt['trigger_mode'],'ChangesZone','actual trigger family');require(type(receipt['trigger_id']) is int and receipt['trigger_id']>0,'actual engine-local definition witness');same(receipt['actual_source_state']['timestamp'],receipt['actual_ability']['source_timestamp'],'actual captured source incarnation')
        callback=selected_forge_trigger_callback(raw,receipt,declaration,next(group for group in case['trigger_groups'] if receipt['name'] in [row['name'] for row in group['triggers']]),entries,returns,case['player_choices']);same(receipt['causal_events'],raw['causal_events'][declaration['cause_operation']],'actual complete cause members')
        matches=[row for row in receipt['causal_events'] if row['subject']==receipt['subject'] and row['event']==declaration['event']];same(len(matches),1,'exact occurrence/cause join')
        for cause in receipt['causal_events']:
            event=events[cause['source_global_sequence']];same(event['kind'],'GameEventCardChangeZone','actual cause event family');same(event['payload']['card'],cause['card'],'actual cause card identity');same(event['payload']['from'],cause['from'],'actual cause from');same(event['payload']['to'],cause['to'],'actual cause to');same(cause['subject'],cause['card']['alias'],'actual cause subject');require(event['global_sequence']<callback['global_sequence'],'cause precedes trigger admission')
        admitted=[r for r in returns[receipt['callback_id']]['reply']['admitted'] if r['trigger']==name];same(len(admitted),1,'actual installed trigger identity');same(admitted[0]['ability']['source_timestamp'],receipt['actual_ability']['source_timestamp'],'installed source preserves capture')
        bodies=[r for r in raw['callbacks'] if r['kind']=='return' and r['method']=='playSpellAbilityNoStack' and r['reply']['trigger']==name];same(len(bodies),1,'actual full installed trigger body completion');same(bodies[0]['reply']['body_complete'],True,'body completed before SBA');require(bodies[0]['global_sequence']>callback['global_sequence'],'body follows installation')
        body=bodies[0];start=entries[body['callback_id']];during=[row for row in raw['events'] if start['global_sequence']<row['global_sequence']<body['global_sequence']]
        same([row for row in entries.values() if row['method']=='chooseSpellAbilityToPlay' and start['global_sequence']<row['global_sequence']<body['global_sequence']],[],'no priority inside mandatory body');same([row for row in during if row['kind']=='GameEventGameOutcome'],[],'full trigger body before state-based loss')
        source_name=inputs['definitions'][next(row['definition_ref'] for row in case['inventory'] if row['alias']==receipt['actual_ability']['source'])]['name'];life_rows=[row for row in during if row['kind']=='GameEventPlayerLivesChanged'];draw_rows=[row for row in during if row['kind']=='GameEventCardChangeZone' and row['payload']['from'] and row['payload']['from']['zone_type']=='library' and row['payload']['to'] and row['payload']['to']['zone_type']=='hand']
        expected_adjustments=[]
        if source_name in ('Soul Warden','Impassioned Orator'):expected_adjustments=[(receipt['captured_controller'],1)]
        elif source_name in ('Phyrexian Rager','Novice Occultist'):expected_adjustments=[(receipt['captured_controller'],-1)]
        elif source_name=='Blood Artist':expected_adjustments=[(next(row['target'] for row in case['player_choices'] if row['trigger']==name),-1),(receipt['captured_controller'],1)]
        same([(row['payload']['player'],integer(row['payload']['newLives'],'life instruction')-integer(row['payload']['oldLives'],'life instruction')) for row in life_rows],expected_adjustments,'actual ordered complete life instructions')
        if source_name in ('Elvish Visionary','Llanowar Visionary','Phyrexian Rager','Novice Occultist'):
            same(len(draw_rows),min(1,start['context']['players'][receipt['captured_controller']]['counts']['library']),'actual successful draw independently of private attempt flag')
            observed_draw_before_life([r['global_sequence'] for r in draw_rows],[r['global_sequence'] for r in life_rows],'actual mandatory Forge source body')
        else:same(draw_rows,[],'no undeclared draw in actual trigger body')
    for call in entries.values():
        if call['method']=='orderAndPlaySimultaneousSa':
            reply=returns[call['callback_id']]['reply'];group=next(g for g in case['trigger_groups'] if g['name']==reply['group']);expected=[name for name in group['order_bottom_first'] if declarations[name]['captured_controller']==call['actor']];same(reply['selected_bottom_first'],expected,'complete APNAP controller permutation');same([r['trigger'] for r in reply['admitted']],expected,'actual trigger bottom-first admission order')
            actual_ids=[row['ability_id'] for row in call['arguments']['domain']];same(set(actual_ids),{triggers[name]['actual_ability']['ability_id'] for name in expected},'actual offered full instance domain');same(len(actual_ids),len(set(actual_ids)),'actual distinct offered instances')
        if call['method']=='playSpellAbilityNoStack':require('forge.game.trigger.WrappedAbility.resolve' in call['actual_stack_tags'] and 'forge.game.zone.MagicStack.resolveStack' in call['actual_stack_tags'],'actual normal installed-wrapper resolution path')
    same(set(raw['used_player_choices']),{row['trigger'] for row in case['player_choices']},'all required actual player-target choices')
    for choice in case['player_choices']:
        calls=[r for r in entries.values() if r['method']=='chooseTargetsFor' and r['arguments'].get('trigger')==choice['trigger']];same(len(calls),1,'one actual player target frame');call=calls[0];same(call['actor'],choice['chooser'],'actual chooser');same(set(call['arguments']['actual_player_domain']),set(choice['domain_roles']),'semantic player-domain membership, actual order retained');same(len(call['arguments']['actual_player_domain']),2,'complete two-player target domain');same(returns[call['callback_id']]['reply']['actual_ability']['target_players'],[choice['target']],'actual selected player target')
    for group in case['trigger_groups']:
        calls=[row for row in entries.values() if row['method']=='orderAndPlaySimultaneousSa' and returns[row['callback_id']]['reply']['group']==group['name']]
        require(bool(calls),'actual APNAP callbacks exist');active=calls[0]['context']['active'];controllers={row['captured_controller'] for row in group['triggers']};same([row['actor'] for row in calls],[role for role in [active,'P1' if active=='P0' else 'P0'] if role in controllers],'actual APNAP controller callback order')
    token_rows=selected_forge_token_births(raw,casts,triggers)
    worlds={}
    selectors=unique(raw['observations'],'key','observation selector')
    sequence_rows={row['global_sequence']:row for stream in ['callbacks','events','random','operations'] for row in raw[stream]}
    required_actions={'action.'+str(index) for index,row in enumerate(case['actions']) if row['kind'] in ('play_land','activate_mana','cast','declare_attackers','declare_blockers')};require(required_actions<=set(selectors),'all actual action checkpoints retained');require({'body.'+name for name in triggers}<=set(selectors),'all actual body checkpoints retained');same({row['checkpoint_ref'] for row in selectors.values() if row['checkpoint_ref'] is not None},set(raw['checkpoints']),'complete reciprocal observation/checkpoint inventory')
    for key,row in selectors.items():
        require(row['source_global_sequence'] in sequence_rows,'actual observation source sequence');source=sequence_rows[row['source_global_sequence']];same(row['observation_id'],'observation-'+str(row['source_global_sequence']),'actual observation identity')
        if key.startswith('priority.'):
            same(source['kind'],'entry','actual priority observation cut');same(source['method'],'chooseSpellAbilityToPlay','actual priority observation owner');same(key,'priority.'+str(source['context']['individual_turn'])+'.'+source['context']['step'],'actual priority observation boundary')
        elif key.startswith('action.'):
            index=int(key.split('.')[1]);require(0<=index<len(case['actions']),'known actual action observation');policy=case['actions'][index]
            if policy['kind'] in ('declare_attackers','declare_blockers'):same(source['kind'],'GameEventAttackersDeclared' if policy['kind']=='declare_attackers' else 'GameEventBlockersDeclared','actual combat declaration observation')
            else:same(source['kind'],'return','actual action completion cut');same(source['method'],'playChosenSpellAbility','actual action completion owner');same(source['reply']['policy'],policy,'actual action checkpoint policy')
        elif key.startswith('body.'):same(source['method'],'playSpellAbilityNoStack','actual body observation owner');same(source['kind'],'return','actual body completion cut');same(source['reply']['trigger'],key.removeprefix('body.'),'actual body observation binding')
        elif key.startswith('event.'):same(key,'event.'+str(source['global_sequence']),'actual event observation ID');require(source['kind'] in ('GameEventPlayerLivesChanged','GameEventSpellResolved','GameEventCardDamaged','GameEventTokenCreated'),'actual finite event observation family')
        elif key.startswith('settle.'):same(source['kind'],'settled_ready','actual settlement observation');same(source['action_id'],key.removeprefix('settle.'),'actual settlement checkpoint identity')
        elif key.startswith('attack-event.'):same(source['kind'],'GameEventAttackersDeclared','actual attacker event observation')
        elif key.startswith('block-event.'):same(source['kind'],'GameEventBlockersDeclared','actual blocker event observation')
        elif key.startswith('damage.'):require(source['kind'] in ('GameEventGameOutcome','entry'),'actual damage observation cut')
        elif key.startswith('death-order.'):same(source['method'],'orderMoveToZoneList','actual graveyard order observation')
        elif key in ('prepared.first_ready','opening.complete'):same(source['method'],'chooseSpellAbilityToPlay','actual first priority observation')
        elif key=='prepared.initialized':same(source['kind'],'prepared_initialized','actual scheduled initializer cut')
        elif key=='setup.predraw':same(source['method'],'chooseStartingPlayer','actual registered setup cut')
        elif key=='bounded.stop':same(source['method'],'chooseSpellAbilityToPlay','actual bounded priority owner');same(source['reply']['kind'],'bounded_ready_exit','actual bounded exit cut')
        elif key=='terminal':same(source['kind'],'GameEventGameOutcome','actual natural outcome cut')
        elif key=='terminal.return':require(source['kind'] in ('start_game_return','prepared_loop_return'),'actual natural loop return cut')
        else:raise EvidenceError('unknown mandatory observation selector '+key)
        if row['checkpoint_ref'] is not None:require(row['checkpoint_ref'] in raw['checkpoints'],'actual observation checkpoint');worlds[key]=mandatory_world(raw['checkpoints'][row['checkpoint_ref']],raw,case)
    validate_forge_settlement_events(raw,casts)
    inventory=unique(case['inventory'],'alias','actual settlement source inventory')
    for action,status in raw['settlements'].items():
        source=casts[action]['source'];permanent='Creature' in raw['paper_cards'][inventory[source]['definition_ref']]['core_types'];same(worlds['settle.'+action]['cards'][source]['zone'],'battlefield' if permanent and status=='resolved' else 'graveyard','actual settled source zone')
    final=mandatory_world(raw['final_world'],raw,case);stop=case['stop'];same(raw['final_world']['header']['stack_size'],0,'actual settled empty stack')
    for alias,token in token_rows.items():
        departures=[row for row in raw['events'] if row['kind']=='GameEventCardChangeZone' and row['payload']['card']['alias']==alias and row['payload']['from'] and row['payload']['from']['zone_type']=='battlefield']
        if departures:same(len(departures),1,'one actual token departure');same(departures[0]['payload']['to']['zone_type'],'graveyard','finite actual token death consequence');require(alias not in final['cards'],'actual departed token absent at settled endpoint; no dedicated cessation claim')
        else:require(alias in final['cards'] and final['cards'][alias]['zone']=='battlefield','actual surviving birth remains on battlefield')
    if stop['kind']=='ready_priority':
        same(complete['kind'],'bounded','required bounded completion');same(raw['actual_outcome'],None,'bounded no outcome');same(complete['game_over'],False,'bounded nonterminal')
        for key,field in [('individual_turn','individual_turn'),('step','step'),('active','active_role'),('raw_priority','priority_holder')]:same(raw['final_world']['header'][key],stop[field],'exact bounded '+key)
    else:
        same(complete['kind'],'natural','required natural completion');same(complete['game_over'],True,'actual natural outcome');require(raw['actual_outcome'] is not None,'actual registered outcome')
        outcome=raw['actual_outcome'];same(set(outcome['players']),{'P0','P1'},'actual outcome full registered partition');winners=[]
        for role,player in outcome['players'].items():
            same(player['won'],player['game_outcome_is_winner'],'actual registered winner relation');same(player['lost'],raw['final_world']['players'][role]['lost'],'actual terminal/header loss partition');same(player['lost'],raw['final_world']['header']['players'][role]['lost'],'actual final header loss partition');same(player['won'],raw['final_world']['players'][role]['won'],'actual terminal winner relation');require(player['won']!=player['lost'],'actual finite winner/loser partition')
            if player['won']:winners.append(role)
        same(outcome['winning_seat'],winners[0] if len(winners)==1 else None,'actual outcome winner identity');same(outcome['draw'],len(winners)==0,'actual draw flag');same(len(winners),1,'finite mandatory natural outcome has one actual winner')
        same(raw['final_world']['header']['individual_turn'],stop['turn'],'natural terminal turn');same(raw['final_world']['header']['step'],stop['step'].replace('-','_'),'natural terminal step');same(raw['final_world']['header']['active'],stop['active'],'natural terminal active')
    return dict(worlds=worlds,final=final,entries=entries,returns=returns,triggers=triggers,tokens=token_rows,casts=casts,settlements=raw['settlements'])


@evidence_boundary
def mandatory_primary_facts(raw:dict[str,Any],case:dict[str,Any],projection:dict[str,Any],primary:dict[str,Any])->dict[str,Any]:
    """Check independently available primary facts without suppressing life/body/outcome for private-hook gaps."""
    name=case['case_id'];life={role:int(value) for role,value in projection['final']['life'].items()};events=raw['events'];triggers=list(projection['triggers'].values());tokens=list(projection['tokens'].values());inventory=unique(case['inventory'],'alias','physical definition');definitions=raw['paper_cards']
    def card_name(alias:str)->str:return tokens_by_alias[alias]['name'] if alias.startswith('token:') else inventory[alias]['definition_ref']
    tokens_by_alias=projection['tokens']
    zones=[row for row in events if row['kind']=='GameEventCardChangeZone'];draws=[row for row in zones if row['payload']['from'] and row['payload']['to'] and row['payload']['from']['zone_type']=='library' and row['payload']['to']['zone_type']=='hand'];deaths=[row for row in zones if row['payload']['from'] and row['payload']['to'] and row['payload']['from']['zone_type']=='battlefield' and row['payload']['to']['zone_type']=='graveyard'];life_events=[dict(player=row['payload']['player'],old=int(row['payload']['oldLives']),new=int(row['payload']['newLives'])) for row in events if row['kind']=='GameEventPlayerLivesChanged']
    trigger_count=lambda card:sum(1 for row in triggers if card_name(row['actual_ability']['source'])==card)
    results={};unavailable=[]
    for fact in primary['primary_facts']:
        expected=fact['authored_expected'];actual={}
        fact_deaths=deaths;fact_draws=draws;fact_life=life
        if name in ('MT01','MT02'):
            offset=0 if name=='MT01' else 1
            if fact['id'].endswith('gain_arithmetic'):
                cut=projection['worlds']['priority.'+str(17+offset)+'.precombat_main'];fact_life={role:int(v) for role,v in cut['life'].items()}
            for number in [9,10,11]:
                if fact['id'].endswith('combat'+str(number)):
                    turn=2*number-1+offset;cut=projection['worlds']['priority.'+str(turn)+'.postcombat_main'];fact_life={role:int(v) for role,v in cut['life'].items()};fact_deaths=[r for r in deaths if r['context']['individual_turn']==turn];fact_draws=[r for r in draws if r['context']['individual_turn']==turn]
        if expected is None:unavailable.append(dict(id=fact['id'],disposition=fact['observation']));continue
        for key,value in expected.items():
            if key in ('required_loss_causes','initial_inactive_control_stamp','token_cessations','stale_allowed','opponent_turn_stamp'):
                unavailable.append(dict(id=fact['id'],field=key,reason='native exact authority/history/private flag or dedicated cessation unavailable; settled consequences checked separately'));continue
            if key.endswith('_life'):actual[key]=life[key[:2]]
            elif key in ('GW','BG'):actual[key]=fact_life['P0' if key=='GW' else 'P1']
            elif key in ('successful_draws','draws'):actual[key]=len(fact_draws)
            elif key=='occultist_draws':actual[key]=sum(row['payload']['to']['player']=='P1' and any('forge.game.ability.effects.DrawEffect.resolve'==f['class']+'.'+f['method'] for f in row['payload']['actual_stack_frames']) for row in fact_draws)
            elif key=='P1_draws':actual[key]=sum(row['payload']['to']['player']=='P1' for row in draws)
            elif key in ('deaths','dies'):actual[key]=len(fact_deaths)
            elif key=='death_members':actual[key]=sorted(row['payload']['card']['alias'] for row in deaths)
            elif key=='artist_instances':actual[key]=trigger_count('Blood Artist')
            elif key=='dissenter_instances':actual[key]=trigger_count('Doomed Dissenter')
            elif key=='entry_instances':actual[key]=sum(row['input_selector']['event']=='entry' for row in triggers)
            elif key in ('warden','orator'):actual[key]=trigger_count('Soul Warden' if key=='warden' else 'Impassioned Orator')
            elif key in ('orator_P0','orator_P1'):actual[key]=sum(card_name(row['actual_ability']['source'])=='Impassioned Orator' and row['captured_controller']==key[-2:] for row in triggers)
            elif key in ('warden_self','orator_self'):actual[key]=sum(row['actual_ability']['source']==row['subject'] and card_name(row['subject'])==('Soul Warden' if key=='warden_self' else 'Impassioned Orator') for row in triggers)
            elif key=='visionary_entry_life_triggers':actual[key]=sum(card_name(row['subject'])=='Elvish Visionary' and card_name(row['actual_ability']['source']) in ('Soul Warden','Impassioned Orator') for row in triggers)
            elif key=='visionary_draw':actual[key]=sum(any(frame['class']=='forge.game.ability.effects.DrawEffect' for frame in row['payload']['actual_stack_frames']) for row in draws)
            elif key in ('opponent_warden','opponent_orator'):actual[key]=sum(card_name(row['actual_ability']['source'])==('Soul Warden' if key.endswith('warden') else 'Impassioned Orator') and inventory[row['subject']]['owner']!=row['captured_controller'] for row in triggers if row['subject'] in inventory)
            elif key in ('soldier_births','zombie_births'):actual[key]=sum(row['name']==('Soldier Token' if key.startswith('soldier') else 'Zombie Token') for row in tokens)
            elif key=='draw_triggers':actual[key]=trigger_count('Elvish Visionary')
            elif key=='library_cards':actual[key]=len(raw['final_world']['players']['P0']['library'])
            elif key=='physical_visionaries':actual[key]=sum(row['definition_ref']=='Elvish Visionary' for row in inventory.values())
            elif key=='casts':actual[key]=sum(row['source'] in inventory and inventory[row['source']]['definition_ref']=='Elvish Visionary' for row in raw['cast_operations'])
            elif key=='entries':actual[key]=sum(row['payload']['to'] and row['payload']['to']['zone_type']=='battlefield' and card_name(row['payload']['card']['alias'])=='Elvish Visionary' for row in zones)
            elif key=='hand_draw_order':actual[key]=[card_name(row['payload']['card']['alias']) for row in draws]
            elif key=='life_after_body':actual[key]=life['P0']
            elif key in ('outcome','terminal'):actual[key]='ready' if raw['actual_outcome'] is None else 'P0-loses' if raw['actual_outcome']['players']['P0']['lost'] else 'P0-wins'
            elif key=='life_event_order':actual[key]=life_events
            elif key=='bears_survive':actual[key]=sum(row['name']=='Grizzly Bears' and row['zone']=='battlefield' for row in raw['final_world']['cards'].values())
            elif key=='scheduled_start':actual[key]=case['initialization']['setup']['step'].replace('_','-')
            elif key=='first_ready':actual[key]=next(r['context']['step'] for r in raw['callbacks'] if r['kind']=='entry' and r['method']=='chooseSpellAbilityToPlay').replace('_','-')
            elif key=='prefix_draw':actual[key]=draws[0]['payload']['card']['alias']
            elif key=='successful_prefix_draws':actual[key]=sum(r['context']['individual_turn']==case['initialization']['setup']['individual_turn'] and r['context']['step']=='draw' for r in draws)
            elif key=='hand_membership_before_named_operations':
                cut=next(world for world in raw['checkpoints'].values() if world['header']['step']=='precombat_main' and world['header']['individual_turn']==case['initialization']['setup']['individual_turn']);actual[key]={role:sorted(cut['players'][role]['hand']) for role in ['P0','P1']}
            elif key=='chooser':actual[key]=next(r['actor'] for r in projection['entries'].values() if r['method']=='orderMoveToZoneList')
            elif key=='domain':actual[key]=next(r['arguments']['offered_sequence'] for r in projection['entries'].values() if r['method']=='orderMoveToZoneList');actual[key]=sorted(actual[key])
            elif key=='final_top_first':actual[key]=raw['final_world']['players']['P1']['graveyard']
            elif key in ('required_owner_order_prompts','owner_order_prompts'):actual[key]=sum(r['method']=='orderMoveToZoneList' for r in projection['entries'].values())
            elif key=='P0_physical_entrants':actual[key]=sum(row['payload']['card']['owner']=='P0' and row['payload']['card']['alias'] in inventory for row in deaths if row['context']['individual_turn']==5)
            elif key=='P1_token_departures':actual[key]=sum(row['payload']['card']['owner']=='P1' and row['payload']['card']['alias'] in tokens_by_alias for row in deaths if row['context']['individual_turn']==5)
            elif key in ('prelude_artist_instances','final_artist_instances'):actual[key]=sum(card_name(row['actual_ability']['source'])=='Blood Artist' and ('prelude' in row['input_selector']['cause_operation'])==(key=='prelude_artist_instances') for row in triggers)
            elif key=='final_death_members':actual[key]=['physical:Bears 1' if row['payload']['card']['alias'] in inventory else 'token:actual-Dissenter-Zombie0' for row in deaths if row['context']['individual_turn']==5]
            elif key=='printed_ability':actual[key]='tap:G' if any(r['kind']=='ordinary_entry' and r['policy']['kind']=='activate_mana' and r['policy']['alias'].endswith('llanowar-visionary.1') and r['policy']['mana_color']=='G' for r in raw['operations']) else None
            elif key in ('new_entry_allowed','second_tap_allowed','fresh_allowed'):
                mutation='tapped' if key=='second_tap_allowed' else 'young';probes=[r for r in raw['operations'] if r['kind']=='mana_legality_probe' and r['policy']['mutation']==mutation];require(bool(probes),'actual printed mana legality probe');actual[key]=probes[0]['can_play']
            elif key=='next_own_turn_G':actual[key]=sum(r['kind']=='ordinary_entry' and r['policy']['kind']=='activate_mana' and r['policy']['alias'].endswith('llanowar-visionary.1') for r in raw['operations'])
            elif key=='old_equals_new_incarnation':
                source='P0.llanowar-visionary.1';actual[key]=raw['checkpoints'][next(iter(raw['checkpoints']))]['cards'][source]['timestamp']==raw['final_world']['cards'][source]['timestamp']
            elif key in ('winner','losers','cause','individual_turn'):
                if key=='winner':actual[key]='GW' if raw['actual_outcome']['winning_seat']=='P0' else 'BG'
                elif key=='losers':actual[key]=['GW' if role=='P0' else 'BG' for role,p in raw['actual_outcome']['players'].items() if p['lost']]
                elif key=='cause':actual[key]='life-zero' if raw['actual_outcome']['players']['P1']['loss_reason']=='LifeReachedZero' else raw['actual_outcome']['players']['P1']['loss_reason']
                else:actual[key]=raw['final_world']['header']['individual_turn']
            else:raise EvidenceError('Unimplemented required common primary field: '+name+'/'+fact['id']+'/'+key)
            compare=value
            if key in ('death_members','domain','hand_membership_before_named_operations','final_death_members'):
                if isinstance(value,list):compare=sorted(value);actual[key]=sorted(actual[key])
                else:compare={role:sorted(rows) for role,rows in value.items()}
            same(actual[key],compare,'primary/Forge '+name+'/'+fact['id']+'/'+key)
        results[fact['id']]=actual
    return dict(actual_common_facts=results,unavailable_fields=unavailable,successful_draws=[r['payload']['card']['alias'] for r in draws],deaths=[r['payload']['card']['alias'] for r in deaths],life_event_order=life_events)


@evidence_boundary
def compare_mandatory_reference(run_dir:Path,primary_index:Path,compiled_input:Path,requirements:Path,selected:list[str]|None=None)->dict[str,Any]:
    """Require the frozen finite authorities, actual producer closure and independent primary checks."""
    import tempfile
    binding=read(run_dir/'authority-binding.json');root=Path(binding['source_root']);authority=mandatory_authorities(root,primary_index,compiled_input,requirements);commands=validate_reference_run(run_dir);inputs=read(run_dir/'inputs.json')
    validate_mandatory_process(run_dir.resolve(),commands,read(run_dir/'results.json'),read(run_dir/'source-stability.json'),binding)
    for name,digest in MANDATORY_PINS.items():same(sha(run_dir/(name+'.json')),digest,'retained mandatory authority '+name)
    stability=read(run_dir/'source-stability.json');same(stability['authority_unchanged'],True,'actual authority guards stable');same(stability['authority_before'],stability['authority_after'],'actual authority pre/post equality')
    for path,digest in stability['authority_after'].items():same(sha(Path(path)),digest,'current authority guard bytes')
    same(classpath_receipts([row['path'] for row in commands['classpath']]),commands['classpath'],'complete actual current classpath byte closure')
    with tempfile.TemporaryDirectory(prefix='spellwake-mandatory-reader-') as directory:
        expected=make_mandatory_inputs(authority,root/'work/reference-research/forge/forge-gui/res',Path(directory));expected['authority']=binding['supplied'];same(inputs,expected,'actual strict input-only lowering')
    cases=unique(inputs['cases'],'case_id','mandatory recipe');names=selected or list(cases);require(len(names)==len(set(names)) and set(names)<=set(cases),'exact known selected cases');same(commands['selected'],names,'actual selected process case inventory');same(set(p.stem for p in (run_dir/'evidence').glob('*.json')),set(names),'exact actual output case inventory')
    primary_cases={row['id']:row for row in authority['payloads']['full-games.json']['cases']+authority['payloads']['reduced-cases.json']['variants']};reports={}
    for name in names:
        raw=read(run_dir/'evidence'/f'{name}.json');same(raw['inputs_sha256'],sha(run_dir/'inputs.json'),'actual input digest');validate_runtime(raw,run_dir,commands)
        required={'forge.game.trigger.TriggerHandler','forge.game.trigger.TriggerChangesZone','forge.game.trigger.WrappedAbility','forge.game.ability.effects.TokenEffect','forge.game.ability.effects.TokenEffectBase','forge.game.ability.effects.LifeGainEffect','forge.game.ability.effects.LifeLoseEffect','forge.game.card.token.TokenInfo','forge.token.TokenDb','forge.item.PaperToken'};require(required<={row['class'] for row in raw['runtime']['loader_classes']},'actual mandatory runtime owner closure')
        projected=mandatory_forge_projection(raw,cases[name],inputs);facts=mandatory_primary_facts(raw,cases[name],projected,primary_cases[name]);reports[name]=dict(raw_sha256=sha(run_dir/'evidence'/f'{name}.json'),completion=raw['completion']['kind'],actions=len(cases[name]['actions']),triggers=len(projected['triggers']),tokens=len(projected['tokens']),facts=facts)
    return dict(status='reference_vs_frozen_primary_checked_native_join_pending',profile=PROFILES[1],cases=reports,case_count=len(reports),native_numeric_agreement_claim=False,reader_sha256=sha(Path(__file__)),unavailable=MANDATORY_UNAVAILABLE)


NATIVE_MANDATORY_ROLES={41:'P0',3:'P1'}
PREDECESSOR_BASELINE_SHA='cef2e3a24a14b851ec5d1e259ebc6df5a19579bcf07899fbec55fe8f55aa510c'
PREDECESSOR_API_PINS={'historical-all':'baa07079e96a7efabd58c81877f767b955521b3b349f4b1a53fea3e76c7507c9','teaching-all':'561d8ad9a6a5cd44b84d89197a09a5da9c7312e7ddd8ee0280eb053bd244ce16','interaction-all':'22bd625f8a3a40cc590fa27d913db96d9ad9a2f789186646ab157701d5cfc889'}
MANDATORY_REPLAY_FIELDS=['actual_candidate_base','retained_actual_input','actual_successor_basis','header','twelve_predecessor_current_fact_families','complete_observed_card_changes','changed_ordered_pile_after_vectors','canonical_execution_stack_resolution_waiting_and_pending_placement','complete_token_observations_and_endpoint_deltas','canonical_rules_journal','complete_native_rules_journals','full_final_native_state']


@evidence_boundary
def token_delta_after(before:dict[str,Any],changes:list[dict[str,Any]])->dict[str,Any]:
    """Apply only the checked endpoint observation grammar, shared by ordinary and test-only readers."""
    require(isinstance(changes,list),'required token endpoint delta array');after=dict(before);seen=set()
    for delta in changes:
        kind=delta['kind'];identity=delta['object'];require(identity not in seen,'one token endpoint delta per actual identity');seen.add(identity);require(kind in ('added','updated','removed'),'closed token endpoint delta tag');same(set(delta),{'kind','object','after'} if kind=='added' else {'kind','object','before','after'} if kind=='updated' else {'kind','object','before'},'exact tagged token delta shape')
        if kind in ('updated','removed'):require(identity in after,'actual token preimage exists');same(delta['before'],after[identity],'actual complete token delta preimage')
        else:require(identity not in after,'actual added token is fresh endpoint')
        if kind=='removed':del after[identity]
        else:same(delta['after']['object']['object'],identity,'actual token delta same-identity endpoint');after[identity]=delta['after']
    return after


@evidence_boundary
def validate_token_observer(document:dict[str,Any])->dict[str,Any]:
    """Validate actual OB01 endpoints at their disclosed synthetic test-only depth."""
    same(document['schema_version'],1,'OB01 schema1');same(document['classification'],'native-test-only-synthetic-observer-mechanism','OB01 explicit depth')
    for field in ['equal_complete_rules_journals','equal_final_native_state','equal_saved_input_reexecution','baseline_restored']:same(document[field],True,'actual OB01 '+field)
    for field in ['ordinary_report','registry_witness','forge_comparison']:same(document[field],False,'OB01 does not claim '+field)
    same(document['saved_input_count'],15,'actual OB01 saved input count');same(document['complete_rules_journal_count'],16,'actual OB01 complete journal count');observations=document['observations'];same([row['ordinal'] for row in observations],list(range(16)),'actual OB01 ordinal/checkpoint inventory');previous=None
    for row in observations:
        same(row['before'],previous,'actual OB01 complete retained endpoint ancestry');same(row['checkpoint'],row['after'],'actual OB01 full checkpoint vs endpoint');same(token_delta_after({} if previous is None else previous['tokens'],row['token_changes']),row['after']['tokens'],'actual OB01 complete endpoint delta inventory');require(isinstance(row['rules_journal'],list),'actual OB01 complete journal array');same(row['input'] is None,row['ordinal']==0,'actual OB01 retained saved inputs')
        for identity,token in row['after']['tokens'].items():
            same(set(token),{'object','program','characteristics','creature','counters'},'complete OB01 token observation');same(identity,token['object']['object'],'actual OB01 same object identity');same(token['object']['physical_card'],None,'OB01 token has no CardId')
            if token['object']['zone']['kind']=='battlefield':
                same(set(token['creature']),{'power','toughness','marked_damage'},'OB01 required stored marks independently of derived PT');same(set(token['counters']),{'plus_one','minus_one'},'OB01 required complete stored counters')
                for value in [*token['creature'].values(),*token['counters'].values()]:integer(value,'actual OB01 RulesInteger')
            else:same(token['creature'],None,'OB01 departed creature observation');same(token['counters'],None,'OB01 departed counter observation')
        previous=row['after']
    checkpoints=document['checkpoints'];same(checkpoints,dict(born=3,counters=6,damage=9,cleanup=11,removed=15),'actual OB01 discriminating checkpoint cuts');identity=None;facts={}
    for name,power,marks,counters in [('born','2','0','0'),('counters','4','0','2'),('damage','4','1','2'),('cleanup','4','0','2')]:
        tokens=observations[checkpoints[name]]['after']['tokens'];same(len(tokens),1,'actual OB01 one current token');current=next(iter(tokens));identity=current if identity is None else identity;same(current,identity,'actual OB01 update retains native identity');token=tokens[current];same(token['creature'],dict(power=power,toughness=power,marked_damage=marks),'OB01 actual derived PT and stored marks '+name);same(token['counters'],dict(plus_one=counters,minus_one='0'),'OB01 actual stored counter inventory '+name);facts[name]=dict(object=current,creature=token['creature'],counters=token['counters'])
    same(observations[checkpoints['removed']]['after']['tokens'],{},'actual OB01 removed token endpoint');controls=document['effective_corruption_controls'];same(len(controls),17,'actual OB01 corruption denominator');same(len(set(controls)),17,'actual OB01 distinct effective controls');required={'missing-token-creature','missing-token-counters','missing-token-marks','changed-token-marks','missing-plus-counters','changed-plus-counters','missing-minus-counters','changed-minus-counters','missing-delta-kind','missing-delta-object','missing-delta-before','missing-delta-after','lost-token-update','changed-delta-marks','changed-delta-counters','missing-token-delta-array','missing-empty-token-delta-array'};same(set(controls),required,'actual OB01 exact effective corruption classes')
    return dict(classification=document['classification'],saved_inputs=15,checkpoints=facts,ordinary_report_credit=False,registry_credit=False,forge_credit=False,effective_controls=controls)


def mandatory_role(value:Any)->str:
    require(type(value) is int and value in NATIVE_MANDATORY_ROLES,'actual mandatory numerical seat');return NATIVE_MANDATORY_ROLES[value]


def canonical_digest(value:Any)->str:
    """Hash exact typed JSON semantics without retaining another packet or changing input bytes."""
    return hashlib.sha256(json.dumps(value,sort_keys=True,separators=(',',':'),ensure_ascii=False,allow_nan=False).encode('utf-8')).hexdigest()


_NATIVE_MANA={'white':'White','blue':'Blue','black':'Black','red':'Red','green':'Green','colorless':'Colourless'}


class MandatoryWire:
    CONTEXT_PREFIX="mandatory-context-v1-"
    JOURNAL_FIELD="rules_journal"

    def primary_context_reference(self)->str:return self.CONTEXT_PREFIX+self.run['fixture_ref']
    """One ordinary/canonical observation normalizer; it never runs or predicts game rules."""
    def __init__(self,run:dict[str,Any],history:dict[str,str|None])->None:
        self.run=run;self.history=history;self.aliases={row['alias']:str(row['card_id']) for row in run['static_inputs']['aliases']};self.by_card={card:alias for alias,card in self.aliases.items()};self.contexts=unique(run['static_inputs']['contexts'],'reference','actual declared context')

    def basis(self,value:dict[str,Any])->dict[str,Any]:
        same(set(value),{'game','context_ref','epoch','revision'},'exact ordinary basis fields');context=self.contexts[value['context_ref']];return dict(game=value['game'],context={key:member for key,member in context.items() if key!='reference'},epoch=value['epoch'],revision=value['revision'])

    def reference(self,value:dict[str,Any],weak:bool=False)->str:
        same(set(value),{'object','alias'},'exact ordinary object reference');identity=value['object'];require(isinstance(identity,str) and re.fullmatch(r'ObjectId\([0-9]+\)',identity) is not None,'canonical object reference identity')
        if identity in self.history:
            expected=self.history[identity];same(value['alias'],None if expected is None or expected.startswith('token:') else expected,'actual reference historical alias')
        else:
            require(weak,'actual issued object reference has known history');require(value['alias'] is None or value['alias'] in self.aliases,'actual weak reference declared physical alias')
        return identity

    def views(self,value:Any,checks:bool=False,weak:bool=False)->Any:
        """Normalize only explicit reference/appearance alias fields, copying changed ancestors."""
        if isinstance(value,dict):
            if set(value)=={'object','alias'}:
                identity=self.reference(value,weak)
                return dict(object=identity,alias=None if value['alias'] is None else self.aliases[value['alias']]) if checks else identity
            updates={}
            for key,member in value.items():
                if key=='alias' and 'physical_card' in value and 'object' in value:
                    expected=None if value['physical_card'] is None else self.by_card[str(value['physical_card'])];same(member,expected,'actual complete appearance physical alias')
                    if checks:updates[key]=None if value['physical_card'] is None else str(value['physical_card'])
                    continue
                mapped=self.views(member,checks,weak)
                if mapped is not member:updates[key]=mapped
            if 'alias' in value and 'physical_card' in value and 'object' in value:
                result={key:member for key,member in value.items() if checks or key!='alias'};result.update(updates);return result
            return dict(value,**updates) if updates else value
        if isinstance(value,list):
            mapped=[self.views(member,checks,weak) for member in value];return mapped if any(a is not b for a,b in zip(mapped,value,strict=True)) else value
        return value

    def target(self,value:dict[str,Any],weak:bool=False)->dict[str,Any]:
        kind=value['kind']
        if kind=='player':same(set(value),{'kind','player'},'exact player target');return dict(tag='Player',player=value['player'])
        if kind=='stack':same(set(value),{'kind','entry'},'exact stack target');return dict(tag='Stack',entry=value['entry'])
        same(kind,'card','finite ordinary object target');same(set(value),{'kind','object'},'exact object target');return dict(tag='Object',object=self.reference(value['object'],weak))

    def spend(self,value:dict[str,Any])->list[dict[str,Any]]:
        same(set(value),{'spend'},'exact ordinary spend');return [dict(type=_NATIVE_MANA[row['mana_type']],amount=row['amount']) for row in value['spend']]

    def reply(self,value:dict[str,Any],weak:bool)->dict[str,Any]:
        kind=value['kind']
        if kind in ('discard','bottom','order_graveyard'):same(set(value),{'kind','ordered'},'exact object-order reply');return dict(tag={'discard':'Discard','bottom':'Bottom','order_graveyard':'OrderGraveyard'}[kind],items=[self.reference(ref,weak) for ref in value['ordered']])
        if kind=='order_triggers':same(set(value),{'kind','ordered'},'exact trigger-order reply');return dict(tag='OrderTriggers',items=value['ordered'])
        if kind=='timestamp_order':same(set(value),{'kind','ordered'},'exact timestamp reply');return dict(tag='TimestampOrder',order=value['ordered'])
        if kind=='trigger_target':same(set(value),{'kind','target'},'exact trigger-target reply');return dict(tag='TriggerTarget',target=self.target(value['target'],weak))
        if kind=='mulligan':same(set(value),{'kind','choice'},'exact opening reply');return dict(tag='Mulligan',choice={'keep':'Keep','mulligan':'Mulligan'}[value['choice']])
        if kind in ('declare_attackers','declare_blockers','assign_combat_damage'):
            field={'declare_attackers':'attackers','declare_blockers':'blockers','assign_combat_damage':'assignments'}[kind];same(set(value),{'kind',field},'exact combat reply');return dict(tag={'declare_attackers':'DeclareAttackers','declare_blockers':'DeclareBlockers','assign_combat_damage':'AssignCombatDamage'}[kind],items=self.views(value[field],weak=weak))
        raise EvidenceError('source19 forbids ordinary reply kind '+str(kind))

    def input(self,value:dict[str,Any],weak:bool=False)->dict[str,Any]:
        same(set(value),{'basis','actor','command'},'exact actual ordinary input');command=value['command'];kind=command['kind']
        if kind=='pass':same(set(command),{'kind'},'exact Pass command');native=dict(tag='Pass')
        elif kind=='play_land':same(set(command),{'kind','land'},'exact PlayLand command');native=dict(tag='PlayLand',land=self.reference(command['land'],weak))
        elif kind=='activate_mana':same(set(command),{'kind','source','ability'},'exact ActivateMana command');native=dict(tag='ActivateMana',source=self.reference(command['source'],weak),ability=command['ability'])
        elif kind=='cast_untargeted':same(set(command),{'kind','spell','payment'},'exact CastUntargeted command');native=dict(tag='CastUntargeted',spell=self.reference(command['spell'],weak),payment=None if command['payment'] is None else self.spend(command['payment']))
        elif kind=='cast_paid':same(set(command),{'kind','spell','target','payment'},'exact CastPaid command');native=dict(tag='CastPaid',spell=self.reference(command['spell'],weak),target=self.target(command['target'],weak),payment=self.spend(command['payment']))
        elif kind=='reply':same(set(command),{'kind','decision','choice'},'exact Reply command');native=dict(tag='Reply',decision=command['decision'],choice=self.reply(command['choice'],weak))
        else:raise EvidenceError('source19 forbids ordinary command '+str(kind))
        return dict(actor=value['actor'],basis=self.basis(value['basis']),command=native)

    def decision(self,value:dict[str,Any]|None)->dict[str,Any]|None:
        if value is None:return None
        same(set(value),{'id','basis','chooser','frame'},'exact ordinary pending decision');encoded=value['frame'];kind=encoded['kind'];frame=encoded['frame'];same(set(encoded),{'kind','frame'},'exact ordinary frame envelope')
        if kind in ('triggers','trigger_target'):native=dict(tag={'triggers':'Triggers','trigger_target':'TriggerTarget'}[kind],frame=frame)
        elif kind=='timestamp_order':same(frame['tag'],'TimestampOrder','actual canonical timestamp frame');native=frame
        elif kind=='checks':native=dict(tag='Checks',frame=self.views(frame,checks=True))
        elif kind=='opening':
            if frame['kind']=='declare':native=dict(tag='Mulligan',chooser=frame['player'],taken=frame['taken'],can_mulligan=frame['can_mulligan'])
            else:same(frame['kind'],'bottom','finite opening frame');native=dict(tag='Bottom',chooser=frame['player'],count=frame['count'],hand=[self.reference(row) for row in frame['exact_hand']])
        elif kind=='combat':
            family=frame['kind'];native=dict(revision=frame['revision'],chooser=frame['chooser'])
            if family=='attackers':native.update(tag='DeclareAttackers',attackers=[self.reference(row) for row in frame['eligible_attackers']],defenders=frame['defenders'])
            elif family=='blockers':native.update(tag='DeclareBlockers',blockers=[self.reference(row) for row in frame['eligible_blockers']],attackers=[self.reference(row) for row in frame['attackers']])
            else:same(family,'damage_assignments','finite combat frame');native.update(tag='AssignCombatDamage',position=frame['position'],domains=self.views(frame['domains']))
        else:raise EvidenceError('source19 forbids pending frame '+str(kind))
        return dict(id=value['id'],basis=self.basis(value['basis']),chooser=value['chooser'],frame=native)

    @staticmethod
    def phase(value:dict[str,Any])->str:
        kind=value['kind']
        if kind=='priority':
            phase=value['phase'];tag=phase['kind']
            if tag=='ready':return 'Priority(Ready { holder: SeatId('+str(phase['holder'])+') })'
            if tag=='requires_checks':return 'Priority(RequiresChecks { candidate: SeatId('+str(phase['candidate'])+') })'
            same(tag,'all_passed','finite priority phase');return 'Priority(AllPassed)'
        if kind in ('resolving','checking_targets','all_targets_illegal'):return {'resolving':'Resolving','checking_targets':'CheckingTargets','all_targets_illegal':'AllTargetsIllegal'}[kind]+'('+value['entry']+')'
        return {'opening':'Opening','turn_work':'TurnWork','step_ended':'StepEnded'}[kind]

    @staticmethod
    def turn(value:dict[str,Any]|None,work:dict[str,Any]|None)->dict[str,Any]|None:
        if value is None:same(work,None,'actual absent turn has no work');return None
        stage=value['stage']
        if stage['kind']=='priority':same(work,None,'actual priority stage has no turn work');text='Priority'
        else:
            same(stage['kind'],'work','finite turn stage');same(stage['work'],work,'actual Header turn work derives from stage');item=work['kind'];text='Work('+({'untap':'Untap','draw':'Draw','declare_attackers':'DeclareAttackers','declare_blockers':'DeclareBlockers','cleanup':'Cleanup'}[item] if item!='combat_damage' else 'CombatDamage { additional: '+str(work['additional']).lower()+' }')+')'
        return dict(position=value['position'],lands_played=value['lands_played'],stage=text)

    def combat(self,value:dict[str,Any]|None)->dict[str,Any]|None:
        if value is None:return None
        attackers=[];blockers=[]
        for row in value['attackers']:
            combatant=row['combatant'];attackers.append(dict(object=self.reference(combatant['object']),controller=combatant['controller'],control_epoch=combatant['control_epoch'],defender=row['defender'],block_status={'not_declared':'NotDeclared','blocked':'Blocked','unblocked':'Unblocked'}[row['blocked']]))
        for row in value['blockers']:
            combatant=row['combatant'];blockers.append(dict(object=self.reference(combatant['object']),controller=combatant['controller'],control_epoch=combatant['control_epoch'],attacker=None if row['attacker'] is None else self.reference(row['attacker'])))
        return dict(had_attackers=value['had_attackers'],attackers=attackers,blockers=blockers)

    def facts(self,value:dict[str,Any])->dict[str,Any]:
        """Normalize all thirteen independently emitted candidate fact families to canonical owners."""
        origins={'player_reply':'PlayerReply','forced':'Forced','forced_empty':'ForcedEmpty'};result=dict(checks=self.views(value['check_journal'],checks=True),damage=value['damage_journal'],turn_transitions=value['turn_transitions'],ability_activation=self.views(value['ability_activation']),land_play=self.views(value['land_play']),permanent_entry=self.views(value['permanent_entry']))
        if self.JOURNAL_FIELD is not None:result['append']=value[self.JOURNAL_FIELD]
        result['shuffles']=[dict(player=row['player'],algorithm=row['algorithm'],before=[self.reference(ref) for ref in row['before_exact_order']],after=[self.reference(ref) for ref in row['after_exact_order']],entropy_start=row['entropy_start'],entropy_end=row['entropy_end']) for row in value['shuffle_journal']]
        result['draws']=[dict(tag={'drawn':'Drawn','empty_library':'EmptyLibrary'}[row['kind']],**self.views({key:member for key,member in row.items() if key!='kind'})) for row in value['draw_journal']]
        result['declarations']=[dict(tag={'attackers':'Attackers','blockers':'Blockers'}[row['kind']],**self.views({key:(origins[member] if key=='origin' else member) for key,member in row.items() if key!='kind'})) for row in value['combat_declarations']]
        result['assignments']=[self.views(dict(row,origin=origins[row['origin']])) for row in value['combat_assignments']]
        payment=value['mana_payment'];result['mana_payment']=None if payment is None else dict(entry=payment['source'],payer=payment['payer'],cost=dict(generic=payment['cost']['generic'],required={_NATIVE_MANA[key]:member for key,member in payment['cost']['required'].items()}),spend=self.spend(dict(spend=payment['spend'])))
        production=value['mana_production'];result['mana_production']=None if production is None else dict(source=self.reference(production['source']),ability=production['ability'],controller=production['controller'],produced={_NATIVE_MANA[key]:member for key,member in production['produced'].items()})
        return result


@evidence_boundary
def mandatory_replay(run:dict[str,Any])->dict[str,Any]:
    """The one owner of exact serialized accepted/checkpoint/probe replay inventories."""
    accepted=run['accepted'];same([row['ordinal'] for row in accepted],list(range(1,len(accepted)+1)),'actual native accepted ordinals');rerun=run['rerun'];same(set(rerun),{'all_inputs_consumed','checkpoint_ordinals_compared','compared_ordinals','comparison_fields','final_state_equal','initialization_equal','probe_ids_compared'},'exact complete mandatory rerun fields')
    for field in ['initialization_equal','final_state_equal','all_inputs_consumed']:same(rerun[field],True,'actual saved-input rerun '+field)
    same(rerun['compared_ordinals'],list(range(1,len(accepted)+1)),'every accepted input reexecuted once');same(rerun['checkpoint_ordinals_compared'],list(range(len(accepted)+1)),'every complete checkpoint reexecuted exactly once');same(rerun['comparison_fields'],MANDATORY_REPLAY_FIELDS,'actual exact complete Report3 comparison fields');probes=unique(run['rejection_probes'],'probe_id','native rejected probe');same(rerun['probe_ids_compared'],[row['probe_id'] for row in run['rejection_probes']],'every actual rejected probe reexecuted exactly once')
    for probe in probes.values():same(probe['no_candidate'],True,'native rejected transition has no adopted candidate');same(probe['state_retained'],True,'native rejection preserves whole state');same(probe['actual_error'],probe['expected_error'],'actual typed error')
    return probes


@evidence_boundary
def mandatory_endpoints(run:dict[str,Any],*,token_changes_required:bool=True,complete_checkpoints:bool=True)->Any:
    """The single owner of complete Report3 endpoint ancestry, typed deltas and per-ordinal checkpoints."""
    accepted=run['accepted'];count=len(accepted);require(set(run['checkpoints'])<={str(i) for i in range(count+1)},'actual checkpoint ordinals are candidate ordinals');
    if complete_checkpoints:same(set(run['checkpoints']),{str(i) for i in range(count+1)},'actual complete per-ordinal checkpoint inventory')
    current=dict(run['checkpoints']['0']['cards']);tokens=dict(run['checkpoints']['0']['tokens']);piles=list(run['checkpoints']['0']['ordered_piles'])
    for ordinal,row in enumerate([run['initialization'],*accepted]):
        same(row['ordinal'],ordinal,'actual endpoint ordinal');
        if ordinal==0:same(row['checkpoint_ordinal'],0,'actual initializer checkpoint ordinal zero')
        same(set(row),{'after_basis','base','checkpoint_ordinal','current_facts','header','ordinal'} if ordinal==0 else {'after_basis','base','card_changes','current_facts','header','input','ordinal','pile_changes','policy_entry_ref'}|({'token_changes'} if token_changes_required else set()),'exact selected Report initializer/accepted fields')
        seen=set()
        for change in row.get('card_changes',[]):
            same(set(change),{'card','before','after'},'exact physical endpoint delta');card=change['card'];require(card not in seen,'one physical endpoint delta per card');seen.add(card);same(current[card],change['before'],'actual card delta preimage');current[card]=change['after']
        if ordinal and token_changes_required:tokens=token_delta_after(tokens,row['token_changes'])
        changed=set()
        for change in row.get('pile_changes',[]):
            same(set(change),{'pile','objects_top_to_bottom'},'exact ordered pile after-vector');key=(change['pile']['owner'],change['pile']['kind']);require(key not in changed,'one ordered pile endpoint per pile');changed.add(key);found=[i for i,pile in enumerate(piles) if pile['pile']==change['pile']];same(len(found),1,'known ordered pile delta');piles[found[0]]=change
        world=dict(cards=current,tokens=tokens,ordered_piles=piles)
        if str(ordinal) in run['checkpoints']:same(run['checkpoints'][str(ordinal)],dict(world,ordinal=ordinal,header_ref=ordinal),'actual complete checkpoint equals endpoint extraction')
        seats=row['header']['participants'];zones=['library','hand','graveyard','battlefield','stack','exile','command'];counts={str(seat):{zone:0 for zone in zones} for seat in seats};controllers={str(seat):0 for seat in seats}
        for value in [*current.values(),*tokens.values()]:
            obj=value['object'];require(obj['owner'] in seats and obj['zone']['kind'] in zones,'actual World registered owner and zone');counts[str(obj['owner'])][obj['zone']['kind']]+=1
            if obj['zone']['kind']=='battlefield':require(obj['controller'] in seats,'actual battlefield registered controller');controllers[str(obj['controller'])]+=1
        same(row['header']['counts_by_owner'],counts,'actual complete Header owner counts derive from World objects');same(row['header']['battlefield_counts_by_controller'],controllers,'actual complete Header controller counts derive from World objects')
        for card,value in current.items():
            same(set(value),{'object','alias','creature','counters'},'complete physical observation fields');same(value['object']['physical_card'],int(card),'actual physical observation identity')
        for identity,value in tokens.items():same(set(value),{'object','program','characteristics','creature','counters'},'complete token observation fields');same(value['object']['object'],identity,'actual token observation identity')
        yield row,world


def mandatory_world_digest(world:dict[str,Any])->str:
    """Compare every actual World field while normalizing only redundant physical aliases."""
    cards={key:{field:value for field,value in row.items() if field!='alias'} for key,row in world['cards'].items()};piles=[dict(pile=row['pile'],objects_top_to_bottom=[dict(object=value['object']) for value in row['objects_top_to_bottom']]) for row in world['ordered_piles']];return canonical_digest(dict(cards=cards,tokens=world['tokens'],ordered_piles=piles))


_NATIVE_STATE_FIELDS={'active','basis','characteristic_timestamps','combat','continuous_effects','copies','creature_vocabulary','damage_multipliers','decision','effect_revision','eliminations','entropy','evaluation_trace','instruction_cursor','life','mana','name_rules','name_rules_id','objects','objects_revision','opening','outcome','participants','pending_damage','pending_trigger_placement','piles','prevention_shields','programs','resolution_state','resources','stack','state_based_losses','turn','waiting','window_phase','window_revision','world_tenures'}
_OBJECT_FIELDS={'continuous_control_since_turn_start','control_epoch','controller','default_controller','object','origin','owner','physical_card','tapped','zone'}
_CHARACTERISTIC_FIELDS={'abilities','controller','current_colors','current_creature_subtypes','current_land_subtypes','current_names','current_supertypes','current_types','defined','intrinsic_mana','keywords','mana_abilities','object','power_toughness','printed_text','zone'}
_STATE_SIGNATURE_FIELDS={'active','basis','combat','creature_vocabulary','decision','effect_revision','eliminations','entropy','instruction_cursor','life','mana','name_rules_id','objects_revision','opening','outcome','participants','pending_trigger_placement','resolution_state','stack','state_based_losses','turn','waiting','window_phase','window_revision'}


def native_world_digest(state:dict[str,Any])->str:
    """The shared complete canonical object/program/pile observer for native owners."""
    cards={};tokens={};objects={};programs=unique(state['programs'],'card_id','canonical physical program')
    for row in state['programs']:same(set(row),{'card_id','definition'},'exact canonical source Program row');require(type(row['card_id']) is int and row['card_id']>0,'canonical physical Program CardId')
    for row in state['objects']:
        same(set(row),{'alias','battlefield_counters','battlefield_marked_damage','characteristics','object','program'},'exact canonical current object row');obj=row['object'];same(set(obj),_OBJECT_FIELDS,'exact canonical object fields');identity=obj['object'];require(identity not in objects,'unique canonical current object identity');objects[identity]=obj;characteristics=row['characteristics'];same(set(characteristics),_CHARACTERISTIC_FIELDS,'exact complete canonical characteristics fields');same(characteristics['object'],identity,'canonical characteristics object');same(characteristics['controller'],obj['controller'],'canonical characteristics controller')
        bf=obj['zone']['kind']=='battlefield';creature=bf and 'Creature' in characteristics['current_types'];pt=characteristics['power_toughness'];observed=None
        if creature:
            require(pt is not None and row['battlefield_marked_damage'] is not None,'actual canonical creature PT and stored marks present');observed=dict(power=pt['derived']['power'],toughness=pt['derived']['toughness'],marked_damage=row['battlefield_marked_damage'])
            for number in observed.values():integer(number,'actual canonical creature integer')
        counters=row['battlefield_counters'];same(counters is not None,bf,'actual canonical stored counters presence')
        if counters is not None:
            same(set(counters),{'plus_one','minus_one'},'actual canonical full stored counters')
            for number in counters.values():require(integer(number,'actual stored counters')>=0,'actual nonnegative stored counters')
        physical=obj['physical_card']
        if physical is not None:
            require(type(physical) is int and physical>0,'canonical actual physical CardId');same(row['program'],programs[physical]['definition'],'canonical object full source Program');key=str(physical);require(key not in cards,'one current object per physical card');cards[key]=dict(object=obj,creature=observed,counters=counters)
        else:
            same(obj['origin']['kind'],'token','canonical token origin');same(row['program'],obj['origin']['definition'],'canonical token full materialized Program');tokens[identity]=dict(object=obj,program=row['program'],characteristics=characteristics,creature=observed,counters=counters)
    same({int(key) for key in cards},set(programs),'canonical physical program/object inventory');piles=[]
    for row in state['piles']:
        same(set(row),{'player','library','graveyard'},'canonical ordered pile fields')
        for kind in ['library','graveyard']:
            values=[]
            for identity in row[kind]:
                require(identity in objects,'canonical pile object exists');same(objects[identity]['zone'],dict(kind=kind,player=row['player']),'canonical ordered pile zone/owner');values.append(dict(object=identity))
            piles.append(dict(pile=dict(owner=row['player'],kind=kind),objects_top_to_bottom=values))
    return canonical_digest(dict(cards=cards,tokens=tokens,ordered_piles=piles))


def native_state_signature(state:dict[str,Any])->dict[str,Any]:
    same(set(state),_NATIVE_STATE_FIELDS,'exact complete canonical GameState projection fields')
    signature={key:state[key] for key in _STATE_SIGNATURE_FIELDS};signature['world_digest']=native_world_digest(state);return signature


def ordinary_common_state_signature(wire:MandatoryWire,header:dict[str,Any],world_digest:str)->dict[str,Any]:
    """Normalize public Header fields to their existing canonical projection owners, preserving every value."""
    participants=header['participants'];context=wire.basis(header['basis'])['context'];opening=header['opening'];outcome=header['outcome'];same(header['combat_rules'],wire.run['static_inputs']['setup']['combat_rules'],'actual constant seeded combat rules');phase=header['phase'];expected_holder=phase['phase']['holder'] if phase['kind']=='priority' and phase['phase']['kind']=='ready' else None;same(header['priority_holder'],expected_holder,'actual Header priority holder derives from phase')
    if outcome is None:outcome_text=None
    elif outcome['kind']=='winner':same(set(outcome),{'kind','seat'},'exact actual winner');outcome_text='Winner(SeatId('+str(outcome['seat'])+'))'
    else:same(outcome,dict(kind='draw'),'finite actual outcome');outcome_text='Draw'
    return dict(active=header['active'],basis=wire.basis(header['basis']),combat=wire.combat(header['combat']),creature_vocabulary=context['creature_vocabulary'],decision=wire.decision(header['pending']),effect_revision=header['executor_revision'],eliminations=header['pending_eliminations'],entropy=header['entropy'],instruction_cursor=header['instruction_index'],life=[dict(player=player,total=header['life'][str(player)]) for player in participants],mana=[dict(player=player,pool={_NATIVE_MANA[key]:member for key,member in header['mana'][str(player)].items()}) for player in participants],name_rules_id=context['name_rules_id'],objects_revision=header['registry_revision'],opening=None if opening is None else dict(complete=opening['complete'],players=opening['seats']),outcome=outcome_text,participants=participants,state_based_losses=header['losses'],turn=wire.turn(header['turn'],header['pending_turn_work']),window_phase=wire.phase(header['phase']),window_revision=header['window_revision'],world_digest=world_digest)



def ordinary_state_signature(wire:MandatoryWire,header:dict[str,Any],world_digest:str)->dict[str,Any]:
    execution=header['execution']
    return dict(ordinary_common_state_signature(wire,header,world_digest),pending_trigger_placement=execution['pending_trigger_placement'],resolution_state=execution['resolution'],stack=execution['stack'],waiting=execution['waiting'])

def mandatory_initializer(wire:MandatoryWire,vocabulary:dict[str,Any],*,legacy_implicit:bool=False)->dict[str,Any]:
    """Encode only the actually recorded supplied fixture inputs, with complete source-backed Programs."""
    static=wire.run['static_inputs'];setup=static['setup'];definitions=unique(static['definitions'],'reference','actual initializer definition');context=wire.contexts[wire.primary_context_reference()];native_context={key:value for key,value in context.items() if key!='reference'};cards=[]
    for card in setup['cards']:
        destination=card['destination'];tag={'library_top':'LibraryTop','library_bottom':'LibraryBottom','hand':'Hand','graveyard':'Graveyard','battlefield':'Battlefield','stack':'Stack','exile':'Exile','command':'Command'}[destination];location=dict(tag=tag)
        if destination in ('battlefield','stack'):location['controller']=card['controller']
        cards.append(dict(card=card['card_id'],owner=card['owner'],destination=location,program=definitions[card['definition_ref']]['program']))
    # fixture.rs authors the historical ordinary fixture with Opening and an
    # empty initial battlefield order; record.rs deliberately omits those two
    # fields from Report1. No fields are added to its observation document.
    order=[] if legacy_implicit else setup['initial_battlefield_order']
    seed=dict(seats=setup['participants'],active=setup['active'],combat_rules={'unrestricted_free_for_all':'UnrestrictedFreeForAll'}[setup['combat_rules']],cards=cards,initial_world_cohorts=[],initial_battlefield_order=[wire.aliases[alias] for alias in order],mana=[dict(player=row['player'],mana_type=_NATIVE_MANA[row['mana_type']],amount=row['amount']) for row in setup['mana']],life=setup['life'],creature_vocabulary=vocabulary['id'],name_rules=[],name_rules_id=native_context['name_rules_id'])
    initialization=dict(kind='opening') if legacy_implicit else setup['initialization'];common=dict(game=static['game_id'],context=native_context,seed=seed)
    if initialization['kind']=='opening':same(set(initialization),{'kind'},'exact opening initialization');return dict(tag='GameInitializeOpening',**common,entropy_words=static['native_entropy_words'])
    same(initialization['kind'],'scheduled','finite scheduled initialization');start=initialization['start'];return dict(tag='GameInitializeScheduled',**common,start=dict(number=str(start['number']),step={'untap':'Untap','upkeep':'Upkeep','draw':'Draw','precombat_main':'PrecombatMain','begin_combat':'BeginCombat','declare_attackers':'DeclareAttackers','declare_blockers':'DeclareBlockers','combat_damage':'CombatDamage','end_combat':'EndCombat','postcombat_main':'PostcombatMain','end':'End','cleanup':'Cleanup'}[start['step']]))


# Finite shapes from engine/test_support/projection::{cause,occurrence,rules_record}.
# The mandatory epoch explicitly excludes instruction choices and CreatureTypeChosen.
_JOURNAL_RECORD_FIELDS={'Event':'event cause occurrences','TriggerDiscovered':'value','TriggerPlacementStarted':'instance wave controller position entry','TriggerPlaced':'instance wave controller position entry','TriggerTargetSelected':'instance wave controller position entry target','TriggerRemoved':'instance wave controller position entry reason'}
_EVENT_CAUSE_FIELDS={'OpeningShuffle':'player phase','Draw':'player origin','OpeningCardsMoved':'phase cohort','LandPlay':'actor','CastAnnouncement':'entry','CastCompleted':'entry','ActivatedAbilityCost':'entry ability','ActivatedAbilityCompleted':'entry ability','ManaAbilityCost':'source ability','ManaAbilityActivated':'source ability','ManaProduced':'source ability','ManaDebited':'entry','Instruction':'entry instruction','CounterSpell':'entry instruction target','ResolutionCompleted':'entry completion','UntapTurn':'active','CleanupDiscard':'event boundary','CleanupCharacteristics':'position','StateBasedActions':'event boundary','DamageAdjusted':'batch','DamageDealt':'batch','DamageResults':'batch','AttackTaps':'position actor','AttackersDeclared':'position actor','BlockersDeclared':'position actor','CombatDamageAssigned':'position actor origin','CombatEnded':'position','ManaPoolsEmptied':'ended','StepBegan':'position'}
_OCCURRENCE_FIELDS={'TokenCreated':'creator after','TokenCeased':'object','ControllerChanged':'before after','LifeChanged':'change','ZoneChange':'before after reason','StatusChange':'before after reason','Shuffled':'value','FailedDraw':'player attempts','CastCompleted':'cast','ActivatedAbility':'entry source ability','ManaAbility':'source ability controller','ManaProduced':'production before after','ManaDebited':'payment type amount before after','ManaPoolEmptied':'player previous','CopyCreated':'copiable_card_types copiable_creature_subtypes copiable_names copiable_supertypes copiable_triggered entry original controller targets','WorldTenureChanged':'change','CopyMoved':'before after','CopyCeased':'copy','ContinuousEffectCreated':'effect appearances','CleanupCharacteristics':'expired_effects marks_removed appearances','CombatantsRemoved':'participants','CountersPlaced':'object before added after before_appearance after_appearance','CountersCancelled':'object before removed after','DamageEffectCreated':'value','DamagePrevented':'application damage_before shield_before','DamageMultiplied':'value','DamageDealt':'damage kind origin','DamageResult':'recipient before after components','PlayerLost':'player reasons','AttackersDeclared':'origin selected membership','BlockersDeclared':'origin selected membership statuses','CombatDamageAssigned':'value','CombatEnded':'previous','EntryCompleted':'entry controller kind completion','StepBegan':'position active'}


def validate_mandatory_journal(records:list[dict[str,Any]],*,additional_occurrence_fields:dict[str,str]|None=None)->None:
    """Admit only source-owned canonical journal grammar before lifetime interpretation."""
    require(isinstance(records,list),'required canonical rules journal array')
    occurrence_fields=_OCCURRENCE_FIELDS|({} if additional_occurrence_fields is None else additional_occurrence_fields)
    for record in records:
        tag=record['tag'];require(tag in _JOURNAL_RECORD_FIELDS,'source19 forbids canonical rules journal record '+str(tag));same(set(record),{'tag'}|set(_JOURNAL_RECORD_FIELDS[tag].split()),'exact actual canonical journal record fields')
        if tag!='Event':continue
        cause=record['cause'];tag=cause['tag'];require(tag in _EVENT_CAUSE_FIELDS,'source19 forbids canonical event cause '+str(tag));same(set(cause),{'tag'}|set(_EVENT_CAUSE_FIELDS[tag].split()),'exact actual canonical event cause fields');require(isinstance(record['occurrences'],list),'actual complete occurrence array')
        for occurrence in record['occurrences']:
            same(set(occurrence),{'id','value'},'exact actual identified occurrence fields');same(set(occurrence['id']),{'event','index'},'exact actual occurrence identity fields');value=occurrence['value'];tag=value['tag'];require(tag in occurrence_fields,'source19 forbids canonical rules occurrence '+str(tag));same(set(value),{'tag'}|set(occurrence_fields[tag].split()),'exact actual canonical occurrence fields')


# Wrapper ownership from test_support/error.rs Node::wrapped/tree! and game().
# This normalizes typed ownership only; it neither constructs errors nor parses Debug.
_ERROR_WRAPPER_DOMAINS={
    'GameError':{'Opening':'OpeningError','Entropy':'EntropyError','Turn':'TurnError','Effect':'EffectError'},
    'EffectError':{'Trigger':'TriggerError','Check':'CheckError','Combat':'CombatError','Window':'StackWindowError','Timing':'TimingError','InvalidManaPayment':'ManaPaymentError','Characteristics':'CharacteristicsError','CharacteristicInvariant':'CharacteristicInvariantError','Damage':'DamageError'},
    'StackWindowError':{'Priority':'PriorityError','Objects':'ObjectIdentityError','Targets':'TargetError'},
    'ObjectIdentityError':{'Entropy':'EntropyError','InvalidTokenDefinition':'DefinitionError'},
    'CharacteristicsError':{'InvalidParts':'CharacteristicDefinitionError'},
}


def native_error_leaf_domain(path:list[str],root_domain:str='GameError')->str:
    """Follow the exact source-owned native wrapper path to its typed leaf owner."""
    require(isinstance(path,list) and bool(path) and all(isinstance(part,str) and bool(part) for part in path),'actual nonempty typed native error path');domain=root_domain
    for wrapper in path[:-1]:
        children=_ERROR_WRAPPER_DOMAINS.get(domain,{});require(wrapper in children,'actual known native error wrapper owner');domain=children[wrapper]
    require(path[-1] not in _ERROR_WRAPPER_DOMAINS.get(domain,{}),'actual native error path reaches a leaf');return domain


class MandatoryWitnessIndex:
    PROFILE=PROFILES[1]
    OUTER_PROFILE='mandatory-trigger'
    REGISTRY_VERSION=7
    REPORT_VERSION=3
    CURRENT_VERSION=9
    WITNESS_SCHEMA='spellwake.current-type-context-witness.v7'
    SOURCE_COUNT=34
    PROGRAM_CONTEXT='mandatory-trigger-v1:registry7'
    WIRE=MandatoryWire
    REPLAY=staticmethod(mandatory_replay)
    STATE_SIGNATURE=staticmethod(native_state_signature)
    HEADER_SIGNATURE=staticmethod(ordinary_state_signature)
    JOURNAL=staticmethod(validate_mandatory_journal)
    REGISTRY=staticmethod(validate_registry)
    DEFINITIONS=staticmethod(validate_mandatory_native_definitions)
    VALIDATE_OWNED_PAYLOADS=False
    ENDPOINTS=staticmethod(mandatory_endpoints)
    REPORT_FACT_OMISSIONS:set[str]=set()
    COMPLETE_OBSERVATIONS=True
    CASE_OBSERVATION_NAMES=True
    REGISTERED_SEATS=NATIVE_MANDATORY_ROLES
    ALLOW_UNADOPTED_SUCCESS=False
    ALIAS_OBSERVATIONS_REQUIRED=True
    """Bounded whole-row validation and one coherent canonical call/point/state relation index."""
    MAX_ACCEPTED=10000  # Maximum explicit accepted-command budget in frozen input767b6f92.
    MAX_PROBES=28       # All22 declared adversaries/variants plus all6 declared mana-probe intents.
    def __init__(self)->None:
        self.calls:list[dict[str,Any]]=[];self.points:list[dict[str,Any]]=[];self.call_ids:set[str]=set();self.point_ids:set[str]=set();self.report:dict[str,Any]|None=None;self.run:dict[str,Any]|None=None;self.max_calls=1+self.MAX_ACCEPTED+self.MAX_PROBES;self.max_points=7+5*self.MAX_ACCEPTED+2*self.MAX_PROBES;self.object_cards:dict[str,int|None]={};self.object_origins:dict[str,str]={};self.observed_aliases:dict[str,str]={};self.programs:dict[str,Any]|None=None;self.program_digest:str|None=None;self.context:dict[str,Any]|None=None;self.vocabulary_id:str|None=None;self.seed:dict[str,Any]|None=None
        self.entry_payloads:dict[str,Any]={}

    def observe_report(self,inner:dict[str,Any])->None:
        same(set(inner),{'status','data'},'exact mandatory API response');same(inner['status'],'success','actual mandatory API success');report=inner['data'];same(set(report),{'schema_version','profile','registry_version','catalogue_sha256','runs'},'exact mandatory aggregate fields');same(report['schema_version'],1,'mandatory aggregate schema1');same(report['profile'],self.PROFILE,'actual mandatory aggregate profile');same(report['registry_version'],self.REGISTRY_VERSION,'actual mandatory registry epoch');same(len(report['runs']),1,'exactly one actual run per capture')
        self._observe_run(report,report['runs'][0])

    def _observe_run(self,report:dict[str,Any],run:dict[str,Any])->None:
        same(set(run),{'accepted','checkpoints','completed','failure','final','fixture_ref','initialization','observations','rejection_probes','rerun','schema_version','source_build_ref','static_inputs'},'exact ordinary RunReport fields');same(run['schema_version'],self.REPORT_VERSION,'actual selected Report epoch');same(run['fixture_ref'],run['static_inputs']['case_id'],'actual run/static-input case identity');same(run['completed'],True,'actual completed ordinary run');same(run['failure'],None,'actual no ordinary failure')
        n=len(run['accepted']);p=len(run['rejection_probes']);require(n<=self.MAX_ACCEPTED and p<=self.MAX_PROBES,'actual finite source-qualified report budget');self.max_calls=1+n+p;self.max_points=5+3*n+2*p+len(run['checkpoints'])+len(run['observations']);require(len(self.calls)<=self.max_calls and len(self.points)<=self.max_points,'streamed witness within actual report inventory');self.report=report;self.run=run

    def _programs(self,rows:list[dict[str,Any]],seed:bool=False)->None:
        values={}
        for row in rows:
            identity=row['card'] if seed else str(row['card_id']);require(identity not in values,'actual distinct physical Program id');values[identity]=row['program'] if seed else row['definition']
        digest=canonical_digest(values)
        if self.program_digest is None:self.program_digest=digest;self.programs=values
        else:same(digest,self.program_digest,'actual immutable source Program universe throughout capture')

    def _context(self,value:dict[str,Any])->None:
        same(set(value),{'release','rules','catalogue','programs','profile','creature_vocabulary','name_rules_id'},'exact actual native context metadata')
        if self.context is None:self.context=value
        else:same(value,self.context,'actual immutable root context throughout capture')
        if self.vocabulary_id is None:self.vocabulary_id=value['creature_vocabulary']
        else:same(value['creature_vocabulary'],self.vocabulary_id,'actual common native vocabulary identity')

    def _seed(self,value:dict[str,Any])->str:
        same(set(value),{'context','seed'},'exact supplied initializer projection');self._context(value['context']);seed=value['seed'];same(set(seed),{'seats','active','combat_rules','cards','initial_world_cohorts','initial_battlefield_order','mana','life','creature_vocabulary','name_rules','name_rules_id'},'exact complete supplied GameSeed');self._programs(seed['cards'],True);same(seed['creature_vocabulary'],self.vocabulary_id,'actual seed vocabulary');same(seed['name_rules_id'],self.context['name_rules_id'],'actual seed name-rule identity')
        if self.seed is None:self.seed=seed
        else:same(seed,self.seed,'actual complete initializer seed retained consistently')
        return canonical_digest(value)

    def _remember_object(self,obj:dict[str,Any])->None:
        """Retain only complete actual GameObjects from states and returned journal appearances."""
        same(set(obj),_OBJECT_FIELDS,'exact canonical object fields');identity=obj['object'];require(isinstance(identity,str) and re.fullmatch(r'ObjectId\([0-9]+\)',identity) is not None,'actual canonical ObjectId');physical=obj['physical_card'];require(obj['owner'] in self.REGISTERED_SEATS,'actual historical object registered owner')
        if physical is None:same(set(obj['origin']),{'kind','definition'},'actual complete token origin');same(obj['origin']['kind'],'token','actual token origin kind')
        else:
            require(type(physical) is int and str(physical) in self.programs,'actual historical physical origin has declared Program');same(obj['origin'],dict(kind='card',card_id=physical),'actual physical origin matches CardId')
        origin=canonical_digest(obj['origin'])
        if identity in self.object_cards:same(physical,self.object_cards[identity],'actual object incarnation has stable physical origin');same(origin,self.object_origins[identity],'actual object incarnation has stable complete origin')
        else:self.object_cards[identity]=physical;self.object_origins[identity]=origin

    def _journal_objects(self,value:Any)->None:
        """Mirror actual History appearances; input/reference operands never register an object."""
        if isinstance(value,dict):
            if 'physical_card' in value:self._remember_object(value)
            else:
                for member in value.values():self._journal_objects(member)
        elif isinstance(value,list):
            for member in value:self._journal_objects(member)

    def _state(self,state:dict[str,Any],observation:bool=False)->tuple[str,str]:
        signature=self.STATE_SIGNATURE(state);self._context(state['basis']['context']);self._programs(state['programs']);same(state['creature_vocabulary'],self.vocabulary_id,'actual state vocabulary');same(state['name_rules_id'],self.context['name_rules_id'],'actual state name rules');aliases={}
        for row in state['objects']:
            obj=row['object'];self._remember_object(obj);identity=obj['object'];physical=obj['physical_card']
            if observation and physical is not None:require(isinstance(row['alias'],str) and bool(row['alias']),'actual observation physical alias');aliases[str(physical)]=row['alias']
            else:same(row['alias'],None,'actual native/default and token aliases are null')
        if observation:
            if not self.observed_aliases:self.observed_aliases=aliases
            else:same(aliases,self.observed_aliases,'actual physical aliases stable at all observation points')
        if self.VALIDATE_OWNED_PAYLOADS:validate_optional_stack_payloads(state,self.object_cards,self.entry_payloads)
        # Only the redundant AliasContext projection differs across native/ordinary snapshot owners.
        normalized=dict(state,objects=[dict(row,alias=None) for row in state['objects']]);return canonical_digest(normalized),canonical_digest(signature)

    def add_call(self,call:dict[str,Any])->None:
        require(len(self.calls)<self.max_calls,'actual finite call index bound');same(set(call),{'adopted','after_point','append','before_point','call_id','input','owner_method','replay','result'},'exact complete captured call fields');identity=call['call_id'];require(isinstance(identity,str) and bool(identity) and identity not in self.call_ids,'actual unique call identity');self.call_ids.add(identity);require(type(call['adopted']) is bool,'actual typed call adoption');replay=call['replay'];same(set(replay),{'source_eq','full_result_eq','full_record_eq','input_storage','source_snapshot_before_invocation'},'exact complete native replay evidence');same(replay['full_result_eq'],True,'actual full native Result rerun');same(replay['full_record_eq'],True,'actual full native record rerun');same(replay['input_storage'],dict(tag='SavedTypedCall',call_id=identity),'actual saved typed-call identity');result=call['result'];saved={key:call[key] for key in ['adopted','after_point','before_point','call_id','input','owner_method','replay']}
        if result['tag']=='Ok':
            same(set(result),{'tag','candidate'},'exact successful native result')
            if not self.ALLOW_UNADOPTED_SUCCESS:same(call['adopted'],True,'actual successful ordinary call adopted')
            require(isinstance(call['after_point'],str) and bool(call['after_point']),'actual successful call has after point');candidate=result['candidate'];fields={'ability_activation','append','assignments','base','checks','damage','declarations','draws','land_play','mana_payment','mana_production','permanent_entry','shuffles','state','turn_transitions'};same(set(candidate),fields,'exact complete native candidate fields');same(call['append'],dict(tag='RulesJournal',value=candidate['append']),'actual returned complete journal equals call append');self.JOURNAL(candidate['append']);state_digest,signature=self._state(candidate['state']);self._journal_objects(candidate['append']);saved.update(result_tag='Ok',base=candidate['base'],state_digest=state_digest,state_signature=signature,facts={key:canonical_digest(value) for key,value in candidate.items() if key not in ('base','state')})
        else:
            same(result['tag'],'Err','closed captured native Result');same(set(result),{'tag','error','error_domain','error_leaf','error_path'},'exact native error projection fields');same(call['adopted'],False,'actual native error not adopted');same(call['after_point'],None,'actual native error has no after point');same(call['append'],dict(tag='NoReturnedTransition',reason='NativeError'),'actual error has no returned transition');same(result['error_domain'],'GameError','actual root error domain');same(set(result['error_leaf']),{'data','domain','variant'},'exact typed native error leaf');require(isinstance(result['error_path'],list) and bool(result['error_path']),'actual typed error path');same(result['error_leaf']['variant'],result['error_path'][-1],'actual leaf/path variant relation');same(result['error_leaf']['domain'],native_error_leaf_domain(result['error_path']),'actual typed native error leaf domain follows path owner');saved.update(result_tag='Err',error=result)
        self.calls.append(saved)

    def add_point(self,point:dict[str,Any])->None:
        require(len(self.points)<self.max_points,'actual finite point index bound');same(set(point),{'damage_parent','native_context','native_vocabulary','observation_pointers','owner','owner_source','phase','point_id','policy_vocabulary','program_source','projection','resolution'},'exact complete captured snapshot fields');identity=point['point_id'];require(isinstance(identity,str) and bool(identity) and identity not in self.point_ids,'actual unique point identity');self.point_ids.add(identity);require(isinstance(point['owner_source'],str) and bool(point['owner_source']),'actual point source owner');same(point['projection']['tag'],'Inline','actual Mandatory snapshot Inline projection');same(set(point['projection']),{'tag','value'},'exact Inline projection');value=point['projection']['value'];owner=point['owner'];phase=point['phase']
        if owner=='GameSeedInput':
            same(phase,'InitializeInput','actual supplied seed phase');digest=self._seed(value);source_ref=point['native_context']['source_ref'];require(isinstance(source_ref,str) and bool(source_ref),'actual supplied source reference');same(point['native_context'],dict(tag='SuppliedInput',source_ref=source_ref,value=value['context']),'actual supplied native context');same(point['native_vocabulary'],dict(tag='SuppliedInput',source_ref=source_ref,value=value['seed']['creature_vocabulary']),'actual supplied vocabulary');same(point['program_source'],dict(tag='SuppliedInput',source_ref=source_ref,value=value['seed']['cards']),'actual supplied whole Programs');same(point['resolution'],dict(tag='NotApplicable',reason='OwnerHasNoResolution'),'actual seed has no resolution owner');same(point['damage_parent'],dict(tag='NotApplicable',reason='OwnerHasNoDamageWork'),'actual seed has no damage owner');signature=None
        else:
            same(owner,'GameState','source19 snapshot owner');require(phase in ('BeforeCall','AfterCall','Observation'),'actual GameState phase');digest,signature=self._state(value,phase=='Observation' and self.ALIAS_OBSERVATIONS_REQUIRED);same(point['native_context'],dict(tag='NativeOwned',value=value['basis']['context']),'actual point native context owns real state');same(point['native_vocabulary'],dict(tag='NativeOwned',value=value['creature_vocabulary']),'actual point native vocabulary owns real state');same(point['program_source'],dict(tag='NativeOwned',value=[dict(card=str(row['card_id']),program=row['definition']) for row in value['programs']]),'actual complete Programs metadata owns state programs');same(point['resolution'],dict(tag='Applicable',value=value['resolution_state']),'actual point resolution owns real cursor');same(point['damage_parent'],dict(tag='Applicable',value=None if value['pending_damage'] is None else value['pending_damage']['parent']),'actual point damage-parent owns real state')
        source_ref=point['native_context'].get('source_ref');same(point['policy_vocabulary'],self.vocabulary_id,'actual policy/native vocabulary agreement');require(isinstance(point['observation_pointers'],list) and len(point['observation_pointers'])==1,'actual one source-owned pointer per Mandatory point');self.points.append(dict(point_id=identity,phase=phase,owner=owner,owner_source=point['owner_source'],pointers=point['observation_pointers'],state_digest=digest,state_signature=signature,source_ref=source_ref))

    def validate_source_owners(self,report:dict[str,Any],witness:dict[str,Any],manifest:dict[str,Any])->dict[str,Any]:
        run=self.run
        same(report['catalogue_sha256'],manifest['catalogue']['sha256'],'actual whole catalogue source');same(witness['registry']['tag'],'Registry','actual Registry witness');self.REGISTRY(witness['registry']['coverage'],manifest,self.PROFILE);vocabulary=read(Path(manifest['vocabulary']['path']));same(sha(Path(manifest['vocabulary']['path'])),manifest['vocabulary']['sha256'],'actual retained vocabulary source bytes');same(set(witness['vocabulary']),{'input','publication','value'},'exact published vocabulary fields');same(witness['vocabulary']['value'],vocabulary,'actual complete published vocabulary value');self.DEFINITIONS(run['static_inputs']['definitions'],manifest,vocabulary,{row['definition_ref'] for row in run['static_inputs']['aliases']});same(self.vocabulary_id,vocabulary['id'],'actual all-point vocabulary identity')
        return vocabulary

    def validate_context_owner(self,primary:dict[str,Any],report:dict[str,Any],manifest:dict[str,Any],vocabulary:dict[str,Any])->None:
        same(primary['profile'],self.PROFILE+':'+self.run['fixture_ref'],'actual context/case profile');same(primary['programs'],self.PROGRAM_CONTEXT,'actual context/Registry epoch');same(primary['release'],self.PROFILE,'actual context release');same(primary['rules'],vocabulary['rules_revision'],'actual source rules revision');same(primary['catalogue'],'oracle-sha256:'+report['catalogue_sha256'],'actual context catalogue');same(primary['creature_vocabulary'],vocabulary['id'],'actual context vocabulary')

    @evidence_boundary
    def finish(self,outer:dict[str,Any],witness:dict[str,Any],manifest:dict[str,Any],expected_case:str|None=None)->tuple[dict[str,Any],dict[str,Any]]:
        require(self.report is not None and self.run is not None,'actual mandatory report was consumed');report=self.report;run=self.run;self.REPLAY(run);n=len(run['accepted']);probes=run['rejection_probes'];p=len(probes);same(set(outer),{'observations','profile','schema_version','type_context_witness'},'exact current mandatory outer fields');same(outer['profile'],self.OUTER_PROFILE,'actual current Mandatory outer profile');same(outer['schema_version'],self.CURRENT_VERSION,'actual current schema9');same(witness['schema'],self.WITNESS_SCHEMA,'actual witness7');same(set(witness),{'calls','coverage','registry','schema','snapshots','source_inputs','vocabulary'},'exact current witness fields')
        if expected_case is not None:same(run['fixture_ref'],expected_case,'actual bound case identity')
        vocabulary=self.validate_source_owners(report,witness,manifest);sources=unique(witness['source_inputs'],'path','actual mandatory witness source');same(len(sources),self.SOURCE_COUNT if self.SOURCE_COUNT is not None else len(manifest['capture_source_rows']),'actual exact source-declared input inventory')
        for source in sources.values():same(set(source),{'path','sha256','bytes'},'exact supplied source receipt');require(isinstance(source['path'],str) and Path(source['path']).is_absolute(),'actual absolute supplied source path');require(re.fullmatch(r'[0-9a-f]{64}',source['sha256']) is not None and type(source['bytes']) is int and source['bytes']>0,'actual complete supplied source digest/bytes')
        same(len(self.calls),1+n+p,'actual exact independently derived call inventory');same(len(self.points),self.max_points,'actual exact independently derived point inventory');coverage=witness['coverage'];same(set(coverage),{'complete','observation_to_points','record_pointers','required_calls','required_points','uncovered'},'exact current coverage fields');same(coverage['complete'],True,'actual complete witness');same(coverage['uncovered'],[],'actual no uncovered fields');same(coverage['required_calls'],[row['call_id'] for row in self.calls],'actual exact ordered required calls');same(coverage['required_points'],[row['point_id'] for row in self.points],'actual exact ordered required points');points={row['point_id']:row for row in self.points}
        aliases={str(row['card_id']):row['alias'] for row in run['static_inputs']['aliases']};same(self.observed_aliases,aliases,'actual observation aliases equal declared physical aliases');history={identity:None if card is None else aliases[str(card)] for identity,card in self.object_cards.items()};wire=self.WIRE(run,history);initializer=mandatory_initializer(wire,vocabulary,legacy_implicit=self.REPORT_VERSION==1);same(self.seed,initializer['seed'],'actual complete declared initializer seed');same(self.context,initializer['context'],'actual complete declared context');primary=wire.contexts[wire.primary_context_reference()];self.validate_context_owner(primary,report,manifest,vocabulary)
        seed_value=dict(context=initializer['context'],seed=initializer['seed']);seed_digest=canonical_digest(seed_value);success_rows=[run['initialization'],*run['accepted']];world_digests={row['ordinal']:mandatory_world_digest(world) for row,world in self.ENDPOINTS(run)};
        if self.COMPLETE_OBSERVATIONS:same([row['candidate_ordinal'] for row in run['observations']],list(range(n+1)),'actual one selector for every candidate')
        expected=[]
        for ordinal,row in enumerate(success_rows):
            expected.append(('success',row))
            for probe in probes:
                if probe['attached_candidate_ordinal']==ordinal:expected.append(('probe',probe))
        same(len(expected),len(self.calls),'actual all probes attach to a reached source');native_points=set();native_pointers=[];state_digests={};latest=None
        for call,(kind,row) in zip(self.calls,expected,strict=True):
            identity=call['call_id'];before=points[call['before_point']];native_points.add(call['before_point']);same(before['owner_source'],call['owner_method'],'actual before-point source method owner');replay=call['replay'];ordinal=row['ordinal'] if kind=='success' else row['attached_candidate_ordinal']
            if kind=='success' and ordinal==0:
                same(call['input'],initializer,'actual complete supplied typed initializer call');same(call['owner_method'],'GameState::initialize_opening' if initializer['tag']=='GameInitializeOpening' else 'GameState::initialize_scheduled','actual initializer owner method');same(before['phase'],'InitializeInput','actual source initializer phase');same(before['owner'],'GameSeedInput','actual initializer input owner');same(before['state_digest'],seed_digest,'actual initializer source complete supplied input');same(before['source_ref'],'retained-actual-constructor-input','actual native retained constructor source owner');same(replay['source_eq'],None,'actual initializer source equality not applicable');same(replay['source_snapshot_before_invocation'],'InputOnlyForInitializer','actual initializer source-capture timing')
            else:
                same(call['input'],dict(tag='GameInput',value=wire.input(row['input'] if kind=='success' else row['attempted_input'],weak=kind=='probe')),'actual saved complete typed input/report relation');same(call['owner_method'],'GameState::advance','actual root advance owner method');same(before['phase'],'BeforeCall','actual before-call phase');same(before['owner'],'GameState','actual before-call owner');same(before['state_digest'],latest,'actual complete before-state equals latest adopted source');same(replay['source_eq'],True,'actual complete native source retained');same(replay['source_snapshot_before_invocation'],True,'actual source snapshot captured before invocation')
            if kind=='probe':
                same(call['result_tag'],'Err','actual recorded probe is native Err');same(call['error']['error_path'],row['actual_error']['path'],'actual typed native/report rejection path');same(call['error']['error_leaf']['data'],row['actual_error']['payload'],'actual typed native/report rejection payload');continue
            same(call['result_tag'],'Ok','actual accepted record has native success');same(call['adopted'],True,'actual accepted result adopted');after=points[call['after_point']];native_points.add(call['after_point']);same(after['phase'],'AfterCall','actual after-call phase');same(after['owner'],'GameState','actual after-call owner');same(after['owner_source'],call['owner_method'],'actual after-point source method owner');same(after['state_digest'],call['state_digest'],'actual whole candidate state equals after Inline state');same(after['state_signature'],call['state_signature'],'actual after-point canonical state signature');same(call['state_signature'],canonical_digest(self.HEADER_SIGNATURE(wire,row['header'],world_digests[ordinal])),'actual canonical candidate complete Header/World relation');base=row['base'];native_base=dict(tag='Initialize',game=base['game'],context={key:value for key,value in wire.contexts[base['context_ref']].items() if key!='reference'}) if ordinal==0 else dict(tag='Advance',basis=wire.basis(base['basis']));same(call['base'],native_base,'actual candidate source base relation');same({key:value for key,value in call['facts'].items() if key not in self.REPORT_FACT_OMISSIONS},{key:canonical_digest(value) for key,value in wire.facts(row['current_facts']).items()},'actual all declared complete returned candidate fact families');latest=call['state_digest'];state_digests[ordinal]=latest
        external={};base='/observations/data/runs/0';external[base+'/static_inputs']=(seed_digest,'InitializeInput','GameSeedInput','record::execute actual supplied fixture');external[base+'/initialization']=(state_digests[0],'Observation','GameState','record::Recorder::initial');external[base+'/final']=(state_digests[n],'Observation','GameState','record::execute actual final GameState')
        for checkpoint in run['checkpoints']:
            ordinal=int(checkpoint);same(str(ordinal),checkpoint,'actual canonical checkpoint ordinal');require(ordinal in state_digests,'actual checkpoint has adopted whole native state');external[base+'/checkpoints/'+checkpoint]=(state_digests[ordinal],'Observation','GameState','record::Recorder::names full checkpoint')
        for index,observation in enumerate(run['observations']):
            ordinal=observation['candidate_ordinal'];require(ordinal in state_digests,'actual observation has whole native candidate state')
            scope='full_checkpoint' if self.CASE_OBSERVATION_NAMES else observation['scope'];require(scope in ('full_checkpoint','candidate_evidence'),'actual declared predecessor observation scope')
            if scope=='full_checkpoint':require(str(ordinal) in run['checkpoints'],'actual full-checkpoint selector has checkpoint')
            selector=run['fixture_ref']+'.candidate.'+str(ordinal) if self.CASE_OBSERVATION_NAMES else observation['name'];same(observation,dict(candidate_ordinal=ordinal,name=selector,scope=scope,semantic_selector_ref=selector),'actual complete declared selector');external[base+'/observations/'+str(index)]=(state_digests[ordinal],'Observation','GameState','record::Recorder::names actual selector')
        for ordinal in range(1,n+1):external[base+'/accepted/'+str(ordinal-1)]=(state_digests[ordinal],'Observation','GameState','record::Recorder::accepted')
        for index,probe in enumerate(probes):external[base+'/rejection_probes/'+str(index)]=(state_digests[probe['attached_candidate_ordinal']],'Observation','GameState','record::Recorder::probes actual reached rejection')
        expected_joins={};seen_external=set()
        for index,point in enumerate(self.points):
            identity=point['point_id'];path=point['pointers'][0]
            if identity in native_points:
                own='/type_context_witness/snapshots/'+str(index)+'/projection/value';same(path,own,'actual native Inline point owns its exact emitted slot');native_pointers.append(own)
            else:
                require(path in external and path not in seen_external,'actual unique source-writer observation pointer');seen_external.add(path);digest,phase,owner,owner_source=external[path];same(point['owner_source'],owner_source,'actual source-writer observation method owner');same(point['phase'],phase,'actual source-writer observation phase');same(point['owner'],owner,'actual source-writer observation owner');same(point['state_digest'],digest,'actual source-writer complete state/seed relation')
                if owner=='GameSeedInput':same(point['source_ref'],'compiled ordinary fixture inputs','actual static supplied fixture source owner')
            require(path not in expected_joins,'actual one point per source-writer pointer');expected_joins[path]=[identity]
        same(seen_external,set(external),'actual complete independent ordinary observation point inventory');same(coverage['record_pointers'],native_pointers,'actual complete native record-pointer inventory');joins=unique(coverage['observation_to_points'],'pointer','actual observation pointer');same({path:row['points'] for path,row in joins.items()},expected_joins,'actual complete independently derived pointer/point relations')
        return report,dict(captured=True,complete=True,points=len(self.points),calls=len(self.calls),registry_version=self.REGISTRY_VERSION,complete_canonical_state_and_input_relations=True,complete_endpoint_world_and_all13_fact_relations=not self.REPORT_FACT_OMISSIONS,declared_report_fact_families=13-len(self.REPORT_FACT_OMISSIONS),canonical_native_append_still_complete=True)


@dataclass
class MandatoryCapture:
    """Validated actual capture result, keeping only source metadata after all full rows are checked."""
    report:dict[str,Any]
    provenance:dict[str,Any]
    source_witness:dict[str,Any]
    ingress:dict[str,Any]


@evidence_boundary
def read_mandatory_capture(path:Path,manifest:dict[str,Any],expected_case:str|None=None,*,index_factory:Callable[[],MandatoryWitnessIndex]=MandatoryWitnessIndex)->MandatoryCapture:
    """Strictly consume a complete capture with bounded value buffers and finite semantic indexes."""
    before=sha(path);index=index_factory();outer={};witness={};root_keys={'observations','profile','schema_version','type_context_witness'};witness_keys={'calls','coverage','registry','schema','snapshots','source_inputs','vocabulary'}
    with JsonStream.open(path) as stream:
        for key in stream.iter_object():
            require(key in root_keys,'unknown mandatory capture root field '+key)
            if key=='observations':
                value=stream.read_value();index.observe_report(value);outer[key]=value
            elif key=='type_context_witness':
                for field in stream.iter_object():
                    require(field in witness_keys,'unknown mandatory witness field '+field)
                    if field=='calls':
                        for ordinal in stream.iter_array():
                            require(ordinal<index.max_calls,'actual finite call index bound before row decode');index.add_call(stream.read_value())
                        witness[field]=None  # Full rows consumed/validated by the single index, never retained as a second DOM.
                    elif field=='snapshots':
                        for ordinal in stream.iter_array():
                            require(ordinal<index.max_points,'actual finite point index bound before row decode');index.add_point(stream.read_value())
                        witness[field]=None
                    else:witness[field]=stream.read_value()
                outer[key]=witness
            else:
                outer[key]=stream.read_value()
                if key=='schema_version':same(outer[key],index.CURRENT_VERSION,'actual selected current capture namespace')
                if key=='profile':same(outer[key],index.OUTER_PROFILE,'actual selected current capture profile')
        counters=dict(max_buffer_chars=stream.max_buffer_chars,read_calls=stream.read_calls,decode_attempts=stream.decode_attempts)
    report,provenance=index.finish(outer,witness,manifest,expected_case);after=sha(path);same(after,before,'actual complete native packet immutable during read');ingress=dict(path=str(path.resolve()),sha256=before,bytes=path.stat().st_size,source_packet_stable=True,**counters);return MandatoryCapture(report,provenance,dict(source_inputs=witness['source_inputs'],vocabulary=witness['vocabulary']),ingress)


class MandatoryLifecycle:
    """One journal-owned trigger lifetime and stable input-boundary execution index."""
    def __init__(self)->None:
        self.instances:dict[str,Any]={};self.placements:dict[str,Any]={};self.placed:dict[str,Any]={};self.completed:dict[str,int]={};self.selected_targets:dict[str,Any]={};self.entry_order:list[str]=[];self.settled:dict[str,str]={};self.spell_objects:dict[str,str]={};self.placement_names:list[str]=[]

    def admit(self,entry:str)->None:
        require(entry not in self.entry_order,'actual stack entry admitted once');self.entry_order.append(entry)

    def require_top(self,entry:str)->None:
        live=[identity for identity in self.entry_order if identity not in self.settled];require(bool(live),'actual resolution has a live stack');same(entry,live[-1],'actual body and resolution execute the current live top')

    def complete(self,entry:str,event:str)->None:
        require(entry in self.entry_order,'actual completed entry was admitted')
        if entry in self.settled:same(self.settled[entry],event,'actual one completion event per entry')
        else:self.settled[entry]=event

    def observe(self,header:dict[str,Any])->None:
        """Bind every exposed queue, stack payload, pending frame and cursor to actual journal lifetimes."""
        execution=header['execution'];same([entry['entry'] for entry in execution['stack']],[entry for entry in self.entry_order if entry not in self.settled],'actual bottom-to-top stack order follows admission and completion');waiting=unique(execution['waiting'],'instance','actual waiting instance');same(set(waiting),set(self.instances)-set(self.placed),'actual complete waiting inventory equals discovered minus installed')
        for identity,value in waiting.items():same(value,self.instances[identity],'actual waiting preserves complete discovered capture')
        active={identity:value for identity,value in self.placements.items() if identity not in self.placed};require(len(active)<=1,'actual at most one provisional placement');expected=next(iter(active.values()),None);same(execution['pending_trigger_placement'],expected,'actual complete started-not-installed placement inventory');placement=expected;stack=unique(execution['stack'],'entry','actual live stack entry');live_installed={}
        for entry,value in stack.items():
            if value['kind']['tag']!='TriggeredAbility':continue
            if placement is not None and entry==placement['entry']:
                same(value['body'],None,'actual provisional entry has no installed body');same(value['bindings'],None,'actual provisional entry has no installed binding');same(value['controller'],placement['controller'],'actual provisional captured controller');same(value['kind'],dict(tag='TriggeredAbility',source=self.instances[placement['instance']]['source']['object']),'actual provisional stack source is captured incarnation');same(value['targets'],[],'actual provisional entry has no selected target');continue
            require(value['bindings'] is not None,'actual installed trigger requires complete binding');binding=value['bindings']['trigger'];identity=binding['instance'];require(identity in self.placed,'actual stack payload requires prior TriggerPlaced');same(entry,self.placed[identity]['entry'],'actual installed entry matches TriggerPlaced');require(entry not in self.completed,'actual completed trigger absent from live stack');same(value['controller'],self.instances[identity]['controller'],'actual installed captured controller');same(value['kind'],dict(tag='TriggeredAbility',source=self.instances[identity]['source']['object']),'actual installed stack source is captured incarnation');same(binding,{key:self.instances[identity][key] for key in ['instance','definition','causes','source']},'actual installed complete captured binding');same(value['body'],self.instances[identity]['body'],'actual installed complete captured body');same(value['targets'],[self.selected_targets[identity]] if identity in self.selected_targets else [],'actual installed stack target equals recorded target occurrence');same(identity in self.selected_targets,self.instances[identity]['body']['target']['tag']!='None','actual installed source-required target selected exactly once');live_installed[identity]=entry
        same(set(live_installed),{identity for identity,value in self.placed.items() if value['entry'] not in self.completed},'actual complete live installed trigger inventory')
        if placement is not None:require(placement['entry'] in stack and placement['instance'] in waiting,'actual provisional entry and waiting capture both present')
        phase=header['phase'];kind=phase['kind'];pending=header['pending'];family=None if pending is None else pending['frame']['kind'];cursor=execution['resolution'];priority=phase.get('phase') if kind=='priority' else None
        if kind=='priority':
            require(priority['kind'] in ('ready','requires_checks','all_passed'),'closed actual priority phase');same(set(priority),{'kind','holder'} if priority['kind']=='ready' else {'kind','candidate'} if priority['kind']=='requires_checks' else {'kind'},'exact actual priority phase fields')
            if priority['kind']=='ready':require(priority['holder'] in header['participants'],'actual registered priority holder')
            elif priority['kind']=='requires_checks':require(priority['candidate'] in header['participants'],'actual registered check candidate')
        require(kind in ('opening','turn_work','priority','resolving','checking_targets','all_targets_illegal','step_ended'),'closed actual window phase');same(set(phase),{'kind','phase'} if kind=='priority' else {'kind','entry'} if kind in ('resolving','checking_targets','all_targets_illegal') else {'kind'},'exact actual phase fields');same(header['priority_holder'],priority['holder'] if priority is not None and priority['kind']=='ready' else None,'actual priority holder agrees with phase')
        if pending is not None:
            same(pending['basis'],header['basis'],'actual issued pending basis');require(pending['chooser'] in header['participants'],'actual registered decision chooser');same(set(pending),{'id','basis','chooser','frame'},'exact actual decision envelope fields');same(set(pending['frame']),{'kind','frame'},'exact actual frame envelope fields');frame=pending['frame']['frame']
            fields={'triggers':{'boundary','chooser','domain','event','revision','wave'},'trigger_target':{'boundary','chooser','domain','entry','event','instance','revision','wave'},'timestamp_order':{'chooser','entry','instruction','members','revision','tag'}}
            if family=='opening':fields[family]={'kind','player','taken','can_mulligan'} if frame['kind']=='declare' else {'kind','player','count','exact_hand'}
            elif family=='combat':fields[family]={'kind','chooser','revision'}|({'eligible_attackers','defenders'} if frame['kind']=='attackers' else {'eligible_blockers','attackers'} if frame['kind']=='blockers' else {'position','domains'})
            elif family=='checks':fields[family]={'kind','chooser','revision','boundary','event'}|({'conflicts'} if frame['kind']=='legend_choice' else {'domain','required'})
            require(family in fields,'source19 known issued frame');same(set(frame),fields[family],'exact actual '+str(family)+' frame fields')
            if family in ('triggers','trigger_target','checks'):
                boundary=frame['boundary'];require(boundary['kind'] in ('priority','cleanup'),'closed check boundary');same(set(boundary),{'kind'} if boundary['kind']=='priority' else {'kind','position'},'exact actual check boundary fields');same(set(frame['event']),{'started_revision','pass'},'exact actual check event fields')
                for value in frame['event'].values():require(type(value) is int and value>=0,'actual unsigned check event fields')
                require(frame['event']['started_revision']<=integer(header['executor_revision'],'actual executor revision'),'actual check event begins no later than issued frame')
            if family=='triggers':same(set(frame['domain']),{identity for identity,capture in waiting.items() if capture['controller']==pending['chooser']},'actual complete current controller trigger domain');same(len(frame['domain']),len(set(frame['domain'])),'actual trigger domain no duplicate')
            if family=='trigger_target':
                for target in frame['domain']:same(set(target),{'tag','player'},'exact source19 player target domain member');same(target['tag'],'Player','source19 player-only target domain')
                same({target['player'] for target in frame['domain']},set(header['participants']),'actual player target domain membership');same(len(frame['domain']),len(header['participants']),'actual distinct complete player target domain')
            if family=='timestamp_order':
                same(frame['tag'],'TimestampOrder','actual canonical timestamp tag')
                for member in frame['members']:same(set(member),{'creator','definition','ordinal'},'exact actual timestamp member fields');require(type(member['ordinal']) is int and member['ordinal']>=0 and member['creator'] in header['participants'],'actual timestamp member ordinal and creator')
                same([member['ordinal'] for member in frame['members']],list(range(len(frame['members']))),'actual complete declared timestamp member domain')
            if 'chooser' in frame:same(frame['chooser'],pending['chooser'],'actual inner frame chooser agrees with envelope')
            if family in ('triggers','trigger_target','timestamp_order','checks'):same(frame['revision'],integer(header['executor_revision'],'actual check revision') if family=='checks' else header['executor_revision'],'actual inner frame executor revision')
            if family=='opening':same(kind,'opening','actual opening frame phase');same(header['pending_turn_work'],None,'opening has no turn work')
            elif family=='combat':same(kind,'turn_work','actual combat frame phase');require(header['pending_turn_work'] is not None,'actual combat decision retains turn work')
            elif family in ('triggers','trigger_target','checks'):same(phase,dict(kind='priority',phase=dict(kind='requires_checks',candidate=priority['candidate'] if priority is not None and 'candidate' in priority else None)),'actual check decision requires check phase');same(header['pending_turn_work'],None,'actual check frame outside turn work')
            elif family=='timestamp_order':same(kind,'resolving','actual timestamp frame suspends body');same(header['pending_turn_work'],None,'actual timestamp frame outside turn work')
            else:raise EvidenceError('unsupported source19 issued decision family '+str(family))
        elif header['outcome'] is None:same(phase,dict(kind='priority',phase=dict(kind='ready',holder=header['priority_holder'])),'actual nonterminal stable input boundary is Ready');same(header['pending_turn_work'],None,'actual Ready has no unfinished turn work')
        same(placement is not None,family=='trigger_target','actual target decision owns precisely one provisional placement')
        if family=='trigger_target':
            frame=pending['frame']['frame'];same(frame['entry'],placement['entry'],'actual pending target entry');same(frame['instance'],placement['instance'],'actual pending target instance');same(frame['wave'],placement['wave'],'actual pending target placement wave');same(pending['chooser'],placement['controller'],'actual pending target captured controller')
        same(header['instruction_index'],None if cursor is None else integer(cursor['instruction'],'actual cursor index'),'actual header instruction index agrees with cursor')
        if family=='timestamp_order':
            require(cursor is not None,'actual timestamp decision retains resolution cursor');frame=pending['frame']['frame'];same(cursor['entry'],frame['entry'],'actual timestamp creation/cursor entry');same(pending['chooser'],stack[cursor['entry']]['controller'],'actual timestamp chooser is creating body controller');same(cursor['instruction'],str(frame['instruction']),'actual timestamp creation/cursor instruction');same(phase['entry'],cursor['entry'],'actual resolving phase/cursor entry');require(cursor['entry'] in stack,'actual cursor live entry');body=stack[cursor['entry']]['body'];index=integer(cursor['instruction'],'actual resolution instruction');require(body is not None and 0<=index<len(body['instructions']),'actual cursor within installed body');same(body['instructions'][index]['op'],'CreateTokens','actual timestamp suspends creating instruction');same(cursor['locals'],[],'actual source19 cursor has no locals')
        else:same(cursor,None,'actual source19 stable boundary has no unexplained resolution cursor')


def token_birth_alias(entry_label:str,instruction:Any,creator:str,member:int)->str:
    """Shared source-entry/instruction/recipient/member identity, never an ObjectId alias guess."""
    return 'token:'+entry_label+':'+str(instruction)+':'+creator+':'+str(member)


def ordinary_endpoint_world(header:dict[str,Any],current:dict[str,Any],tokens:dict[str,Any],piles:list[dict[str,Any]],physical:dict[str,Any],definitions:dict[str,Any],history:dict[str,str],remember:Callable[[dict[str,Any]],str])->dict[str,Any]:
    """The shared physical/token endpoint observer, moved from the maintained mandatory reader."""
    cards={};players={role:dict(life=header['life'][str(seat)],mana={key:header['mana'][str(seat)][value] for key,value in [('W','white'),('U','blue'),('B','black'),('R','red'),('G','green'),('C','colorless')]},library=[],hand=[],battlefield=[],graveyard=[],exile=[]) for seat,role in NATIVE_MANDATORY_ROLES.items()}
    require(len(current)==len(physical),'native complete physical card rows')
    for key,value in current.items():
        alias=value['alias'];require(alias in physical,'native known physical alias');same(str(value['object']['physical_card']),key,'actual CardId-key relation');same(remember(value['object']),alias,'actual physical observation alias')
    for identity,value in tokens.items():same(identity,value['object']['object'],'actual token endpoint identity');same(set(value),{'object','program','characteristics','creature','counters'},'complete Report3 token fields');remember(value['object']);same(value['program'],value['object']['origin']['definition'],'actual token definition vs origin')
    for value in [*current.values(),*tokens.values()]:
        obj=value['object'];alias=remember(obj);zone=obj['zone']['kind'];role=mandatory_role(obj['owner']);creature=value['creature'];counters=value['counters'];bf=zone=='battlefield';is_creature='Creature' in (value['characteristics']['current_types'] if obj['physical_card'] is None else definitions[physical[alias]['oracle_id']]['program']['card_types']);same(creature is not None,bf and is_creature,'actual required current creature observation presence');same(counters is not None,bf,'actual required current stored counter observation presence')
        if counters is not None:same(set(counters),{'plus_one','minus_one'},'complete stored counter inventory');require(integer(counters['plus_one'],'actual stored positive counters')>=0 and integer(counters['minus_one'],'actual stored negative counters')>=0,'actual nonnegative stored counters')
        if not bf:same(creature,None,'nonbattlefield creature observation absent');same(counters,None,'nonbattlefield counter observation absent')
        if creature is not None:
            same(set(creature),{'power','toughness','marked_damage'},'complete current creature fields')
            for field in ['power','toughness','marked_damage']:integer(creature[field],'native creature '+field)
        cards[alias]=dict(owner=role,controller=None if obj['controller'] is None else mandatory_role(obj['controller']),zone=zone,tapped=obj['tapped'],power=None if creature is None else creature['power'],toughness=None if creature is None else creature['toughness'],marked_damage=None if creature is None else creature['marked_damage'],plus_one_counters=None if counters is None else counters['plus_one'],minus_one_counters=None if counters is None else counters['minus_one'],continuous_control_since_turn_start=obj['continuous_control_since_turn_start'],object=obj['object'])
        if zone in ('hand','battlefield','exile'):players[role][zone].append(alias)
    for pile in piles:
        role=mandatory_role(pile['pile']['owner']);kind=pile['pile']['kind'];require(kind in ('library','graveyard'),'actual ordered physical pile kind');players[role][kind]=[history[row['object']] for row in pile['objects_top_to_bottom']]
        for ref in pile['objects_top_to_bottom']:same(ref['alias'],history[ref['object']],'actual ordered pile appearance binding')
    for player in players.values():
        for zone in ['hand','battlefield','exile']:player[zone].sort()
    return dict(cards=cards,players=players)


@evidence_boundary
def mandatory_native_projection(run:dict[str,Any],case:dict[str,Any],manifest:dict[str,Any],vocabulary:dict[str,Any])->dict[str,Any]:
    """Read Report3 through canonical journals and endpoint deltas; never infer absent transient records."""
    same(set(run),{'accepted','checkpoints','completed','failure','final','fixture_ref','initialization','observations','rejection_probes','rerun','schema_version','source_build_ref','static_inputs'},'exact mandatory RunReport3 fields');same(run['schema_version'],3,'actual mandatory RunReport3');same(run['fixture_ref'],case['id'],'actual native case');same(run['completed'],True,'actual native completed');same(run['failure'],None,'actual native no failure');definitions=validate_mandatory_native_definitions(run['static_inputs']['definitions'],manifest,vocabulary,{row['oracle_id'] for row in case['inventory']})
    accepted=run['accepted'];probes=mandatory_replay(run)
    bindings=unique(run['static_inputs']['aliases'],'alias','native physical alias');physical=unique(case['inventory'],'alias','frozen physical alias');same(set(bindings),set(physical),'actual exact mandatory physical inventory')
    for alias,row in bindings.items():same(row['definition_ref'],physical[alias]['oracle_id'],'actual physical source definition');same(mandatory_role(row['owner']),physical[alias]['owner'],'actual physical owner')
    final=run['final'];same(final['policy_complete'],True,'actual consumed complete policy');same(final['policy_entries_consumed'],final['policy_entries_total'],'actual no remaining instruction');same(final['accepted_inputs'],len(accepted),'actual accepted input denominator');same(final['final_ordinal'],len(accepted),'actual final ordinal')
    current=dict(run['checkpoints']['0']['cards']);tokens=dict(run['checkpoints']['0']['tokens']);piles=list(run['checkpoints']['0']['ordered_piles']);history={};births={};lineage={};events={};instances={};trigger_names={};entry_names={};casts={};placed={};placements={};targets={};journals=[];states={};draws=[];deaths=[];life_events=[];completed={};failed_draws=[];losses=[];ceased=[];zone_changes=[];productions=[];graveyard_choices=[];payments={};funding={'P0':[],'P1':[]};previous=None
    raw_world_digests={};timestamp_frames=[];cast_dispositions={};cast_targets={};lifecycle=MandatoryLifecycle();lifecycle.instances=instances;lifecycle.placements=placements;lifecycle.placed=placed;lifecycle.completed=completed
    instructions={row['id']:row for row in case['instructions']};declared={row['name']:row for group in case['trigger_groups'] for row in group['triggers']}
    def remember(obj:dict[str,Any])->str:
        identity=obj['object'];require(re.fullmatch(r'ObjectId\([0-9]+\)',identity) is not None,'actual opaque native object identity')
        if obj['physical_card'] is not None:
            require(type(obj['physical_card']) is int,'actual physical CardId');alias=next(row['alias'] for row in bindings.values() if str(row['card_id'])==str(obj['physical_card']));same(obj['origin'],dict(kind='card',card_id=obj['physical_card']),'actual physical origin');same(mandatory_role(obj['owner']),physical[alias]['owner'],'actual object owner')
        else:
            same(obj['origin']['kind'],'token','known actual token origin');require(identity in lineage,'native token must join actual birth lineage');alias=lineage[identity]
        if identity in history:same(history[identity],alias,'stable actual incarnation/alias relation')
        history[identity]=alias;return alias
    def appearance(value:dict[str,Any])->str:return remember(value['object'])
    def entry(reference:dict[str,Any])->str|None:
        name=reference.get('operation',reference.get('trigger'));return next((identity for identity,bound in entry_names.items() if bound==name),None)
    def cause_matches(name:str,event:dict[str,Any],row:dict[str,Any])->bool:
        operations=[op for op in case['causal_operations'] if op['name']==name]
        if len(operations)!=1:return False
        operation=operations[0];cause=event['cause']
        if 'entry' in operation:return entry(operation['entry']) is not None and cause.get('entry')==entry(operation['entry']) and cause['tag'] in ('Instruction','ResolutionCompleted','CounterSpell')
        return row.get('policy_entry_ref','').startswith(operation['combat_instruction']) and cause['tag'] in ('DamageAdjusted','DamageDealt','DamageResults','StateBasedActions')
    def state_world(header:dict[str,Any])->dict[str,Any]:
        same(set(header),{'active','basis','battlefield_counts_by_controller','combat','combat_rules','counts_by_owner','entropy','execution','executor_revision','instruction_index','life','losses','mana','opening','outcome','participants','pending','pending_eliminations','pending_turn_work','phase','priority_holder','registry_revision','turn','window_revision'},'exact Report3 common Header plus canonical execution')
        same(header['participants'],[41,3],'actual APNAP participant order independent of numerical sorting');require('stack' not in header and 'interaction_execution' not in header,'Report3 omits historical Header adapters');execution=header['execution'];same(set(execution),{'stack','resolution','waiting','pending_trigger_placement'},'exact canonical ExecutionObservation fields')
        common=ordinary_endpoint_world(header,current,tokens,piles,physical,definitions,history,remember);cards=common['cards'];players=common['players']
        for stack in execution['stack']:
            require(stack['controller'] in NATIVE_MANDATORY_ROLES,'known actual stack controller');kind=stack['kind']['tag'];require(kind in ('CardSpell','TriggeredAbility'),'finite mandatory actual stack kinds')
            if kind=='CardSpell':
                alias=history[stack['kind']['object']];program=definitions[physical[alias]['oracle_id']]['program'];same(stack['body'],program['kind'].get('body'),'actual complete source-linked spell body')
                for field,source_field in [('copiable_card_types','card_types'),('copiable_creature_subtypes','creature_subtypes'),('copiable_names','names'),('copiable_supertypes','supertypes'),('copiable_triggered','triggered')]:same(stack[field],program[source_field],'actual complete copiable source '+field)
        lifecycle.observe(header)
        return dict(header=header,cards=cards,players=players,execution=execution)
    for row,endpoint in mandatory_endpoints(run):
        current=endpoint['cards'];tokens=endpoint['tokens'];piles=endpoint['ordered_piles'];raw_world_digests[row['ordinal']]=mandatory_world_digest(endpoint)
        ordinal=row['ordinal'];same(set(row),{'after_basis','base','checkpoint_ordinal','current_facts','header','ordinal'} if ordinal==0 else {'after_basis','base','card_changes','current_facts','header','input','ordinal','pile_changes','policy_entry_ref'}|({'token_changes'} if token_changes_required else set()),'exact selected Report initializer/accepted fields');header=row['header'];facts=row['current_facts'];same(set(facts),{'ability_activation','check_journal','combat_assignments','combat_declarations','damage_journal','draw_journal','land_play','mana_payment','mana_production','permanent_entry','rules_journal','shuffle_journal','turn_transitions'},'exact complete Facts families plus canonical journal');validate_mandatory_journal(facts['rules_journal'])
        if previous is not None:same(row['base'],dict(kind='advance',basis=previous['basis']),'actual adopted candidate ancestry');same(row['input']['basis'],previous['basis'],'actual retained issued input basis')
        same(row['after_basis'],header['basis'],'actual successor/header basis')
        for value in current.values():remember(value['object'])
        if previous is not None:
            command=row['input']['command'];actor=mandatory_role(row['input']['actor']);kind=command['kind'];reference=row['policy_entry_ref'];expected=instructions.get(reference)
            if kind=='reply':
                pending=previous['pending'];require(pending is not None,'actual reply has issued native decision');same(command['decision'],pending['id'],'actual outer decision authority');same(row['input']['actor'],pending['chooser'],'actual decision chooser');same(pending['basis'],previous['basis'],'actual issued decision basis');frame=pending['frame']['frame'];choice=command['choice'];family=pending['frame']['kind']
                if family=='triggers':
                    same(choice['kind'],'order_triggers','actual trigger-order reply kind');domain=frame['domain'];require(len(domain)==len(set(domain)) and all(identity in trigger_names for identity in domain),'actual complete known trigger domain');names=[trigger_names[identity] for identity in domain];groups=[group for group in case['trigger_groups'] if set(names)=={trigger['name'] for trigger in group['triggers'] if trigger['captured_controller']==actor}];same(len(groups),1,'actual complete controller order group');same([trigger_names[identity] for identity in choice['ordered']],[name for name in groups[0]['order_bottom_first'] if name in names],'actual native complete trigger permutation');same(frame['chooser'],pending['chooser'],'actual trigger order chooser envelope')
                elif family=='trigger_target':
                    same(choice['kind'],'trigger_target','actual player-target reply kind');name=trigger_names[frame['instance']];policy=next(policy for policy in case['player_choices'] if policy['trigger']==name);same(actor,policy['chooser'],'actual named target chooser');same({mandatory_role(target['player']) for target in frame['domain']},set(policy['domain_roles']),'actual semantic player target membership independent of array order');same(len(frame['domain']),2,'actual complete target membership');same({target['tag'] for target in frame['domain']},{'Player'},'actual player-only domain');same(choice['target'],dict(kind='player',player=41 if policy['target']=='P0' else 3),'actual target reply value');same(frame['entry'],entry(dict(trigger=name)),'actual target frame installed/provisional entry relation');same(previous['execution']['pending_trigger_placement']['instance'],frame['instance'],'actual target frame placement instance')
                elif family=='timestamp_order':
                    same(choice['kind'],'timestamp_order','actual timestamp reply kind');policies=[policy for policy in case['timestamp_choices'] if entry(policy['creation_entry'])==frame['entry'] and policy['instruction']==frame['instruction']];same(len(policies),1,'actual named creation instruction timestamp frame');policy=policies[0];same(actor,policy['chooser'],'actual timestamp chooser');members=[member['ordinal'] for member in frame['members']];same(sorted(members),list(range(len(members))),'actual complete distinct token member domain');same(choice['ordered'],policy['members_earliest_first'],'actual native-only timestamp permutation');same(set(choice['ordered']),set(members),'actual exact timestamp member bijection');timestamp_frames.append(dict(entry=frame['entry'],instruction=frame['instruction'],members=frame['members']))
                elif family=='checks' and frame['kind']=='graveyard_order':
                    same(choice['kind'],'order_graveyard','actual graveyard reply kind');policies=[policy for policy in case['graveyard_choices'] if policy['chooser']==actor];same(len(policies),1,'actual required graveyard policy');policy=policies[0];same(sorted(remember(value) for value in frame['domain']),sorted(reference_alias(ref) for ref in policy['domain']),'actual complete pre-move graveyard domain');same([history[value['object']] for value in choice['ordered']],[reference_alias(ref) for ref in policy['entrants_top_first']],'actual native graveyard top-first permutation');same(frame['required'],len(frame['domain']),'actual graveyard domain denominator')
            else:same(row['input']['actor'],previous['priority_holder'],'actual priority command holder')
            if expected is not None and expected['kind'] in ('play_land','activate_mana','cast_untargeted','cast_targeted'):
                point=expected['point'];boundary=point['boundary'];same(previous['turn']['position']['number'],boundary['turn'],'actual instruction turn');same(previous['turn']['position']['step'],boundary['step'].replace('-','_'),'actual instruction step');same(mandatory_role(previous['active']),boundary['active'],'actual instruction active');same(actor,point['actor'],'actual instruction actor');same(mandatory_role(previous['priority_holder']),boundary['holder'],'actual instruction holder');same([value['entry'] for value in previous['execution']['stack']],[entry(ref) for ref in point['stack_bottom_first']],'actual complete instruction stack cut');source=reference_alias(expected['source']);field='land' if expected['kind']=='play_land' else 'source' if expected['kind']=='activate_mana' else 'spell';same(command[field]['alias'],source,'actual instruction source alias');same(command[field]['object'],states[ordinal-1]['cards'][source]['object'],'actual current instruction source incarnation')
                if expected['kind']=='activate_mana':
                    same(kind,'activate_mana','actual mana command');production=facts['mana_production'];same(production['source'],command['source'],'actual mana source receipt');same(production['controller'],row['input']['actor'],'actual production controller');same(production['ability'],command['ability'],'actual printed/intrinsic ability receipt');funding[actor].append(source)
                elif expected['kind'].startswith('cast_'):
                    require(kind in ('cast_paid','cast_untargeted'),'actual paid cast command');payment=facts['mana_payment'];expected_cost=definitions[physical[source]['oracle_id']]['program']['kind']['casting_cost'];same(payment['cost'],dict(generic=expected_cost['generic'],required={key.lower().replace('colourless','colorless'):value for key,value in expected_cost['required'].items()}),'actual complete source-derived casting cost');same(payment['payer'],row['input']['actor'],'actual paid caster');same(command['payment']['spend'],payment['spend'],'actual submitted/debited spend');same(payment['spend'],[dict(mana_type=spend['mana_type'].lower().replace('colourless','colorless'),amount=spend['amount']) for spend in expected['payment']],'actual exact finite spend');same(sum(integer(spend['amount'],'actual spend') for spend in payment['spend']),len(funding[actor]),'actual complete source funding attribution');payments[reference]=dict(payer=actor,cost=payment['cost'],spend=payment['spend'],sources=list(funding[actor]),entry=payment['source']);funding[actor]=[]
                    if expected['kind']=='cast_untargeted':require('target' not in command,'actual untargeted command has no target')
                    else:
                        target=expected['target'];actual=command['target']
                        if target['kind']=='object':same(actual,dict(kind='card',object=dict(alias=reference_alias(target['object']),object=states[ordinal-1]['cards'][reference_alias(target['object'])]['object'])),'actual current object target')
                        elif target['kind']=='stack':same(actual,dict(kind='stack',entry=entry(target['entry'])),'actual live stack target')
                        else:same(actual,dict(kind='player',player=41 if target['player']=='P0' else 3),'actual selected player cast target')
        for choice in facts['check_journal']['choices']:
            if choice['kind']=='graveyard_order':graveyard_choices.append(dict(ordinal=ordinal,choice=choice,issued=None if previous is None else previous['pending']))
        for journal in facts['rules_journal']:
            journals.append(dict(ordinal=ordinal,record=journal));tag=journal['tag']
            if tag=='Event':
                identity=journal['event'];integer(identity,'native EventId');require(identity not in events,'unique actual event identity');require(not events or integer(identity,'actual event identity')>integer(next(reversed(events)),'previous actual event identity'),'actual chronological native event identities');events[identity]=journal
                same([occ['id'] for occ in journal['occurrences']],[dict(event=identity,index=str(index)) for index,_ in enumerate(journal['occurrences'])],'actual complete ordered occurrence identities')
                cause=journal['cause'];body_entry=cause.get('entry') if cause['tag'] in ('Instruction','CounterSpell','ResolutionCompleted') else cause.get('origin',{}).get('entry') if cause['tag']=='Draw' and cause.get('origin',{}).get('tag')=='Instruction' else None
                if body_entry is not None:lifecycle.require_top(body_entry)
                if cause['tag']=='ResolutionCompleted':completed[cause['entry']]=ordinal;lifecycle.complete(cause['entry'],identity)
                member_counts={}
                for occurrence in journal['occurrences']:
                    value=occurrence['value'];family=value['tag']
                    if family=='CastCompleted':
                        cast=value['cast'];name=row['policy_entry_ref'];require(name in instructions and instructions[name]['kind'] in ('cast_untargeted','cast_targeted'),'actual named cast instruction');require(cast['entry'] not in entry_names,'new actual cast entry');entry_names[cast['entry']]=name;casts[name]=cast;cast_targets[cast['entry']]=row['input']['command'].get('target');lifecycle.admit(cast['entry']);lifecycle.spell_objects[cast['spell']['object']['object']]=cast['entry'];same(appearance(cast['spell']),reference_alias(instructions[name]['source']),'actual source-linked native cast')
                    elif family=='TokenCreated':
                        cause=journal['cause'];same(cause['tag'],'Instruction','actual creating instruction cause');require(cause['entry'] in entry_names,'actual named creator entry');creator=mandatory_role(value['creator']);coordinate=(cause['entry'],cause['instruction'],creator);member=member_counts.get(coordinate,0);member_counts[coordinate]=member+1;alias=token_birth_alias(entry_names[cause['entry']],cause['instruction'],creator,member);obj=value['after']['object'];require(alias not in births and obj['object'] not in lineage,'actual unique birth lineage');births[alias]=dict(object=obj['object'],appearance=value['after'],event=identity,ordinal=ordinal,entry=cause['entry'],instruction=cause['instruction'],creator=creator,member=member);lineage[obj['object']]=alias;remember(obj)
                    elif family=='ZoneChange':
                        before=value['before']['object'];after=value['after']['object']
                        if before['object'] in lineage:lineage[after['object']]=lineage[before['object']]
                        before_alias=remember(before);same(remember(after),before_alias,'actual zone-change identity lineage')
                        if before['zone']['kind']=='stack' and after['zone']['kind']!='stack':
                            require(before['object'] in lifecycle.spell_objects,'actual stack departure has source-linked cast entry');actual_entry=lifecycle.spell_objects[before['object']];lifecycle.complete(actual_entry,identity);cast_name=entry_names[actual_entry];require(cast_name not in cast_dispositions,'actual cast settles once');reason=value['reason']['tag']
                            if reason=='Countered':
                                same(cause['tag'],'CounterSpell','actual countered source has CounterSpell cause');same(cause['target'],actual_entry,'actual countered source binds target entry');same(cast_targets[cause['entry']],dict(kind='stack',entry=actual_entry),'actual counter effect retains declared target');same(after['zone']['kind'],'graveyard','actual countered source graveyard disposition');cast_dispositions[cast_name]='countered'
                            else:
                                require(reason in ('PermanentResolution','ResolutionExit'),'actual finite stack-exit reason');same(cause['tag'],'ResolutionCompleted','actual resolved source has completion cause');same(cause['entry'],actual_entry,'actual resolution source entry');require(cause['completion'] in ('Resolved','AllTargetsIllegal'),'actual finite cast completion');same(after['zone']['kind'],'battlefield' if reason=='PermanentResolution' else 'graveyard','actual resolved source destination');cast_dispositions[cast_name]='resolved' if cause['completion']=='Resolved' else 'all_targets_illegal_nonresolution'
                        zone_changes.append(dict(alias=before_alias,before=before,after=after,ordinal=ordinal,event=identity,cause=journal['cause']))
                        if before['zone']['kind']=='battlefield' and after['zone']['kind']=='graveyard':deaths.append(dict(alias=before_alias,ordinal=ordinal,event=identity,before=before['object'],after=after['object']))
                        if before['zone']['kind']=='library' and after['zone']['kind']=='hand':draws.append(dict(alias=before_alias,player=mandatory_role(after['owner']),ordinal=ordinal,event=identity,cause=journal['cause']))
                    elif family=='EntryCompleted':lifecycle.complete(value['entry'],identity)
                    elif family=='TokenCeased':require(value['object']['object'] in lineage,'actual cessation must retain birth lineage');ceased.append(dict(alias=remember(value['object']),object=value['object']['object'],ordinal=ordinal,event=identity))
                    elif family=='LifeChanged':change=value['change'];life_events.append(dict(player=mandatory_role(change['player']),old=integer(change['before'],'actual old life'),new=integer(change['after'],'actual new life'),ordinal=ordinal,event=identity,cause=journal['cause']))
                    elif family=='FailedDraw':failed_draws.append(dict(player=mandatory_role(value['player']),attempts=integer(value['attempts'],'actual failed draw count'),ordinal=ordinal,event=identity,cause=journal['cause']))
                    elif family=='PlayerLost':losses.append(dict(player=mandatory_role(value['player']),reasons=value['reasons'],ordinal=ordinal,event=identity))
                    elif family=='ManaProduced':productions.append(dict(production=value['production'],ordinal=ordinal,event=identity,cause=journal['cause']))
                    elif family=='ManaPoolEmptied':funding[mandatory_role(value['player'])]=[]
            elif tag=='TriggerDiscovered':
                waiting=journal['value'];identity=waiting['instance'];require(identity not in instances,'unique actual trigger instance');instances[identity]=waiting;same(waiting['definition'],1,'source first authored trigger definition');causes=waiting['causes'];event=events[causes['event']];same(causes['key']['tag'],'Occurrence','finite EachOccurrence grouping');same(len(causes['members']),1,'complete EachOccurrence cause denominator');member=causes['members'][0];same(member['occurrence'],causes['key']['occurrence'],'actual cause key/member identity');occ=event['occurrences'][integer(member['occurrence']['index'],'occurrence index')];same(occ['id'],member['occurrence'],'actual existing cause occurrence');related=member['related'];require(related['tag'] in ('ZoneChange','TokenCreated'),'finite actual source cause family');same(related['tag'],occ['value']['tag'],'actual cause/occurrence family');same(related['after'],occ['value']['after'],'actual complete cause after appearance')
                if related['tag']=='ZoneChange':
                    same(related['before'],occ['value']['before'],'actual complete cause before appearance');before=related['before']['object']['zone']['kind'];after=related['after']['object']['zone']['kind'];semantic='entry' if after=='battlefield' else 'dies' if before=='battlefield' and after=='graveyard' else None;subject=appearance(related['after'] if semantic=='entry' else related['before'])
                else:
                    same(related['creator'],occ['value']['creator'],'actual creating recipient');semantic='entry';subject=appearance(related['after'])
                same(waiting['source']['object']['controller'],waiting['controller'],'actual source captured controller');source=appearance(waiting['source']);controller=mandatory_role(waiting['controller']);matches=[]
                for name,selector in declared.items():
                    if name in trigger_names.values() or source!=reference_alias(selector['source']) or subject!=reference_alias(selector['subject']) or controller!=selector['captured_controller'] or not cause_matches(selector['cause_operation'],event,row):continue
                    if semantic==selector['event']:matches.append(name)
                same(len(matches),1,'unique actual source/controller/complete-cause selector');trigger_names[identity]=matches[0]
            elif tag in ('TriggerPlacementStarted','TriggerPlaced','TriggerTargetSelected','TriggerRemoved'):
                identity=journal['instance'];require(identity in instances,'actual placement discovered instance');same(journal['controller'],instances[identity]['controller'],'actual placement captured controller');entry_name=trigger_names[identity];entry_names[journal['entry']]=entry_name
                if tag=='TriggerPlacementStarted':require(identity not in placements,'single actual placement start');placements[identity]={key:journal[key] for key in ['instance','wave','controller','position','entry']};lifecycle.admit(journal['entry']);lifecycle.placement_names.append(entry_name)
                else:same({key:journal[key] for key in ['instance','wave','controller','position','entry']},placements[identity],'actual stable provisional/installed relation')
                if tag=='TriggerPlaced':require(identity not in placed,'single actual trigger install');placed[identity]=journal
                elif tag=='TriggerTargetSelected':
                    require(identity not in lifecycle.selected_targets,'actual source19 target selected once');lifecycle.selected_targets[identity]=journal['target'];targets[entry_name]=journal['target']['target'];choice=next(c for c in case['player_choices'] if c['trigger']==entry_name);same(journal['target']['target'],dict(tag='Player',player=41 if choice['target']=='P0' else 3),'actual native selected trigger player')
                elif tag=='TriggerRemoved':raise EvidenceError('finite mandatory trigger unexpectedly removed')
            else:raise EvidenceError('source19 forbids canonical rules journal record '+str(tag))
        world=state_world(header);states[ordinal]=world
        previous=header
    for frame in timestamp_frames:
        created=[birth for birth in births.values() if birth['entry']==frame['entry'] and birth['instruction']==frame['instruction']];same(len(created),len(frame['members']),'actual timestamp preview complete birth cohort')
        for member in frame['members']:
            matches=[birth for birth in created if birth['member']==member['ordinal'] and mandatory_role(member['creator'])==birth['creator']];same(len(matches),1,'actual timestamp member/creator birth identity');same(member['definition'],matches[0]['appearance']['object']['origin']['definition'],'actual timestamp preview materialized Program equals created birth')
    same(set(cast_dispositions),set(casts),'actual every cast has source-event disposition');same(lifecycle.placement_names,[name for group in case['trigger_groups'] for name in group['order_bottom_first']],'actual global APNAP and within-controller placement order');same(set(trigger_names.values()),set(declared),'actual complete named native trigger inventory');same(set(placed),set(instances),'every discovered instance actually installed');same(set(casts),{r['id'] for r in case['instructions'] if r['kind'] in ('cast_untargeted','cast_targeted')},'actual complete cast command inventory');same(set(targets),{r['trigger'] for r in case['player_choices']},'actual complete player choices')
    for identity,waiting in instances.items():
        source_name=next(card['name'] for card in manifest['cards'] if card['oracleId']==physical[history[waiting['source']['object']['object']]]['oracle_id']);program=definitions[physical[history[waiting['source']['object']['object']]]['oracle_id']]['program'];same(waiting['body'],program['triggered'][0]['body'],'actual captured complete source body '+source_name)
        actual_entry=placed[identity]['entry'];require(actual_entry in completed,'actual full captured trigger body completion');body=waiting['body'];body_instructions=body['instructions'];adjustments=[]
        for instruction in body_instructions:
            if instruction['op']=='AdjustControllerLife':target=mandatory_role(waiting['controller'])
            elif instruction['op']=='AdjustTargetLife':target=mandatory_role(targets[trigger_names[identity]]['player'])
            else:continue
            value=integer(instruction['adjustment']['amount'],'authored life instruction');adjustments.append((target,value if instruction['adjustment']['tag']=='Gain' else -value))
        actual_life=[value for value in life_events if value['cause'].get('entry')==actual_entry];same([(value['player'],value['new']-value['old']) for value in actual_life],adjustments,'actual native complete ordered life body')
        body_draws=[value for value in draws if value['cause'].get('origin',{}).get('entry')==actual_entry];body_failures=[value for value in failed_draws if value['cause'].get('origin',{}).get('entry')==actual_entry];required_draws=sum(integer(instruction['amount']['value'],'authored draw amount') for instruction in body_instructions if instruction['op']=='DrawController');same(len(body_draws)+sum(value['attempts'] for value in body_failures),required_draws,'actual native exact successful and failed draw attempt denominator')
        completion_event=next(event for event in events.values() if event['cause'].get('entry')==actual_entry and event['cause']['tag']=='ResolutionCompleted')
        for value in [*actual_life,*body_draws,*body_failures]:require(integer(value['event'],'body event')<integer(completion_event['event'],'completion event'),'actual native body event precedes full completion')
        observed_draw_before_life([integer(v['event'],'draw event') for v in [*body_draws,*body_failures]],[integer(v['event'],'life event') for v in actual_life],'actual mandatory native source body')
        first=min([integer(value['event'],'body event') for value in [*actual_life,*body_draws,*body_failures]],default=integer(completion_event['event'],'completion event'))
        require(not any(first<=integer(value['event'],'loss event')<integer(completion_event['event'],'completion event') for value in losses),'actual native body completes before state-based loss')
        require(not any(event['cause']['tag']=='StateBasedActions' and first<=integer(event['event'],'actual state-based event')<integer(completion_event['event'],'completion event') for event in events.values()),'no actual native state-based check inside a mandatory body')
    world=states[len(accepted)];common_final={'accepted_inputs','checkpoint_ref','final_ordinal','policy_complete','policy_entries_consumed','policy_entries_total'}
    if case['stop']['kind']=='bounded_ready':
        same(set(final),common_final|{'active','boundary_ref','holder','kind','position'},'exact bounded native final summary');same(final['kind'],'bounded_prefix','actual bounded completion kind');same(final['boundary_ref'],case['id'],'actual named bounded boundary');same(final['active'],world['header']['active'],'actual bounded active');same(final['holder'],world['header']['priority_holder'],'actual bounded priority holder');same(final['position'],world['header']['turn']['position'],'actual bounded position');same(world['header']['outcome'],None,'actual bounded prefix remains nonterminal');boundary=case['stop']['boundary'];same(world['header']['phase'],dict(kind='priority',phase=dict(kind='ready',holder=41 if boundary['holder']=='P0' else 3)),'actual frozen bounded stop requires Ready phase');same(world['header']['priority_holder'],41 if boundary['holder']=='P0' else 3,'actual frozen bounded holder');same(world['header']['active'],41 if boundary['active']=='P0' else 3,'actual frozen bounded active');same(world['header']['turn']['position']['number'],boundary['turn'],'actual frozen bounded turn');same(world['header']['turn']['position']['step'],boundary['step'].replace('-','_'),'actual frozen bounded step')
    else:
        same(set(final),common_final|{'entropy_position','entropy_remaining','outcome'},'exact natural native final summary');same(final['outcome'],world['header']['outcome'],'actual final outcome equals terminal native header');entropy=world['header']['entropy'];same(final['entropy_position'],None if entropy is None else entropy['position'],'actual final entropy position');same(final['entropy_remaining'],None if entropy is None else entropy['remaining'],'actual final entropy remaining')
    same(final['checkpoint_ref'],len(accepted),'actual final complete checkpoint');same(sorted(world['header']['losses']),sorted(41 if loss['player']=='P0' else 3 for loss in losses),'actual final loser partition equals complete actual loss journal');execution=world['execution'];same(execution['stack'],[],'actual completed empty stack');same(execution['waiting'],[],'actual completed empty waiting');same(execution['resolution'],None,'actual completed no resolution');same(execution['pending_trigger_placement'],None,'actual completed no placement');same(world['header']['pending'],None,'actual completed no decision');same(world['header']['pending_turn_work'],None,'actual completed no forced work')
    for probe in probes.values():
        reference=probe['expectation_ref'];ordinal=probe['attached_candidate_ordinal'];require(type(ordinal) is int and ordinal in states,'actual reached probe source ordinal');same(probe['original_basis'],states[ordinal]['header']['basis'],'actual probe source authority');same(probe['attachment'],'actual-mandatory-reached-state','actual reached-state probe depth')
        if ordinal<len(accepted):same(probe['continuation_ordinal'],ordinal+1,'actual valid immediate continuation');same(accepted[ordinal]['input']['basis'],probe['original_basis'],'actual continuation uses retained valid source')
        else:require(reference.startswith('T09.'),'ordinary decision adversary requires successful continuation');same(probe['continuation_ordinal'],None,'actual terminal bounded mana probe has no invented adopted successor')
        if reference.startswith('T10.'):
            issued=states[ordinal]['header'];attempt=probe['attempted_input'];command=attempt['command'];pending=issued['pending'];same(command['kind'],'reply','actual adversary attempted reply');basis=attempt['basis']
            if reference=='T10.stale-outer-basis':same(basis,dict(issued['basis'],revision=str(integer(issued['basis']['revision'],'actual revision')-1)),'actual stale outer basis only')
            else:same(basis,issued['basis'],'actual adversary otherwise current outer basis')
            if reference=='T10.repeated-consumed-reply':
                same(pending,None,'actual repeated reply has no outstanding frame');require(any(previous['input']['command']==command and previous['input']['actor']==attempt['actor'] for previous in accepted[:ordinal]),'actual repeated command is a previously consumed issued reply')
            else:
                require(pending is not None,'actual adversary has reached issued decision');frame=pending['frame']['frame'];family=pending['frame']['kind'];same(pending['basis'],issued['basis'],'actual adversary issued frame basis')
                if reference.startswith('T10.wrong-actor'):require(attempt['actor'] in issued['participants'] and attempt['actor']!=pending['chooser'],'actual registered wrong chooser only')
                else:same(attempt['actor'],pending['chooser'],'actual otherwise correct chooser')
                if reference=='T10.target-from-another-frame':require(command['decision']!=pending['id'] and any(previous['input']['command']['kind']=='reply' and previous['input']['command']['decision']==command['decision'] and previous['input']['command']['choice']['kind']=='trigger_target' and previous['input']['actor']==attempt['actor'] for previous in accepted[:ordinal]),'actual old target reply belongs to another consumed issued frame')
                else:same(command['decision'],pending['id'],'actual otherwise correct outer decision')
                expected_kind={'triggers':'order_triggers','timestamp_order':'timestamp_order','checks':'order_graveyard','trigger_target':'trigger_target'}[family];choice=command['choice']
                if reference.startswith('T10.wrong-decision-kind'):require(choice['kind']!=expected_kind,'actual wrong typed reply family')
                else:
                    same(choice['kind'],expected_kind,'actual otherwise correct reply family')
                    if 'ordered' in choice:
                        domain=frame['domain'] if family=='triggers' else [member['ordinal'] for member in frame['members']] if family=='timestamp_order' else [dict(alias=history[value['object']],object=value['object']) for value in frame['domain']];ordered=choice['ordered'];encode=lambda value:json.dumps(value,sort_keys=True,separators=(',',':'));available={encode(value) for value in domain};selected=[encode(value) for value in ordered]
                        if 'omitted-order-member' in reference:require(len(ordered)==len(domain)-1 and len(set(selected))==len(selected) and set(selected)<available,'actual complete-domain member omitted')
                        elif 'duplicate-order-member' in reference:require(len(ordered)==len(domain) and len(set(selected))<len(selected) and set(selected)<=available,'actual domain member duplicated')
                        elif reference in ('T10.stale-trigger-instance','T10.foreign-order-member') or 'foreign-order-member.' in reference:
                            require(bool(set(selected)-available),'actual selected member outside complete current domain')
                            if family=='triggers':require(all(value in instances for value in ordered),'actual foreign/stale instance was genuinely observed')
                        else:same(ordered,accepted[ordinal]['input']['command']['choice']['ordered'],'actual otherwise valid complete permutation')
                    elif reference=='T10.creature-target-in-player-frame':
                        target=choice['target'];same(target['kind'],'card','actual nonplayer creature target');card=states[ordinal]['cards'][target['object']['alias']];same(target['object']['object'],card['object'],'actual target current incarnation');same(card['zone'],'battlefield','actual illegal creature is a live permanent');require(card['power'] is not None,'actual illegal target is creature')
                    elif reference=='T10.stack-target-in-player-frame':same(choice['target'],dict(kind='stack',entry=frame['entry']),'actual live provisional stack entry is invalid player target')
            if reference.startswith('T10.wrong-actor'):error=['WrongChooser']
            elif reference.startswith('T10.wrong-decision-kind'):error=['Effect','WrongDecisionKind']
            elif reference=='T10.stale-outer-basis':error=['StaleRevision']
            elif reference=='T10.target-from-another-frame':error=['WrongDecision']
            elif reference=='T10.repeated-consumed-reply':error=['NoDecision']
            elif reference in ('T10.creature-target-in-player-frame','T10.stack-target-in-player-frame'):error=['Effect','IllegalTarget']
            elif reference.endswith('.timestamp'):error=['Effect','IllegalTimestampOrder']
            elif reference.endswith('.graveyard'):error=['Effect','Check','InvalidGraveyardOrder']
            else:require(reference in ('T10.stale-trigger-instance','T10.omitted-order-member','T10.duplicate-order-member','T10.foreign-order-member'),'finite ordinary trigger-order adversary');error=['Effect','Trigger','InvalidTriggerOrder']
        else:
            instruction=instructions[probe['probe_id'].rsplit('@',1)[0]];same(instruction['kind'],'probe_mana','actual named mana probe');same(reference,{'young':'T09.Young','tapped':'T09.Tapped','wrong_actor':'T09.WrongActor','stale_incarnation':'T09.StaleIncarnation'}[instruction['mutation']],'actual independent mana probe family');attempt=probe['attempted_input'];command=attempt['command'];same(attempt['basis'],probe['original_basis'],'actual mana probe current outer authority');same(mandatory_role(attempt['actor']),instruction['point']['actor'],'actual requested mana probe actor');same(command['kind'],'activate_mana','actual attempted printed mana command');same(command['ability'],dict(tag='Printed',id=1),'actual printed Llanowar ability1');source=command['source'];card=states[ordinal]['cards'][source['alias']];same(source['alias'],'P0.llanowar-visionary.1','actual mana probe source');same(card['zone'],'battlefield','actual live source permanent')
            if instruction['mutation']=='stale_incarnation':require(source['object']!=card['object'] and history.get(source['object'])==source['alias'],'actual previously observed stale source incarnation')
            else:same(source['object'],card['object'],'actual current mana probe incarnation')
            if instruction['mutation']=='young':same(card['continuous_control_since_turn_start'],False,'actual young creature control interval');same(card['tapped'],False,'actual young source untapped')
            elif instruction['mutation']=='tapped':same(card['tapped'],True,'actual second tap source already tapped')
            elif instruction['mutation']=='wrong_actor':require(attempt['actor']!=states[ordinal]['header']['priority_holder'],'actual registered wrong priority actor')
            error={'young':['Effect','CreatureTapRestricted'],'tapped':['Effect','Window','SourceTapped'],'wrong_actor':['Effect','Window','Priority','WrongPlayer'],'stale_incarnation':['Effect','UnknownObject']}[instruction['mutation']]
        same(probe['actual_error'],dict(path=error,payload=None),'actual frozen typed probe error independent of declared expectation')
    return dict(final=world,states=states,raw_world_digests=raw_world_digests,journals=journals,events=events,triggers={trigger_names[key]:value for key,value in instances.items()},trigger_entries={trigger_names[key]:value['entry'] for key,value in placed.items()},casts=casts,tokens=births,lineage=lineage,targets=targets,draws=draws,deaths=deaths,life_events=life_events,completed=completed,probes=probes,failed_draws=failed_draws,losses=losses,ceased=ceased,zone_changes=zone_changes,productions=productions,graveyard_choices=graveyard_choices,entry_names=entry_names,history=history,definition_ids=set(definitions),placements=placed,payments=payments,settlements=cast_dispositions)


def join_common_token_births(native:dict[str,Any],reference:dict[str,Any])->None:
    """Join materialized native birth appearances to independently recorded Forge births."""
    same(set(native['tokens']),set(reference),'actual complete token creation-coordinate inventory')
    colors={'White':1,'Blue':2,'Black':4,'Red':8,'Green':16}
    for alias,birth in native['tokens'].items():
        actual=reference[alias];appearance=birth['appearance'];same(appearance['current_names'],[actual['name']],'native/Forge exact materialized token name');same(sum(colors[color] for color in appearance['colors']),actual['color_mask'],'native/Forge actual token color');same(appearance['types'],actual['core_types'],'native/Forge token complete core types');same(appearance['current_supertypes'],actual['supertypes'],'native/Forge token complete supertypes');same(appearance['keywords'],actual['keywords'],'native/Forge token actual keyword inventory');same(appearance['current_power_toughness'],dict(power=actual['state']['power'],toughness=actual['state']['toughness']),'native/Forge actual token PT');same(appearance['battlefield_marked_damage'],actual['state']['marked_damage'],'native/Forge actual stored birth damage');same(appearance['battlefield_counters'],dict(plus_one=actual['state']['plus_one_counters'],minus_one=actual['state']['minus_one_counters']),'native/Forge actual stored birth counters');same(birth['creator'],actual['creator'],'native/Forge actual token creator/owner')


@evidence_boundary
def join_mandatory_common(native:dict[str,Any],reference:dict[str,Any],raw:dict[str,Any],case:dict[str,Any])->dict[str,Any]:
    """Join independently available observations, preserving explicit private/timestamp/cessation limits."""
    same(native['settlements'],reference['settlements'],'actual native/Forge entry dispositions');final=native['final'];forge=reference['final'];same(set(final['cards']),set(forge['cards']),'complete actual native/Forge physical and surviving token inventory')
    for role,player in final['players'].items():
        for field in ['life','mana','library','hand','battlefield','graveyard','exile']:same(player[field],forge['players'][role][field],'native/Forge actual settled '+role+'/'+field)
    common_card_fields=['owner','controller','zone','tapped']
    for alias,card in final['cards'].items():
        for field in common_card_fields:same(card[field],forge['cards'][alias][field],'native/Forge actual '+alias+'/'+field)
        if card['zone']=='battlefield' and card['power'] is not None:
            for field in ['power','toughness','marked_damage','plus_one_counters','minus_one_counters']:same(card[field],forge['cards'][alias][field],'native/Forge complete battlefield creature '+alias+'/'+field)
    same(set(native['casts']),set(reference['casts']),'complete independently bound cast inventory')
    for name,cast in native['casts'].items():
        actual=reference['casts'][name];same(mandatory_role(cast['controller']),actual['payer'],'native/Forge actual cast controller');same(native['history'][cast['spell']['object']['object']],actual['source'],'native/Forge actual cast source');payment=native['payments'][name];same(payment['sources'],actual['paid_sources'],'native/Forge actual complete mana-source funding trace');same(payment['payer'],actual['payer'],'native/Forge actual payer');same(payment['cost'],cost_tuple(actual['required_cost']),'native/Forge actual printed casting cost');same(sum(integer(row['amount'],'actual debit') for row in payment['spend']),len(actual['paid_sources']),'native/Forge actual spent amount')
    same(set(native['triggers']),set(reference['triggers']),'complete actual source/cause-bound trigger inventory')
    for name,trigger in native['triggers'].items():
        actual=reference['triggers'][name];same(native['history'][trigger['source']['object']['object']],actual['actual_ability']['source'],'native/Forge captured trigger source');same(mandatory_role(trigger['controller']),actual['captured_controller'],'native/Forge captured controller');member=trigger['causes']['members'][0]['related'];appearance=member['after'] if member['tag']=='TokenCreated' or member['after']['object']['zone']['kind']=='battlefield' else member['before'];same(native['history'][appearance['object']['object']],actual['subject'],'native/Forge actual trigger subject')
    join_common_token_births(native,reference['tokens'])
    forge_life=[dict(player=row['payload']['player'],old=integer(row['payload']['oldLives'],'Forge old life'),new=integer(row['payload']['newLives'],'Forge new life')) for row in raw['events'] if row['kind']=='GameEventPlayerLivesChanged'];same([{key:row[key] for key in ['player','old','new']} for row in native['life_events']],forge_life,'complete native/Forge chronological life instructions')
    forge_draws={role:[] for role in ['P0','P1']};forge_deaths=[]
    for row in raw['events']:
        if row['kind']!='GameEventCardChangeZone':continue
        payload=row['payload'];before=payload['from'];after=payload['to']
        if before and after and before['zone_type']=='library' and after['zone_type']=='hand':forge_draws[after['player']].append(payload['card']['alias'])
        if before and after and before['zone_type']=='battlefield' and after['zone_type']=='graveyard':forge_deaths.append(payload['card']['alias'])
    for role in ['P0','P1']:same([row['alias'] for row in native['draws'] if row['player']==role],forge_draws[role],'native/Forge every actual ordered successful draw '+role)
    same(sorted(row['alias'] for row in native['deaths']),sorted(forge_deaths),'native/Forge every actual death, independent of missing cessation callback')
    outcome=final['header']['outcome'];actual=raw['actual_outcome']
    if outcome is None:same(actual,None,'native/Forge bounded nonterminal outcome')
    else:
        same(outcome['kind'],'winner','finite actual terminal outcome class');same(mandatory_role(outcome['seat']),actual['winning_seat'],'native/Forge actual natural winner');same({mandatory_role(seat) for seat in final['header']['losses']},{role for role,player in actual['players'].items() if player['lost']},'native/Forge exact natural loser partition')
    position=final['header']['turn']['position'];same(position['number'],raw['final_world']['header']['individual_turn'],'native/Forge final individual turn');same(position['step'],raw['final_world']['header']['step'],'native/Forge final step');same(mandatory_role(final['header']['active']),raw['final_world']['header']['active'],'native/Forge final active')
    return dict(status='ACTUAL_COMMON_OBSERVATIONS_EQUAL',settled_cards=len(final['cards']),casts=len(native['casts']),triggers=len(native['triggers']),token_births=len(native['tokens']),ordered_successful_draws=sum(len(rows) for rows in forge_draws.values()),deaths=len(forge_deaths),chronological_life_events=len(forge_life),unavailable_reference_fields=MANDATORY_UNAVAILABLE,private_native_fields_comparison='Native validation/replay is separate; no reference credit for private waiting, relative identical-token timestamp, complete failed-draw causes or dedicated cessation')


@evidence_boundary
def mandatory_native_primary_facts(native:dict[str,Any],run:dict[str,Any],case:dict[str,Any],primary:dict[str,Any])->dict[str,Any]:
    """Extract independent native arithmetic and private obligations from actual journals and reached states."""
    physical=unique(case['inventory'],'alias','physical source');names={card['oracle_id']:card['name'] for card in run['static_inputs']['definitions']}
    def name(alias:str)->str:return native['tokens'][alias]['appearance']['current_names'][0] if alias.startswith('token:') else names[physical[alias]['oracle_id']]
    def source(trigger:dict[str,Any])->str:return native['history'][trigger['source']['object']['object']]
    def subject(trigger:dict[str,Any])->str:
        related=trigger['causes']['members'][0]['related'];appearance=related['after'] if related['tag']=='TokenCreated' or related['after']['object']['zone']['kind']=='battlefield' else related['before'];return native['history'][appearance['object']['object']]
    def turn(ordinal:int)->int:
        position=native['states'][ordinal]['header']['turn'];return 0 if position is None else position['position']['number']
    def body_entry(cause:dict[str,Any])->str|None:return cause.get('entry') if cause['tag']=='Instruction' else cause.get('origin',{}).get('entry') if cause['tag']=='Draw' else None
    def entry_source(identity:str)->str|None:
        label=native['entry_names'].get(identity)
        if label in native['triggers']:return source(native['triggers'][label])
        if label in native['casts']:return native['history'][native['casts'][label]['spell']['object']['object']]
        return None
    triggers=list(native['triggers'].values());births=list(native['tokens'].values());draws=native['draws'];deaths=native['deaths'];final=native['final'];life={role:integer(player['life'],'actual native final life') for role,player in final['players'].items()};outcome=final['header']['outcome'];results={}
    count_trigger=lambda card:sum(name(source(trigger))==card for trigger in triggers)
    initial_turn=case['initialization'].get('turn');prefix=[draw for draw in draws if draw['cause'].get('origin',{}).get('tag')=='Turn' and turn(draw['ordinal'])==initial_turn]
    for fact in primary['primary_facts']:
        expected=fact['authored_expected']
        if expected is None:continue
        values={};fact_life=life;fact_deaths=deaths;fact_draws=draws
        if case['id'] in ('MT01','MT02'):
            offset=case['id']=='MT02';cut_turn=17+offset if fact['id'].endswith('gain_arithmetic') else next((2*number-1+offset for number in [9,10,11] if fact['id'].endswith('combat'+str(number))),None)
            if cut_turn is not None:
                step='precombat_main' if fact['id'].endswith('gain_arithmetic') else 'postcombat_main';cuts=[state for state in native['states'].values() if state['header']['turn'] and state['header']['turn']['position']['number']==cut_turn and state['header']['turn']['position']['step']==step and not state['execution']['stack'] and state['header']['pending'] is None and state['header']['pending_turn_work'] is None];require(bool(cuts),'actual native primary complete settled cut');fact_life={role:integer(player['life'],'actual cut life') for role,player in cuts[0]['players'].items()};fact_deaths=[death for death in deaths if turn(death['ordinal'])==cut_turn];fact_draws=[draw for draw in draws if turn(draw['ordinal'])==cut_turn]
        for key,value in expected.items():
            if key.endswith('_life'):actual=life[key[:2]]
            elif key in ('GW','BG'):actual=fact_life['P0' if key=='GW' else 'P1']
            elif key in ('deaths','dies'):actual=len(fact_deaths)
            elif key in ('successful_draws','draws'):actual=len(fact_draws)
            elif key=='P1_draws':actual=sum(draw['player']=='P1' for draw in draws)
            elif key=='death_members':actual=sorted(death['alias'] for death in deaths)
            elif key=='artist_instances':actual=count_trigger('Blood Artist')
            elif key=='dissenter_instances':actual=count_trigger('Doomed Dissenter')
            elif key=='entry_instances':actual=sum(trigger['causes']['members'][0]['related']['tag']=='TokenCreated' or trigger['causes']['members'][0]['related']['after']['object']['zone']['kind']=='battlefield' for trigger in triggers)
            elif key in ('warden','orator'):actual=count_trigger('Soul Warden' if key=='warden' else 'Impassioned Orator')
            elif key in ('orator_P0','orator_P1'):actual=sum(name(source(trigger))=='Impassioned Orator' and mandatory_role(trigger['controller'])==key[-2:] for trigger in triggers)
            elif key in ('warden_self','orator_self'):actual=sum(source(trigger)==subject(trigger) and name(source(trigger))==('Soul Warden' if key=='warden_self' else 'Impassioned Orator') for trigger in triggers)
            elif key=='visionary_entry_life_triggers':actual=sum(name(subject(trigger))=='Elvish Visionary' and name(source(trigger)) in ('Soul Warden','Impassioned Orator') for trigger in triggers)
            elif key in ('visionary_draw','occultist_draws'):actual=sum(body_entry(draw['cause']) is not None and entry_source(body_entry(draw['cause'])) is not None and name(entry_source(body_entry(draw['cause'])))==('Elvish Visionary' if key=='visionary_draw' else 'Novice Occultist') for draw in fact_draws)
            elif key in ('opponent_warden','opponent_orator'):actual=sum(name(source(trigger))==('Soul Warden' if key.endswith('warden') else 'Impassioned Orator') and physical[subject(trigger)]['owner']!=mandatory_role(trigger['controller']) for trigger in triggers if subject(trigger) in physical)
            elif key in ('soldier_births','zombie_births'):actual=sum(birth['appearance']['current_names']==['Soldier Token' if key.startswith('soldier') else 'Zombie Token'] for birth in births)
            elif key=='draw_triggers':actual=count_trigger('Elvish Visionary')
            elif key=='library_cards':actual=len(final['players']['P0']['library'])
            elif key=='physical_visionaries':actual=sum(names[row['oracle_id']]=='Elvish Visionary' for row in physical.values())
            elif key=='casts':actual=sum(name(native['history'][cast['spell']['object']['object']])=='Elvish Visionary' for cast in native['casts'].values())
            elif key=='entries':actual=sum(change['after']['zone']['kind']=='battlefield' and name(change['alias'])=='Elvish Visionary' for change in native['zone_changes'])
            elif key=='hand_draw_order':actual=[name(draw['alias']) for draw in draws]
            elif key=='life_after_body':actual=life['P0']
            elif key in ('outcome','terminal'):actual='ready' if outcome is None else 'P0-wins' if mandatory_role(outcome['seat'])=='P0' else 'P0-loses'
            elif key=='required_loss_causes':actual=sorted({'NonpositiveLife':'life-zero','FailedDraw':'empty-library'}[reason] for loss in native['losses'] if loss['player']=='P0' for reason in loss['reasons']);value=sorted(value)
            elif key=='life_event_order':actual=[{field:event[field] for field in ['player','old','new']} for event in native['life_events']]
            elif key=='bears_survive':actual=sum(name(alias)=='Grizzly Bears' and card['zone']=='battlefield' for alias,card in final['cards'].items())
            elif key=='token_cessations':actual=len(native['ceased'])
            elif key=='scheduled_start':actual=run['initialization']['current_facts']['turn_transitions'][0]['to']['step']
            elif key=='first_ready':actual=next(state['header']['turn']['position']['step'] for state in native['states'].values() if state['header']['priority_holder'] is not None and state['header']['pending'] is None and state['header']['pending_turn_work'] is None and not state['execution']['stack'])
            elif key=='prefix_draw':actual=prefix[0]['alias']
            elif key=='successful_prefix_draws':actual=len(prefix)
            elif key=='hand_membership_before_named_operations':
                cut=next(state for state in native['states'].values() if state['header']['turn'] and state['header']['turn']['position']['number']==initial_turn and state['header']['turn']['position']['step']=='precombat_main');actual={role:cut['players'][role]['hand'] for role in ['P0','P1']}
            elif key=='initial_inactive_control_stamp':actual=any(card['continuous_control_since_turn_start'] for card in native['states'][0]['cards'].values() if card['zone']=='battlefield' and card['controller']!=mandatory_role(native['states'][0]['header']['active']))
            elif key=='chooser':actual=mandatory_role(native['graveyard_choices'][0]['choice']['player'])
            elif key=='domain':actual=sorted(row['alias'] for row in native['graveyard_choices'][0]['issued']['frame']['frame']['domain']);value=sorted(value)
            elif key=='final_top_first':actual=final['players']['P1']['graveyard']
            elif key in ('required_owner_order_prompts','owner_order_prompts'):actual=len(native['graveyard_choices'])
            elif key=='P0_physical_entrants':actual=sum(death['alias'] in physical and physical[death['alias']]['owner']=='P0' for death in deaths if turn(death['ordinal'])==5)
            elif key=='P1_token_departures':actual=sum(death['alias'] in native['tokens'] and native['tokens'][death['alias']]['creator']=='P1' for death in deaths if turn(death['ordinal'])==5)
            elif key in ('prelude_artist_instances','final_artist_instances'):actual=sum(name(source(trigger))=='Blood Artist' and ('prelude' in next(row['cause_operation'] for group in case['trigger_groups'] for row in group['triggers'] if row['name']==label))==(key=='prelude_artist_instances') for label,trigger in native['triggers'].items())
            elif key=='final_death_members':actual=sorted('physical:Bears 1' if death['alias'] in physical else 'token:actual-Dissenter-Zombie0' for death in deaths if turn(death['ordinal'])==5);value=sorted(value)
            elif key in ('printed_ability','next_own_turn_G'):
                actual_production=[row['production'] for row in native['productions'] if name(native['history'][row['production']['source']])=='Llanowar Visionary'];actual=sum(integer(row['produced']['Green'],'actual produced green') for row in actual_production) if key=='next_own_turn_G' else 'tap:G' if actual_production and all(row['produced']=={'Green':'1'} for row in actual_production) else None
            elif key in ('new_entry_allowed','second_tap_allowed','fresh_allowed','stale_allowed'):
                suffix='.probe.tapped' if key=='second_tap_allowed' else '.probe.stale' if key=='stale_allowed' else '.probe.fresh';probe=next(probe for probe in native['probes'].values() if probe['probe_id'].rsplit('@',1)[0].endswith(suffix));actual=not(probe['no_candidate'] and probe['state_retained'])
            elif key=='opponent_turn_stamp':
                probe=next(probe for probe in native['probes'].values() if probe['probe_id'].rsplit('@',1)[0].endswith('.probe.opponent-turn'));actual=native['states'][probe['attached_candidate_ordinal']]['cards']['P0.llanowar-visionary.1']['continuous_control_since_turn_start']
            elif key=='old_equals_new_incarnation':actual=native['states'][0]['cards']['P0.llanowar-visionary.1']['object']==final['cards']['P0.llanowar-visionary.1']['object']
            elif key=='winner':actual='GW' if mandatory_role(outcome['seat'])=='P0' else 'BG'
            elif key=='losers':actual=['GW' if mandatory_role(player)=='P0' else 'BG' for player in final['header']['losses']]
            elif key=='cause':actual='life-zero' if any(loss['player']=='P1' and loss['reasons']==['NonpositiveLife'] for loss in native['losses']) else None
            elif key=='individual_turn':actual=final['header']['turn']['position']['number']
            else:raise EvidenceError('unimplemented required independent native primary field '+fact['id']+'/'+key)
            same(actual,value,'primary/native '+case['id']+'/'+fact['id']+'/'+key);values[key]=actual
        results[fact['id']]=values
    return dict(actual_primary_facts=results,depth='actual ordinary Report3 canonical fields/journals; full witness/image/source binding remains a separate prerequisite')


@evidence_boundary
def bound_file(row:dict[str,Any],label:str)->Path:
    """Validate an explicit retained byte receipt without discovering alternate inputs."""
    path=Path(row['path']);require(path.is_absolute() and path.is_file(),label+' actual absolute file');same(sha(path),row['sha256'],label+' actual bytes')
    if 'bytes' in row:same(path.stat().st_size,row['bytes'],label+' actual byte count')
    return path


def equal_api_bytes(before:Path,after:Path)->None:
    """Compare complete typed API bytes in bounded chunks, preserving order and omissions."""
    with before.open('rb') as first,after.open('rb') as second:
        offset=0
        while True:
            a=first.read(1024*1024);b=second.read(1024*1024);require(a==b,'whole predecessor API bytes including field order/name/omission at offset '+str(offset)+' lengths '+str(len(a))+'/'+str(len(b))+' sha256 '+hashlib.sha256(a).hexdigest()+'/'+hashlib.sha256(b).hexdigest())
            if not a:return
            offset+=len(a)


@evidence_boundary
def validate_blue_preservation(document:dict[str,Any],reference_sources:dict[str,str])->dict[str,Any]:
    """Require fresh final-source Blue15 and entire accepted semantic projection equality."""
    same(set(document),{'path','baselinePath','baselineJoin','commands','results','sourceStability'},'exact Blue preservation binding fields');fresh=Path(document['path']);baseline=Path(document['baselinePath']);receipt=bound_file(document['baselineJoin'],'accepted Blue join');same(document['baselineJoin']['sha256'],'c25fea567ac988cb5ca644c80516c0d2a9895b793b29cea5810d16cf0444a145','accepted immutable Blue semantic baseline');accepted=read(receipt);compare_reference(fresh);validate_final_reference_sources(fresh,reference_sources)
    for field,name in [('commands','commands.json'),('results','results.json'),('sourceStability','source-stability.json')]:same(bound_file(document[field],'actual fresh Blue '+field).resolve(),(fresh/name).resolve(),'actual Blue receipt directory')
    for relative,digest in reference_sources.items():same(sha(fresh/'source'/Path(relative).name),digest,'fresh Blue binds final four helper source bytes')
    inputs=read(fresh/'inputs.json');same(inputs,read(baseline/'inputs.json'),'whole preserved Blue input grammar');full=read(fresh/'cases-v1.json');manifest=read(fresh/'manifest.json');cases=unique(inputs['cases'],'case_id','actual Blue case');same(set(cases),set(accepted['cases']),'all15 accepted Blue cases')
    for name,case in cases.items():
        before_path=baseline/'evidence'/f'{name}.json';same(sha(before_path),accepted['cases'][name]['raw_sha256'],'accepted exact Blue raw case');before=forge_projection(read(before_path),case,full,manifest,inputs);after=forge_projection(read(fresh/'evidence'/f'{name}.json'),case,full,manifest,inputs);same(after,before,'entire Blue non-provenance projection '+name)
    return dict(cases=15,entire_projection_equal=True,scope='Every maintained forge_projection field; actual source/runtime/class/resource/process provenance independently checked')


@evidence_boundary
def validate_mandatory_supplemental(supplemental:dict[str,Any],binding:dict[str,Any],requirements:dict[str,Any],captured:dict[str,Any])->dict[str,Any]:
    """Bind actual compact tests at honest depth, separate from each ordinary full witness."""
    same(set(supplemental),{'schemaVersion','kind','boundAt','sourceManifest','buildReceipt','files','tests','mechanismEpoch','predecessorApi','blueReferenceRun'},'exact mandatory supplemental fields');same(supplemental['schemaVersion'],1,'mandatory supplemental schema1');same(supplemental['kind'],'actual-mandatory-trigger-supplemental-test-binding','actual supplemental kind');stamp=datetime.fromisoformat(supplemental['boundAt'].replace('Z','+00:00'));require(stamp.tzinfo is not None and stamp.utcoffset().total_seconds()==0,'actual UTC supplemental binding time');same(supplemental['sourceManifest'],binding['sourceManifest'],'actual supplemental same native source');same(supplemental['buildReceipt'],binding['runtime']['buildReceipt'],'actual supplemental same native checker epoch');source_manifest=read(bound_file(supplemental['sourceManifest'],'actual supplemental source'));build=read(bound_file(supplemental['buildReceipt'],'actual supplemental checks'));checks=unique(build,'name','actual final check');check_dir=Path(supplemental['buildReceipt']['path']).parent;commands=read(check_dir/'commands.json');cwd=Path(commands['cwd']);environment=commands['explicit_environment']
    files=unique(supplemental['files'],'name','actual supplemental named file');same(set(files),{'cases','probes','foreign','synthetic','observer'},'actual exact five separately bound test summaries');payloads={}
    for name,key in [('cases','SPELLWAKE_MANDATORY_CHECK_RECEIPT'),('probes','SPELLWAKE_MANDATORY_PROBE_RECEIPT'),('foreign','SPELLWAKE_MANDATORY_FOREIGN_PROBE_RECEIPT'),('synthetic','SPELLWAKE_MANDATORY_SYNTHETIC_RECEIPT'),('observer','SPELLWAKE_MANDATORY_OBSERVER_RECEIPT')]:path=bound_file(files[name],'actual '+name+' test summary');same(path.resolve(),Path(environment[key]).resolve(),'actual configured summary output');payloads[name]=read(path)
    mechanism=supplemental['mechanismEpoch'];same(set(mechanism),{'buildReceipt','sourceBefore','sourceAfter','referenceBefore','referenceAfter','inputsBefore','inputsAfter'},'exact separate mechanism epoch fields');mechanism_results=read(bound_file(mechanism['buildReceipt'],'actual separate mechanism check results'));mechanism_checks=unique(mechanism_results,'name','actual separate mechanism check');same(len(mechanism_checks),5,'actual five separate exact mechanism executions');mechanism_dir=Path(mechanism['buildReceipt']['path']).parent;mechanism_commands=read(mechanism_dir/'commands.json');same(Path(mechanism_commands['cwd']).resolve(),cwd.resolve(),'actual mechanism/final canonical source root');same(mechanism_commands['commands'],[[row['name'],row['command']] for row in mechanism_results],'actual mechanism executed command plan');helper_dir=cwd/'work/engine-milestones/mandatory-trigger-v1/integration-tools';mechanism_authorities={str(cwd/'.quzzar-system-design/architecture-proposed/work-planning-20261103/planning/mandatory-trigger-v1/contracts/evidence-dispositions.proposed.v1.json'):MANDATORY_PINS['requirements'],**{str(helper_dir/name):MANDATORY_HELPER_HASHES[name] for name in ['run_checks.py','run_mechanisms.py']}};same(mechanism_commands['authorities'],mechanism_authorities,'actual reviewed mechanism helper/disposition authority inventory')
    for path,digest in mechanism_authorities.items():bound_file(dict(path=path,sha256=digest),'actual mechanism helper/disposition source')
    for flag in ['success','source_equal','inputs_equal','reference_equal','test_executables_equal','helpers_equal']:same(read(mechanism_dir/'completion.json')[flag],True,'actual separate mechanism completion '+flag)
    for field in ['sourceBefore','sourceAfter']:same(read(bound_file(mechanism[field],'actual mechanism source guard')),source_manifest,'actual mechanism/final source equality')
    reference_sources=read(check_dir/'reference-before.json')
    for field in ['referenceBefore','referenceAfter']:same(read(bound_file(mechanism[field],'actual mechanism reference guard')),reference_sources,'actual mechanism final helper source epoch')
    for field in ['inputsBefore','inputsAfter']:same(read(bound_file(mechanism[field],'actual mechanism input guard')),read(check_dir/'inputs-before.json'),'actual mechanism final input epoch')
    tests=unique(supplemental['tests'],'name','actual source-bound test');required_tests={'cases','probes','foreign','synthetic','observer','historical-api','teaching-interaction-api'}|{row['id'] for row in requirements['synthetic_mechanisms'][1:]};same(set(tests),required_tests,'every actual required separately bound test selector')
    for name,test in tests.items():
        same(set(test),{'name','epoch','check','command','selector','source','log','testBinary'},'exact source-bound test binding fields');separate=name.startswith('T09.M.');same(test['epoch'],'mechanism' if separate else 'final','actual explicit supplemental test epoch');check=(mechanism_checks if separate else checks)[test['check']];same(check['exitCode'],0,'actual separate test passed');validate_actual_check_row(check,cwd);same(test['command'],check['command'],'actual supplemental test/check command');source=bound_file(test['source'],'actual test assertion source');require(source.is_relative_to(cwd),'actual test source belongs to checked native root');same(test['source']['sha256'],source_manifest[str(source.relative_to(cwd))],'actual checked test assertion source');same(bound_file(test['log'],'actual passed test log').resolve(),Path(check['log']).resolve(),'actual selected checker test log');binary=bound_file(test['testBinary'],'actual test executable');require(any(row==test['testBinary'] for row in check['testExecutables']),'actual test executable observed at process completion');require('test '+test['selector']+' ... ok' in Path(check['log']).read_text(),'actual named native test passed '+test['selector'])
        if name in {'cases','probes','foreign','synthetic','observer','historical-api','teaching-interaction-api'}:
            expected_source={'cases':'crates/reference_tools/tests/mandatory_games.rs','probes':'crates/reference_tools/tests/mandatory_games.rs','synthetic':'crates/reference_tools/tests/mandatory_games.rs','foreign':'crates/reference_tools/src/ordinary_game/mandatory.rs','observer':'crates/reference_tools/src/ordinary_game/record/observer_tests.rs','historical-api':'crates/reference_tools/tests/predecessor_api.rs','teaching-interaction-api':'crates/reference_tools/tests/predecessor_api.rs'}[name];same(source.resolve(),(cwd/expected_source).resolve(),'actual owned test assertion path');expected={'cases':'mandatory-full-tests','probes':'mandatory-probe-tests','foreign':'mandatory-foreign-tests','synthetic':'mandatory-synthetic-tests','observer':'mandatory-observer-tests','historical-api':'historical-api-tests','teaching-interaction-api':'teaching-interaction-api-tests'}[name];same(test['check'],expected,'actual finite supplemental test route')
        else:
            definition=next(row for row in requirements['synthetic_mechanisms'] if row['id']==name);same(source.resolve(),(cwd/definition['source']).resolve(),'actual frozen mechanism test source');same(test['command'],definition['command'],'actual exact separately executed mechanism command');selector=definition['command'][definition['command'].index('--test')+2];require(test['selector']==selector or test['selector'].endswith('::'+selector),'exact retained synthetic mechanism selector')
    cases=payloads['cases'];same(cases['kind'],'actual-source-bound-mandatory-game-tests','actual ordinary test summary kind');rows=unique(cases['cases'],'case','actual summary case');same(set(rows),set(captured),'actual all23 summary/capture cases');same(cases['bounded'],17,'actual17 bounded tests');same(cases['natural'],6,'actual6 natural tests')
    for name,row in rows.items():
        for field in ['accepted','rerun','final']:same(row[field],captured[name][field],'actual test summary/final witness '+name+'/'+field)
    probe=payloads['probes'];same(probe['kind'],'actual-ordinary-recorded-replayed-mandatory-probes','actual probe summary depth');required={row['id'] for row in requirements['adversaries']+requirements['additional_frame_variants']};ordinary=required-{'T10.foreign-trigger-instance'};same(set(probe['probe_ids']),ordinary,'actual21 ordinary probe families');same(len(probe['probe_ids']),21,'actual no duplicate probe summary families');same(probe['separate_test_only_family'],'T10.foreign-trigger-instance','separate actual foreign root test')
    actual_probes={probe['expectation_ref'] for case in captured.values() for probe in case['probes']};require(ordinary<=actual_probes,'all21 ordinary probe families present in actual final captures')
    foreign=payloads['foreign'];same(foreign['kind'],'actual-test-only-mandatory-foreign-instance-root-probe','actual foreign handle test depth');same(foreign['probe_id'],'T10.foreign-trigger-instance','actual separate family');same(foreign['target_case'],'T02.own-main','actual target case');same(foreign['observed_foreign_case'],'T01.own-then-opponent','actual foreign observed case');require(foreign['foreign_instance'] not in foreign['actual_target_domain'],'actual foreign handle absent from complete current domain')
    for flag in ['no_candidate','whole_source_retained','valid_continuation','saved_probe_rerun','final_state_equal']:same(foreign[flag],True,'actual foreign test '+flag)
    for flag in ['ordinary_report_credit','full_witness_credit','forge_credit']:same(foreign[flag],False,'foreign test honest '+flag)
    synthetic=payloads['synthetic'];same(synthetic['kind'],'actual-source-Llanowar-plus-synthetic-control-effect','separate synthetic mechanism');same(synthetic['selector'],'mandatory_llanowar_control_change_is_a_separate_synthetic_mechanism','actual synthetic selector')
    for flag in ['same_object','fresh_restricted','old_controller_rejected']:same(synthetic[flag],True,'actual synthetic '+flag)
    same(synthetic['next_controller_turn_mana'],{'Green':'1'},'actual synthetic next-controller-turn output');same(synthetic['registry7_ordinary_credit'],False,'no synthetic Registry credit');same(synthetic['forge_credit'],False,'no synthetic Forge credit');observer=validate_token_observer(payloads['observer'])
    preservation=supplemental['predecessorApi'];baseline=bound_file(preservation['baseline'],'actual B03 immutable baseline');same(preservation['baseline']['sha256'],PREDECESSOR_BASELINE_SHA,'hard pinned actual pre-edit B03 binder');baseline_data=read(baseline);same(baseline_data['case_count'],21,'actual B03 full21 cases');pairs=unique(preservation['files'],'name','predecessor API group');same(set(pairs),set(PREDECESSOR_API_PINS),'exact three whole API groups')
    for name,pair in pairs.items():
        before=bound_file(pair['before'],'actual pre-edit '+name);after=bound_file(pair['after'],'actual final '+name);same(pair['before']['sha256'],PREDECESSOR_API_PINS[name],'hard pinned pre-edit bytes');same(before.parent.resolve(),Path(environment['SPELLWAKE_PREDECESSOR_API_BASELINE_DIR']).resolve(),'actual configured baseline bytes');same(after.parent.resolve(),Path(environment['SPELLWAKE_PREDECESSOR_API_OBSERVED_DIR']).resolve(),'actual configured final bytes');equal_api_bytes(before,after)
    blue=validate_blue_preservation(supplemental['blueReferenceRun'],read(check_dir/'reference-before.json'))
    return dict(status='ACTUAL_SEPARATELY_BOUND_NATIVE_TESTS',bound_at=supplemental['boundAt'],depth='Actual checked test source/executable/log and compact summaries; only ordinary23 use current-envelope full witness',ordinary_probe_families=21,separate_foreign_families=1,synthetic_mechanisms=6,observer=observer,predecessor_api_cases=21,blue_reference=blue)


@evidence_boundary
def compare_mandatory_native_files(run_dir:Path,native_paths:list[Path],native_binding:dict[str,Any],supplemental:dict[str,Any],authority:dict[str,Any],selected:list[str]|None=None)->dict[str,Any]:
    """Consume exact final23 full witnesses, one file at a time, and every required separate native gate."""
    indexed=validate_image_binding(native_binding,PROFILES[1],authority['requirements']);compiled=authority['data'];cases=unique(compiled['cases'],'id','frozen native case');same(list(indexed),list(cases),'actual final exact23 ordered native cases');require(selected is None or selected==list(cases),'final mandatory joins require complete exact23; no partial adoption');paths=native_paths or [Path(row['path']) for row in indexed.values()];same({str(path.resolve()) for path in paths},{str(Path(row['path']).resolve()) for row in indexed.values()},'actual complete indexed capture file set');same(len(paths),23,'actual exactly one full witness per case');inputs=read(run_dir/'inputs.json');forge_cases=unique(inputs['cases'],'case_id','actual Forge recipe');manifest=read(Path(authority['authorities']['content']['path']));vocabulary=read(Path(authority['authorities']['vocabulary']['path']));primary={row['id']:row for row in authority['payloads']['full-games.json']['cases']+authority['payloads']['reduced-cases.json']['variants']};reports={};captured={};definition_ids=set();reference_sources=read(Path(native_binding['runtime']['buildReceipt']['path']).parent/'reference-before.json')
    validate_final_reference_sources(run_dir,reference_sources)
    for relative,digest in reference_sources.items():same(sha(Path(__file__).resolve().parents[1]/relative),digest,'actual executing maintained reference source epoch')
    path_cases={str(Path(row['path']).resolve()):name for name,row in indexed.items()}
    for path in paths:
        document=read_mandatory_capture(path,manifest,path_cases[str(path.resolve())]);report=document.report;provenance=document.provenance;same(len(report['runs']),1,'one actual full case per captured file');run=report['runs'][0];name=run['fixture_ref'];require(name in cases and name not in reports,'actual unique known final case');same(str(path.resolve()),str(Path(indexed[name]['path']).resolve()),'actual indexed current witness path');validate_captured_sources(document,native_binding,indexed[name]);native=mandatory_native_projection(run,cases[name],manifest,vocabulary);definition_ids|=native['definition_ids'];raw=read(run_dir/'evidence'/f'{name}.json');reference=mandatory_forge_projection(raw,forge_cases[name],inputs);mandatory_primary_facts(raw,forge_cases[name],reference,primary[name]);facts=mandatory_native_primary_facts(native,run,cases[name],primary[name]);joined=join_mandatory_common(native,reference,raw,cases[name]);reports[name]=dict(join=joined,native_primary=facts,captured=True,native_sha256=sha(path));captured[name]=dict(accepted=len(run['accepted']),rerun=run['rerun'],final=run['final'],probes=list(native['probes'].values()));del document,report,run,native,raw,reference
    same(set(reports),set(cases),'complete final23 joined case inventory');same(definition_ids,{row['oracle_id'] for row in compiled['definitions']},'actual all23 physically used definition union19');extra=validate_mandatory_supplemental(supplemental,native_binding,authority['requirements'],captured)
    return dict(status='ACTUAL_FINAL_IMAGE_MANDATORY_JOIN',final_image_adopted=True,case_count=23,cases=reports,supplemental=extra,source_bound_registry=7,all_source_bindings_partial=True,globally_fully_qualified_cards=0,reference_unavailable=MANDATORY_UNAVAILABLE,processing='one actual full per-case witness at a time; no whole-packet deep copies')


# PRIVATE SOURCE-ONLY OPTIONAL/PAYMENT PREPARATION.
# These adapters are not wired into the CLI until actual reduced native/Forge
# packets bind the outstanding streaming, source, lifecycle and common joins.
OPTIONAL_PROFILE='optional-payment-v1'
OPTIONAL_EPOCHS={'registry':8,'report':4,'aggregate':1,'current':10,'witness':8,'record_codec':2,'vocabulary':1}
OPTIONAL_CASES=('OP-FG01','OP-FG02',*(f'OP-R{index:02}' for index in range(1,25)))
OPTIONAL_REPLAY_FIELDS=[*MANDATORY_REPLAY_FIELDS[:7],
    'canonical_execution_stack_resolution_waiting_and_pending_placement',
    'complete_pending_work_and_suspended_payment_parent','complete_spell_copy_objects',
    'complete_token_observations_and_endpoint_deltas',
    'canonical_rules_journal_with_optional_and_payment_controls',
    'complete_native_rules_journals','full_final_native_state']


@evidence_boundary
def optional_report_ingress(document:dict[str,Any],expected_case:str|None=None)->dict[str,Any]:
    """Reject failed or incomplete real packets before source/witness success claims.

    This finite ingress is only the entry guard. Its return value is not a
    successful comparison, complete witness validation or image/source authority.
    """
    same(set(document),{'observations','profile','schema_version','type_context_witness'},'exact optional current10 envelope')
    same(document['profile'],'optional-payment','explicit optional current profile')
    same(document['schema_version'],10,'explicit optional current10')
    witness=document['type_context_witness']
    same(set(witness),{'calls','coverage','registry','schema','snapshots','source_inputs','vocabulary'},'exact optional witness8 fields')
    same(witness['schema'],'spellwake.current-type-context-witness.v8','explicit optional witness8')
    api=document['observations'];same(set(api),{'status','data'},'exact optional API response')
    same(api['status'],'success','actual optional API success required before baseline')
    report=api['data'];same(set(report),{'schema_version','profile','registry_version','catalogue_sha256','runs'},'exact optional aggregate fields')
    same(report['schema_version'],1,'explicit optional aggregate schema1')
    same(report['profile'],OPTIONAL_PROFILE,'explicit optional aggregate profile')
    same(report['registry_version'],8,'explicit optional Registry8')
    same(len(report['runs']),1,'exact one actual case per optional packet')
    run=report['runs'][0]
    same(set(run),{'accepted','checkpoints','completed','failure','final','fixture_ref','initialization','observations','rejection_probes','rerun','schema_version','source_build_ref','static_inputs'},'exact optional Report4 fields')
    same(run['schema_version'],4,'explicit optional Report4')
    require(run['fixture_ref'] in OPTIONAL_CASES,'actual optional case26 member')
    if expected_case is not None:same(run['fixture_ref'],expected_case,'actual expected optional case')
    same(run['completed'],True,'actual optional case completed before baseline')
    same(run['failure'],None,'actual optional case has no failure before baseline')
    for field in ('initialization','final','rerun','static_inputs'):
        require(run[field] is not None,'actual optional required '+field+' before baseline')
    return report


def optional_raw_source_join(compiled:dict[str,Any],selected:dict[str,Any],reference:dict[str,Any],*,metadata:dict[str,Any]|None=None,source_receipt:dict[str,Any]|None=None,primary:dict[str,Any]|None=None)->None:
    """Bind all complete saved source clauses and printing fields without rewriting them."""
    if source_receipt is not None:same(selected['source'],source_receipt,'actual exact compressed source receipt')
    if metadata is not None:same(selected['metadata'],metadata,'actual complete pinned source metadata')
    primary_rows=None if primary is None else unique(primary['rows'],'oracle_id','independent complete raw source19')
    sources=unique(selected['rows'],'oracle_id','optional selected raw source19')
    references=unique(reference['cards'],'oracle_id','optional selected Forge source19')
    definitions=unique(compiled['definitions'],'oracleId','optional compiled source19')
    same(len(sources),19,'exact optional raw source19')
    same(set(references),set(sources),'exact optional Forge source19 identities')
    same(set(definitions),set(sources),'exact optional compiled source19 identities')
    same(reference['forge_commit'],PIN,'exact selected Forge source pin')
    for oracle,source in sources.items():
        actual=definitions[oracle];forge=references[oracle];printing=source['printing']
        if primary_rows is not None:same(source['whole_raw_record'],primary_rows[oracle]['oracle_record_complete'],'actual complete raw Oracle record/independent source authority')
        same((actual['name'],actual['scryfallId'],actual['sourceLineOneBased'],actual['rawLineSha256IncludingLf']),
            (source['name'],printing['id'],source['source_line'],source['raw_line_sha256_including_lf']),
            'actual complete optional compiled/raw identity join')
        same((actual['set'],actual['collector'],actual['language']),
            (printing['set'],printing['collector_number'],printing['lang']),'actual exact optional printing join')
        same(forge['printing'],printing,'actual full Forge/saved printing identity')
        same(forge['whole_saved_faces'],source['materialized_faces_for_review_not_native_signature'],'actual complete saved source faces')
        same(forge['full_saved_clauses'],source['full_oracle_paragraphs'],'actual complete saved clauses and paragraph order')
        same(forge['saved_legacy'],source['complete_saved_legalities']['legacy'],'actual dated saved Legacy status')


def optional_source_join(compiled:dict[str,Any],recipes:dict[str,Any])->dict[str,Any]:
    """Join frozen input identities, not a regenerated Program or native signature.

    File byte/hash authority is supplied by the caller's existing bound_file/source
    owners. Native admission remains authoritative for semantic signatures.
    """
    same(compiled['schema_version'],1,'optional compiled input schema1')
    same(recipes['schema_version'],1,'optional recipe schema1')
    for document in (compiled,recipes):
        same(document['profile'],OPTIONAL_PROFILE,'exact optional source profile')
        same(document['epochs'],OPTIONAL_EPOCHS,'exact optional source epoch pairing')
    source=unique(compiled['definitions'],'oracleId','optional compiled source19')
    recipe=unique(recipes['definitions'],'oracle_id','optional recipe source19')
    same(len(source),19,'exact optional source19')
    same(set(recipe),set(source),'exact optional source19 recipe join')
    for oracle,row in source.items():
        signature=row['registrySignatureV1Sha256']
        require(isinstance(signature,str) and re.fullmatch(r'[0-9a-f]{64}',signature) is not None,'actual native C03 signature required')
        other=recipe[oracle]
        same(other,dict(name=row['name'],oracle_id=oracle,printing_id=row['scryfallId'],
            source_line=row['sourceLineOneBased'],raw_line_sha256=row['rawLineSha256IncludingLf'],
            semantic_sha256=signature),'exact optional identity/printing/raw/native signature join')
    cases=unique(recipes['cases'],'id','optional recipe case26')
    same(tuple(cases),OPTIONAL_CASES,'exact ordered optional case26')
    same(recipes['seats'],[dict(role='P0',ordinary_role='P0',native_seat=41),dict(role='P1',ordinary_role='P1',native_seat=3)],'exact optional participant mapping')
    for case in cases.values():
        inventory=unique(case['inventory'],'alias','optional actual physical inventory')
        require(bool(inventory),'optional nonempty physical inventory')
        require(all(row['oracle_id'] in source for row in inventory.values()),'optional inventory only source19')
    return dict(cases=cases,definitions=source)


class OptionalWire(MandatoryWire):
    CONTEXT_PREFIX="optional-context-v1-"
    """Extend the maintained wire owner only for source-owned Report4 differences."""
    def reply(self,value:dict[str,Any],weak:bool)->dict[str,Any]:
        kind=value['kind']
        if kind=='optional_effect':
            same(set(value),{'kind','choice'},'exact optional reply fields')
            require(value['choice'] in ('take','decline'),'closed optional reply choice')
            return dict(tag='OptionalEffect',choice={'take':'Take','decline':'Decline'}[value['choice']])
        if kind=='resolution_payment':
            same(set(value),{'kind','choice'},'exact payment reply fields')
            choice=value['choice'];tag=choice['kind']
            if tag=='decline':
                same(set(choice),{'kind'},'exact declined payment reply')
                reply=dict(tag='Decline')
            else:
                same(tag,'pay','closed payment reply choice')
                same(set(choice),{'kind','payment'},'exact Pay allocation only')
                reply=dict(tag='Pay',payment=dict(spend=self.spend(choice['payment'])))
            return dict(tag='ResolutionPayment',reply=reply)
        if kind=='mana_color':
            same(set(value),{'kind','mana_type'},'exact color reply')
            require(value['mana_type'] in _NATIVE_MANA,'closed actual mana color')
            return dict(tag='ManaColor',color=_NATIVE_MANA[value['mana_type']])
        if kind=='copy_targets':
            same(set(value),{'kind','choice'},'exact copy target reply')
            choice=value['choice'];tag=choice['kind']
            if tag=='retain':
                same(set(choice),{'kind'},'exact retained copy target reply')
                reply=dict(tag='Retain')
            else:
                same(tag,'change','closed actual copy choice')
                same(set(choice),{'kind','target'},'exact changed copy target reply')
                reply=dict(tag='Change',target=self.target(choice['target'],weak))
            return dict(tag='CopyTargets',choice=reply)
        return super().reply(value,weak)

    def decision(self,value:dict[str,Any]|None)->dict[str,Any]|None:
        if value is None:return None
        encoded=value['frame'];kind=encoded['kind']
        tags={'optional_effect':'OptionalEffect','resolution_payment':'ResolutionPayment','mana_color':'ManaColor','copy_targets':'CopyTargets'}
        if kind not in tags:return super().decision(value)
        same(set(value),{'id','basis','chooser','frame'},'exact Report4 pending decision')
        same(set(encoded),{'kind','frame'},'exact Report4 frame envelope')
        frame=encoded['frame'];same(frame['tag'],tags[kind],'actual canonical Report4 frame tag')
        fields={'OptionalEffect':'revision entry instruction chooser options',
            'ResolutionPayment':'revision entry instruction payer cost',
            'ManaColor':'revision source ability chooser amount offered',
            'CopyTargets':'entry original instruction chooser'}
        same(set(frame),{'tag'}|set(fields[frame['tag']].split()),'exact canonical Report4 frame fields')
        same(value['chooser'],frame['payer'] if kind=='resolution_payment' else frame['chooser'],'root/frame actual chooser relation')
        return dict(id=value['id'],basis=self.basis(value['basis']),chooser=value['chooser'],frame=frame)


def optional_native_state_signature(state:dict[str,Any])->dict[str,Any]:
    """Reuse every existing state/world check; retain all Report4 owner additions."""
    same(set(state),_NATIVE_STATE_FIELDS|{'pending_work','resolving'},'exact current10 complete state fields')
    predecessor={key:value for key,value in state.items() if key not in ('pending_work','resolving')}
    signature=native_state_signature(predecessor)
    validate_optional_pending(state)
    validate_optional_copy_objects(state)
    return dict(signature,pending_work=state['pending_work'],copies=state['copies'],resolving=state['resolving'])


def validate_optional_copy_objects(state:dict[str,Any])->None:
    copies=unique(state['copies'],'entry','actual complete spell-copy object inventory')
    stack=unique(state['stack'],'entry','actual copy stack payload inventory')
    for entry,copy in copies.items():
        same(set(copy),{'entry','owner','controller','zone'},'actual complete spell-copy object fields')
        require(copy['owner'] in state['participants'],'actual copy owner is registered participant')
        if copy['zone']=='Stack':
            require(entry in stack and stack[entry]['kind']['tag']=='SpellCopy','actual on-stack copy object has owned copy payload')
            same((copy['owner'],copy['controller']),(stack[entry]['kind']['owner'],stack[entry]['controller']),'actual stack-copy/native copy object owner and controller')
        else:
            require(entry not in stack and copy['controller'] is None,'actual departed copy awaits cessation outside stack without controller')
    same({entry for entry,row in stack.items() if row['kind']['tag']=='SpellCopy'},{entry for entry,row in copies.items() if row['zone']=='Stack'},'actual on-stack copy payload/object bijection')


def optional_ordinary_state_signature(wire:OptionalWire,header:dict[str,Any],world_digest:str)->dict[str,Any]:
    """Compare actual Header fields with canonical owners through the existing adapter."""
    execution=header['execution']
    same(set(execution),{'stack','resolution','waiting','pending_trigger_placement','pending_work','copies','resolving'},'exact Report4 execution fields')
    return dict(ordinary_state_signature(wire,header,world_digest),
        pending_work=execution['pending_work'],copies=execution['copies'],resolving=execution['resolving'])


def validate_optional_pending(state:dict[str,Any])->None:
    """Validate actual continuation/frame relations without executing instructions."""
    decision=state['decision']
    validate_optional_owned_work(state,None if decision is None else decision['frame'])


def validate_optional_owned_work(state:dict[str,Any],frame:dict[str,Any]|None,*,require_frame:bool=True)->None:
    """Share actual EffectWindow work checks without inventing root decision state.

    Root GameState callers require their issued frame. A native-only EffectWindow
    observation has no root decision; its work/cursor/source checks run directly,
    and an accepted ResumeRule call separately supplies its real input frame.
    """
    pending=state['pending_work']
    if pending is None:
        require(frame is None or frame['tag'] not in ('OptionalEffect','ResolutionPayment','ManaColor'),'issued Report4 frame requires actual pending owner')
        return
    tag=pending['tag']
    if tag=='ManaColor':
        same(set(pending),{'tag','frame','parent'},'exact owned color work')
        if not require_frame and frame is None:frame=pending['frame']
        require(frame is not None,'actual color owner has issued frame')
        same(pending['frame'],frame,'actual color decision/owned frame relation')
        same(frame['revision'],state['effect_revision'],'actual color frame/executor revision')
        parent=pending['parent']
        if parent['tag']=='Priority':
            same(set(parent),{'tag'},'exact priority color parent')
            return
        same(parent['tag'],'Payment','closed color parent')
        same(set(parent),{'tag','work'},'complete suspended payment parent')
        work=parent['work'];kind='ResolutionPayment'
    elif tag in ('OptionalEffect','ResolutionPayment'):
        same(set(pending),{'tag','work'},'exact owned resolution choice work')
        work=pending['work'];kind=tag
        if require_frame:require(frame is not None,'actual resolution choice owner has issued frame')
        if frame is not None:same(frame['tag'],tag,'actual choice decision/owned process relation')
    else:
        # Remaining process grammar remains with the shared canonical owners.
        require(frame is None or frame['tag'] not in ('OptionalEffect','ResolutionPayment','ManaColor'),'wrong actual pending process for Report4 decision')
        return
    fields={'OptionalEffect':{'cursor','chooser','options'},'ResolutionPayment':{'cursor','payer','cost','paid_destination','declined_destination'}}
    same(set(work),fields[kind],'complete owned optional/payment continuation fields')
    cursor=work['cursor'];same(set(cursor),{'entry','instruction','locals'},'complete owned cursor fields')
    same(cursor,state['resolution_state'],'actual owned cursor/current resolution relation')
    require(state['resolving'] is not None,'actual payment/optional work has resolving payload')
    same(cursor['entry'],state['resolving']['entry'],'actual continuation/current resolving entry')
    if tag!='ManaColor' and frame is not None:
        same((frame['entry'],frame['instruction']),(cursor['entry'],cursor['instruction']),'actual issued frame/cursor origin')
        same(frame['revision'],state['effect_revision'],'actual fresh frame/executor revision')
        for field in ('chooser','options') if kind=='OptionalEffect' else ('payer','cost'):
            same(frame[field],work[field],'actual issued frame/owned '+field)


def validate_optional_journal(records:list[dict[str,Any]],*,additional_occurrence_fields:dict[str,str]|None=None)->None:
    """Deepen the single closed journal grammar with actual current10 controls."""
    extra={'OptionalEffectOutcome':'entry instruction chooser outcome next',
        'ResolutionPaymentOutcome':'entry instruction payer cost outcome next',
        'InstructionBranch':'entry instruction condition left right result next',
        'InstructionJump':'entry instruction next'}
    require(isinstance(records,list),'required optional canonical journal array')
    for row in records:
        require('tag' in row,'actual control/journal record requires explicit kind outside triggerable occurrences')
        tag=row['tag']
        if tag not in extra:
            validate_mandatory_journal([row],additional_occurrence_fields=additional_occurrence_fields)
            continue
        same(set(row),{'tag'}|set(extra[tag].split()),'exact current10 canonical control fields')
        if tag=='OptionalEffectOutcome':require(row['outcome'] in ('Take','Decline'),'closed optional outcome')
        if tag=='ResolutionPaymentOutcome':
            outcome=row['outcome'];kind=outcome['tag']
            same(set(outcome),{'tag','payment'} if kind=='Paid' else {'tag'},'exact payment outcome payload')
            require(kind in ('Paid','Declined'),'closed payment outcome')
            if kind=='Paid':
                payment=outcome['payment'];same(set(payment),{'entry','payer','cost','spend'},'same existing normalized native payment record')
                for field,origin in [('entry','entry'),('payer','payer'),('cost','cost')]:same(payment[field],row[origin],'actual Paid record/offer '+field)


def optional_control_join(before:dict[str,Any],call:dict[str,Any],*,journal_validator:Callable[[list[dict[str,Any]]],None]=validate_optional_journal)->list[dict[str,Any]]:
    """Join returned controls to the real issued input and real debit publication.

    Branch destinations and normalized records are read from actual owned inputs,
    never predicted by another evaluator. Full source/body and canonical call/state
    equality remain the streaming witness owner's required responsibility.
    """
    candidate=call['result']['candidate'];journal=candidate['append']
    journal_validator(journal)
    controls=[row for row in journal if row['tag'] in ('OptionalEffectOutcome','ResolutionPaymentOutcome')]
    frame=None if before['decision'] is None else before['decision']['frame']
    command=call['input']['value']['command']
    if frame is None or frame['tag'] not in ('OptionalEffect','ResolutionPayment') or command['tag']!='Reply':
        same(controls,[],'no invented optional/payment outcome without its actual reply')
        return controls
    same(len(controls),1,'one outcome per actual optional/payment reply')
    row=controls[0];same((row['entry'],row['instruction']),(frame['entry'],frame['instruction']),'actual control/issued offer origin')
    same(command['decision'],before['decision']['id'],'actual control reply/current DecisionId')
    same(call['input']['value']['basis'],before['basis'],'actual control reply/current complete GameBasis')
    actor=call['input']['value']['actor'];same(actor,before['decision']['chooser'],'actual control reply/root chooser')
    choice=command['choice']
    debit=[item['value'] for event in journal if event['tag']=='Event' for item in event['occurrences'] if item['value']['tag']=='ManaDebited']
    if frame['tag']=='OptionalEffect':
        same(row['tag'],'OptionalEffectOutcome','actual optional reply control kind')
        same(row['chooser'],actor,'actual optional control actor')
        same(choice,dict(tag='OptionalEffect',choice=row['outcome']),'actual optional reply/outcome')
        require(row['outcome'] in frame['options'],'actual outcome was admitted by issued domain')
        # OptionalEffect has a structural one-instruction body in the native
        # Program grammar. This binds its recorded edge, without evaluating it.
        same(row['next'],str(integer(frame['instruction'],'actual optional instruction')+(1 if row['outcome']=='Take' else 2)),'actual optional control structural successor')
    else:
        same(row['tag'],'ResolutionPaymentOutcome','actual payment reply control kind')
        same(row['payer'],actor,'actual payment control actor')
        same(row['cost'],frame['cost'],'actual payment quoted full cost')
        same(choice['tag'],'ResolutionPayment','actual payment reply kind')
        work=before['pending_work']['work'];outcome=row['outcome']
        same(candidate['mana_payment'],None,'resolution payment never fabricates cast/activation fact')
        if outcome['tag']=='Declined':
            same(choice['reply'],dict(tag='Decline'),'actual Declined reply relation')
            same(debit,[],'Declined emits no debit')
            same(row['next'],work['declined_destination'],'actual Declined owned successor')
            require('mana' in candidate['state'],'actual Declined requires complete returned mana pools')
            same(candidate['state']['mana'],before['mana'],'actual Declined preserves complete accepted mana prefix without debit')
        else:
            same(choice['reply']['tag'],'Pay','actual Paid reply relation')
            payment=outcome['payment'];same(row['next'],work['paid_destination'],'actual Paid owned successor')
            supplied=choice['reply']['payment']['spend'];order=list(_NATIVE_MANA.values())
            same(len({item['type'] for item in supplied}),len(supplied),'actual saved Pay allocation unique mana types')
            expected=sorted([item for item in supplied if integer(item['amount'],'actual saved Pay amount')>0],key=lambda item:order.index(item['type']))
            same(payment['spend'],expected,'actual normalized Paid record/saved Pay allocation')
            for item in debit:
                same(item['payment'],payment,'same actual debit/normalized Paid record')
                same(integer(item['before'],'actual debit pool before')-integer(item['after'],'actual debit pool after'),integer(item['amount'],'actual debit amount'),'actual native debit pool delta')
            same([(item['type'],item['amount']) for item in debit],[(item['type'],item['amount']) for item in payment['spend'] if integer(item['amount'],'actual normalized spend')>0],'actual exact ordered debit/normalized spend relation including explicit zero Paid')
    return controls


def optional_color_reply_join(before:dict[str,Any],call:dict[str,Any])->dict[str,Any]|None:
    """Bind the real color child's returned parent and production publication."""
    pending=before['pending_work'];command=call['input']['value']['command']
    if pending is None or pending['tag']!='ManaColor' or pending['parent']['tag']!='Payment' or command['tag']!='Reply':return None
    same(command['choice']['tag'],'ManaColor','actual color child reply kind')
    decision=before['decision'];frame=decision['frame'];candidate=call['result']['candidate'];after=candidate['state']
    same(after['window_phase'],before['window_phase'],'no actual ordinary priority cut during color child')
    require('checks' in candidate,'actual color child requires complete no-check proof')
    same(command['decision'],decision['id'],'actual color reply/current DecisionId')
    same(call['input']['value']['basis'],before['basis'],'actual color reply/current GameBasis')
    same(call['input']['value']['actor'],frame['chooser'],'actual color reply/owned chooser')
    require(command['choice']['color'] in frame['offered'],'actual color reply belongs to issued domain')
    same(after['pending_work'],dict(tag='ResolutionPayment',work=pending['parent']['work']),'actual color child restores complete same payment parent')
    same(after['resolution_state'],before['resolution_state'],'actual color child retains complete original cursor and locals')
    same(after['resolving'],before['resolving'],'actual color child retains same complete resolving payload')
    require(after['decision'] is not None and after['decision']['id']!=decision['id'],'actual restored parent has fresh decision identity')
    same(after['decision']['basis'],after['basis'],'actual restored parent decision current basis')
    validate_optional_pending(after)
    for field in ('stack','waiting','pending_trigger_placement'):same(after[field],before[field],'actual color child preserves '+field+' without placement')
    same(candidate['checks'],dict(choices=[],moves=[]),'no actual state-based check during color child')
    same(candidate['ability_activation'],None,'color reply does not repeat actual mana activation')
    production=candidate['mana_production']
    expected_produced={} if integer(frame['amount'],'actual chosen-color amount')==0 else {command['choice']['color']:frame['amount']}
    same(production,dict(source=frame['source'],ability=frame['ability'],controller=frame['chooser'],produced=expected_produced),'actual chosen color/amount/source production fact')
    publications=[item['value'] for event in candidate['append'] if event['tag']=='Event' for item in event['occurrences'] if item['value']['tag']=='ManaProduced']
    same(len(publications),1 if expected_produced else 0,'actual color child publishes positive production exactly once and no zero production occurrence')
    if publications:same(publications[0]['production'],production,'actual color production fact/journal relation')
    require(not any(row['tag'] in ('OptionalEffectOutcome','ResolutionPaymentOutcome') for row in candidate['append']),'color child cannot consume the restored payment parent')
    return dict(owner=before['basis']['game']+':'+decision['id'],call_id=call['call_id'])


def optional_mana_prefix_join(before:dict[str,Any],call:dict[str,Any])->dict[str,Any]|None:
    """Read actual tap/activation/production publications under a payment owner."""
    pending=before['pending_work'];command=call['input']['value']['command']
    if pending is None or pending['tag']!='ResolutionPayment' or command['tag']!='ActivateMana':return None
    candidate=call['result']['candidate'];after=candidate['state'];source=command['source'];ability=command['ability'];actor=call['input']['value']['actor']
    same(actor,pending['work']['payer'],'actual payment parent owns mana activation authority')
    same(after['window_phase'],before['window_phase'],'no actual ordinary priority cut during payment mana prefix')
    require('checks' in candidate,'actual payment mana prefix requires complete no-check proof')
    same(candidate['checks'],dict(choices=[],moves=[]),'no actual state-based check during payment mana prefix')
    same(candidate['ability_activation'],None,'mana prefix does not fabricate stack activation fact')
    journal=candidate['append'];events=[row for row in journal if row['tag']=='Event']
    costs=[(i,row) for i,row in enumerate(events) if row['cause']['tag']=='ManaAbilityCost']
    activated=[(i,row) for i,row in enumerate(events) if row['cause']['tag']=='ManaAbilityActivated']
    produced=[(i,row) for i,row in enumerate(events) if row['cause']['tag']=='ManaProduced']
    same(len(costs),1,'actual accepted mana tap-cost publication exactly once')
    same(len(activated),1,'actual accepted mana activation publication exactly once')
    cost_i,cost=costs[0];activation_i,activation=activated[0]
    require(cost_i<activation_i,'actual tap-cost cut precedes activation publication')
    for row in (cost,activation):same((row['cause']['source'],row['cause']['ability']),(source,ability),'actual cost/activation source incarnation and ability')
    same(len(cost['occurrences']),1,'actual one tap-cost status change')
    status=cost['occurrences'][0]['value'];same(status['tag'],'StatusChange','actual tap-cost status occurrence kind')
    objects={row['object']['object']:row['object'] for row in before['objects']};after_objects={row['object']['object']:row['object'] for row in after['objects']}
    same(status['before'],objects[source],'actual mana tap-cost before world cut')
    same(status['after'],after_objects[source],'actual mana tap-cost after world cut')
    same((status['before']['tapped'],status['after']['tapped'],status['reason']),(False,True,'ManaAbilityCost'),'actual mana accepted source tap and cost reason')
    same(len(activation['occurrences']),1,'actual one activation occurrence')
    same(activation['occurrences'][0]['value'],dict(tag='ManaAbility',source=status['after'],ability=ability,controller=actor),'actual activation publication owns post-cost source and actor')
    same(after['resolution_state'],before['resolution_state'],'actual mana prefix retains complete cursor and locals')
    same(after['resolving'],before['resolving'],'actual mana prefix retains complete resolving payload')
    same(after['stack'],before['stack'],'actual mana prefix leaves owned stack payloads in place')
    same(after['pending_trigger_placement'],before['pending_trigger_placement'],'no actual trigger placement during payment mana prefix')
    require(not any(row['tag'] in ('TriggerPlaced','OptionalEffectOutcome','ResolutionPaymentOutcome') for row in journal),'no actual placement or payment outcome during mana prefix')
    after_pending=after['pending_work'];require(after_pending is not None,'actual mana prefix returns its payment owner or child')
    if after_pending['tag']=='ManaColor':
        same(after_pending['parent'],dict(tag='Payment',work=pending['work']),'actual color child suspends complete same payment owner')
        same((after_pending['frame']['source'],after_pending['frame']['ability'],after_pending['frame']['chooser']),(source,ability,actor),'actual color child source ability and payment actor')
        same(candidate['mana_production'],None,'actual color child begins before any production fact')
        same(produced,[],'actual color child begins before production publication')
        same(after['mana'],before['mana'],'actual color child begins without pool mutation')
    else:
        same(after_pending,pending,'actual complete mana prefix restores same payment owner')
        require('mana_production' in candidate and candidate['mana_production'] is not None,'actual complete mana prefix requires production fact')
        same((candidate['mana_production']['source'],candidate['mana_production']['ability'],candidate['mana_production']['controller']),(source,ability,actor),'actual production source ability and payment controller')
        same(len(produced),1 if candidate['mana_production']['produced'] else 0,'actual complete mana prefix positive production once and no zero production occurrence')
        if produced:
            production_i,event=produced[0];require(activation_i<production_i,'actual activation publication precedes production cut')
            same(len(event['occurrences']),1,'actual one production occurrence')
            publication=event['occurrences'][0]['value'];same(publication['production'],candidate['mana_production'],'actual prefix production fact/journal relation')
            pools={row['player']:row['pool'] for row in before['mana']};after_pools={row['player']:row['pool'] for row in after['mana']}
            same(publication['before'],pools[actor],'actual production before pool cut');same(publication['after'],after_pools[actor],'actual production after pool cut')
            for color in _NATIVE_MANA.values():same(integer(publication['after'][color],'actual produced pool after')-integer(publication['before'][color],'actual produced pool before'),integer(candidate['mana_production']['produced'].get(color,'0'),'actual produced amount'),'actual full production amount/color pool delta')
        else:same(after['mana'],before['mana'],'actual zero-count mana prefix retains all pools')
    validate_optional_pending(after)
    return dict(call_id=call['call_id'],source=source,ability=ability,parent=pending['work']['cursor'])


def optional_color_restoration_inventory(owners:set[str],restorations:list[dict[str,Any]])->None:
    """Count actual accepted child-return cuts, without an invented counter field."""
    counts={owner:sum(row['owner']==owner for row in restorations) for owner in owners}
    same(set(row['owner'] for row in restorations),owners,'actual complete color-parent restoration owner inventory')
    same(counts,{owner:1 for owner in owners},'actual exactly once color-parent restoration cut')


def optional_replay(run:dict[str,Any])->dict[str,Any]:
    """Reuse the exact inventory checks with the distinct source-owned Report4 labels."""
    same(run['schema_version'],4,'actual Report4 required')
    same(run['rerun']['comparison_fields'],OPTIONAL_REPLAY_FIELDS,'exact complete Report4 replay fields')
    # Only the epoch's declared comparison labels differ; original inputs, journal
    # receipts, probe arrays and full-state equality remain with the shared owner.
    adapted=dict(run,rerun=dict(run['rerun'],comparison_fields=MANDATORY_REPLAY_FIELDS))
    return mandatory_replay(adapted)



def validate_optional_registry(coverage:dict[str,Any],manifest:dict[str,Any],profile:str)->None:
    """Bind exact Registry8 keys/signatures to the retained actual C03 owner output."""
    same(profile,OPTIONAL_PROFILE,'strict optional Registry profile')
    same(coverage,manifest['native_registry_coverage'],'actual complete retained C03 Registry8 coverage')
    rows=unique(coverage['cards'],'oracleId','actual optional Registry8 source19')
    definitions=unique(manifest['compiled']['definitions'],'oracleId','actual C03 source19')
    same(set(rows),set(definitions),'actual Registry8 exact source19 identities')
    for oracle,row in rows.items():
        source=definitions[oracle]
        same(row['name'],source['name'],'actual Registry8 source name')
        same(row['binding']['semanticSha256'],source['registrySignatureV1Sha256'],'actual Registry8/C03 native signature')
        same(row['binding']['scope'],'optional-payment-ordinary-card-v1','actual Registry8 finite scope')
        same(row['binding']['status'],'partial','all optional source bindings remain Partial')


def validate_optional_native_definitions(definitions:list[dict[str,Any]],manifest:dict[str,Any],vocabulary:dict[str,Any],selected_ids:set[str]|None=None)->dict[str,dict[str,Any]]:
    """Bind real definition source/printing metadata and complete Program presence.

    The shared index checks every full Program against the complete supplied seed
    and every native state. Independent whole-clause/Program adjudication is still
    required; this metadata gate does not turn a signature string into that proof.
    """
    same(vocabulary['id'],manifest['vocabulary']['id'],'actual optional source vocabulary')
    compiled=unique(manifest['compiled']['definitions'],'oracleId','actual optional compiled source19')
    rows=unique(definitions,'oracle_id','actual optional used definitions')
    required=set(compiled) if selected_ids is None else selected_ids
    require(required<=set(compiled),'only optional source19 definitions')
    same(set(rows),required,'exact actual optional used source definitions')
    for oracle,row in rows.items():
        source=compiled[oracle]
        same(set(row),{'collector','language','name','oracle_id','printing_id','program','reference','set','sources'},'exact complete optional definition fields')
        for field,value in [('reference',oracle),('name',source['name']),('printing_id',source['scryfallId']),('set',source['set']),('collector',source['collector']),('language',source['language'])]:
            same(row[field],value,'actual optional source '+field)
        same(row['sources'],manifest['definition_sources'],'actual optional complete source receipt')
        require(isinstance(row['program'],dict) and bool(row['program']),'actual complete optional Program present')
        if 'source_programs' in manifest:
            same(row['program'],manifest['source_programs'][oracle],'actual complete independently exported source Program and nested templates')
    return rows


def bind_optional_program_export(export:dict[str,Any],compiled:dict[str,Any],registry:dict[str,Any],vocabulary:dict[str,Any])->dict[str,Any]:
    """Bind a separate actual source export without taking values from a game packet.

    This structural join cannot substitute for the required independent primary
    whole-clause review or the export's actual process/source/executable receipts.
    Callers must bind that reviewed authority before granting final source credit.
    """
    same(set(export),{'schema_version','kind','profile','registry_version','catalogue_sha256','vocabulary','epochs','token_template_references','rows','limits'},'exact separate source Program export fields')
    same(export['schema_version'],1,'separate source Program export schema1')
    same(export['kind'],'guarded-source19-program-observation-v1','separate guarded source Program export kind')
    same(export['profile'],OPTIONAL_PROFILE,'separate source Program profile')
    same(export['registry_version'],8,'separate source Program Registry8')
    same(export['epochs'],OPTIONAL_EPOCHS,'separate source Program explicit epochs')
    same(export['catalogue_sha256'],registry['sourceSha256'],'separate source Program catalogue')
    same(export['vocabulary'],vocabulary,'separate source Program complete vocabulary')
    same(export['token_template_references'],compiled['token_templates'],'separate source Program exact2 template descriptors')
    definitions=unique(compiled['definitions'],'oracleId','separate C03 source19')
    bindings=unique(registry['cards'],'oracleId','separate native Registry8 source19')
    rows={}
    for row in export['rows']:
        same(set(row),{'source_definition','registry_binding','program'},'exact complete separate source Program row')
        oracle=row['source_definition']['oracleId']
        require(oracle in definitions and oracle not in rows,'exact distinct separate source19 member')
        same(row['source_definition'],definitions[oracle],'separate complete C03 source/printing/signature definition')
        same(row['registry_binding'],bindings[oracle],'separate exact native ProgramKey/scope/signature row')
        require(isinstance(row['program'],dict) and bool(row['program']),'separate complete native Program present')
        rows[oracle]=row['program']
    same(set(rows),set(definitions),'separate complete source19 Program inventory')
    return rows


def optional_role_join(state:dict[str,Any])->None:
    """Check actual source operand to complete captured/current player relations."""
    pending=state['pending_work']
    if pending is None:return
    kind=pending['tag'];work=None
    if kind in ('OptionalEffect','ResolutionPayment'):work=pending['work']
    elif kind=='ManaColor' and pending['parent']['tag']=='Payment':kind='ResolutionPayment';work=pending['parent']['work']
    if work is None:return
    entry=state['resolving'];cursor=work['cursor'];index=integer(cursor['instruction'],'actual optional source instruction index')
    instructions=entry['body']['instructions'];require(0<=index<len(instructions),'actual cursor has source body instruction')
    instruction=instructions[index];same(instruction['op'],'OptionalEffect' if kind=='OptionalEffect' else 'OfferManaPayment','actual source/owned choice instruction')
    field='chooser' if kind=='OptionalEffect' else 'payer';role=instruction[field];actor=work[field]
    if role=='Controller':same(actor,entry['controller'],'actual captured resolving entry controller')
    elif role=='UniqueRelatedPlayer':
        binding=entry['bindings']['trigger'];members=binding['causes']['members']
        require(bool(members),'actual complete related cause set present')
        players={row['related_player'] for row in members}
        same(len(players),1,'actual complete cause set has unique related player')
        require(None not in players and players<=set(state['participants']),'actual related player is current participant')
        same(actor,next(iter(players)),'actual unique related payer/chooser')
    else:
        same(role,'TargetSpellController','closed actual resolving player operand')
        targets=[row['target']['entry'] for row in entry['targets'] if row['target']['tag']=='Stack']
        same(len(targets),1,'actual one current bound spell target')
        stack=unique(state['stack'],'entry','actual current target stack inventory')
        require(targets[0] in stack,'actual target spell still present')
        target=stack[targets[0]];require(target['kind']['tag'] in ('CardSpell','SpellCopy'),'actual target is card spell or actual copy')
        same(actor,target['controller'],'actual exact bound current target-spell controller')
    if kind=='ResolutionPayment':
        for field in ('cost','paid_destination','declined_destination'):
            same(work[field],instruction[field],'actual owned payment quote/successor/source '+field)


def optional_completion_join(run:dict[str,Any],recipe:dict[str,Any])->dict[str,Any]:
    """Join the complete final summary to its actual endpoint and frozen stop.

    Full saved-input/canonical-state replay is checked by the shared replay and
    witness owners. This observes transport/source relations, not game semantics.
    """
    accepted=run['accepted'];n=len(accepted);final=run['final'];header=(accepted[-1] if accepted else run['initialization'])['header']
    same(final['policy_complete'],True,'actual optional complete saved policy');same(final['policy_entries_consumed'],final['policy_entries_total'],'actual optional no unconsumed executable policy entries')
    for field in ('accepted_inputs','final_ordinal','checkpoint_ref'):same(final[field],n,'actual optional final '+field+' equals complete accepted input count')
    common={'accepted_inputs','checkpoint_ref','final_ordinal','policy_complete','policy_entries_consumed','policy_entries_total'};stop=recipe['stop'];position=header['turn']['position']
    require(n<=recipe['budget']['maximum_accepted_commands'],'actual optional completed input budget');require(position['number']<=recipe['budget']['maximum_individual_turn'],'actual optional completed turn budget')
    if stop['kind'] in ('bounded_ready','bounded_at_stack'):
        same(set(final),common|{'active','boundary_ref','holder','kind','position'},'exact optional bounded final summary');same(final['kind'],'bounded_prefix','actual optional bounded completion');same(final['boundary_ref'],recipe['id'],'actual optional bounded source boundary');same(final['active'],header['active'],'actual optional final active');same(final['holder'],header['priority_holder'],'actual optional final holder');same(final['position'],header['turn']['position'],'actual optional final turn position');same(header['outcome'],None,'actual optional bounded outcome absent')
        boundary=stop['boundary'];same((position['number'],position['step'],mandatory_role(header['active']),mandatory_role(header['priority_holder'])),(boundary['turn'],boundary['step'].replace('-','_'),boundary['active'],boundary['holder']),'actual optional exact frozen bounded stop');same(header['phase'],dict(kind='priority',phase=dict(kind='ready',holder=header['priority_holder'])),'actual optional bounded stop Ready')
        if stop['kind']=='bounded_at_stack':
            native=optional_native_common_index(run,recipe);same([native['entries'][row['entry']] for row in header['execution']['stack']],[optional_entry_name(row) for row in stop['stack_bottom_first']],'actual optional exact ordered source survivor stack')
        else:same(header['execution']['stack'],[],'actual optional drained bounded Ready stack')
    else:
        same(stop['kind'],'natural_outcome','explicit optional natural stop');same(set(final),common|{'entropy_position','entropy_remaining','outcome'},'exact optional natural final summary');same(final['outcome'],header['outcome'],'actual optional complete final outcome/terminal endpoint');require(header['outcome'] is not None,'actual optional natural outcome present');same(set(header['outcome']),{'kind','seat'},'actual optional winner outcome fields');same(header['outcome']['kind'],'winner','actual optional natural winner outcome');require(header['outcome']['seat'] in header['participants'],'actual optional winner registered')
        entropy=header['entropy'];same(final['entropy_position'],None if entropy is None else entropy['position'],'actual optional final entropy position');same(final['entropy_remaining'],None if entropy is None else entropy['remaining'],'actual optional final entropy remaining');same((position['number'],position['step'],mandatory_role(header['active'])),(stop['turn'],stop['step'].replace('-','_'),stop['active']),'actual optional exact source natural outcome cut')
        losses=[occurrence['value']['player'] for row in [run['initialization'],*accepted] for event in row['current_facts']['rules_journal'] if event['tag']=='Event' for occurrence in event['occurrences'] if occurrence['value']['tag']=='PlayerLost'];same(sorted(header['losses']),sorted(losses),'actual optional final loser partition/complete loss publications');require(header['outcome']['seat'] not in losses and set(losses)==set(header['participants'])-{header['outcome']['seat']},'actual optional winner and loser partition')
        execution=header['execution']
        for field in ('stack','waiting','copies'):same(execution[field],[],'actual optional terminal empty '+field)
        for field in ('resolution','pending_trigger_placement','pending_work','resolving'):same(execution[field],None,'actual optional terminal absent '+field)
        for field in ('pending','pending_turn_work'):same(header[field],None,'actual optional terminal absent '+field)
    return dict(kind=stop['kind'],accepted=n,final_ordinal=final['final_ordinal'],checkpoint_ref=final['checkpoint_ref'],outcome=header['outcome'],losses=header['losses'],turn=position,active=header['active'],saved_policy_complete=True,full_saved_original_input_reexecution=run['rerun']['all_inputs_consumed'],full_state_equal=run['rerun']['final_state_equal'],every_checkpoint_reexecuted=len(run['rerun']['checkpoint_ordinals_compared']))


class OptionalWitnessIndex(MandatoryWitnessIndex):
    """Use the one bounded streaming call/point/index owner with explicit Report4 hooks."""
    PROFILE=OPTIONAL_PROFILE
    OUTER_PROFILE='optional-payment'
    REGISTRY_VERSION=8
    REPORT_VERSION=4
    CURRENT_VERSION=10
    WITNESS_SCHEMA='spellwake.current-type-context-witness.v8'
    SOURCE_COUNT=39
    PROGRAM_CONTEXT='optional-payment-v1:registry8'
    MAX_ACCEPTED=4096
    MAX_PROBES=256
    WIRE=OptionalWire
    REPLAY=staticmethod(optional_replay)
    STATE_SIGNATURE=staticmethod(optional_native_state_signature)
    HEADER_SIGNATURE=staticmethod(optional_ordinary_state_signature)
    JOURNAL=staticmethod(validate_optional_journal)
    REGISTRY=staticmethod(validate_optional_registry)
    DEFINITIONS=staticmethod(validate_optional_native_definitions)
    VALIDATE_OWNED_PAYLOADS=True
    CONTROL_JOIN=staticmethod(optional_control_join)

    def __init__(self)->None:
        super().__init__()
        self.optional_calls:dict[str,Any]={}
        self.control_joins:list[dict[str,Any]]=[]
        self.color_owners:set[str]=set()
        self.color_restorations:list[dict[str,Any]]=[]

    def observe_report(self,inner:dict[str,Any])->None:
        same(inner['status'],'success','actual optional API success required before baseline')
        super().observe_report(inner)

    def add_call(self,call:dict[str,Any])->None:
        super().add_call(call)
        if call['result']['tag']=='Ok':
            candidate=call['result']['candidate']
            # Preserve only the actual fields consumed by the control relation
            # join. Full state rows are validated/digested once by the shared owner.
            state_fields=('basis','decision','effect_revision','pending_work','resolution_state','resolving','stack','waiting','pending_trigger_placement','mana','window_phase')
            command=call['input'].get('value',{}).get('command',{});objects=[row for row in candidate['state']['objects'] if command.get('tag')=='ActivateMana' and row['object']['object']==command['source']]
            self.optional_calls[call['before_point']]=dict(call_id=call['call_id'],input=call['input'],result=dict(candidate={**{key:candidate[key] for key in ('append','mana_payment','mana_production','checks','ability_activation')},'state':dict({key:candidate['state'][key] for key in state_fields},objects=objects)}))

    def add_point(self,point:dict[str,Any])->None:
        super().add_point(point)
        if point['owner']!='GameState':return
        state=point['projection']['value'];optional_role_join(state)
        pending=state['pending_work']
        if pending is not None and pending['tag']=='ManaColor' and pending['parent']['tag']=='Payment':self.color_owners.add(state['basis']['game']+':'+state['decision']['id'])
        call=self.optional_calls.get(point['point_id'])
        if call is not None:
            rows=self.CONTROL_JOIN(state,call)
            self.control_joins.extend(dict(before_point=point['point_id'],record=row) for row in rows)
            restored=optional_color_reply_join(state,call)
            if restored is not None:self.color_restorations.append(restored)
            optional_mana_prefix_join(state,call)

    def finish(self,outer:dict[str,Any],witness:dict[str,Any],manifest:dict[str,Any],expected_case:str|None=None)->tuple[dict[str,Any],dict[str,Any]]:
        optional_color_restoration_inventory(self.color_owners,self.color_restorations)
        report,provenance=super().finish(outer,witness,manifest,expected_case)
        run=report['runs'][0];recipe=unique(manifest['recipes']['cases'],'id','actual optional recipe case26')[run['fixture_ref']]
        require(len(run['accepted'])<=recipe['budget']['maximum_accepted_commands'],'actual source-case accepted-input budget')
        require(len(run['rejection_probes'])<=recipe['reached_probe_limit'],'actual source-case reached-probe budget')
        completion=optional_completion_join(run,recipe);native=optional_native_common_index(run,recipe);body_order=optional_body_publications(native);primary=optional_full_primary(native,manifest['compiled'],run['fixture_ref'])
        source_rows=unique(witness['source_inputs'],'path','actual optional witnessed source')
        same(set(source_rows),set(manifest['capture_source_rows']),'actual exact source-declared optional capture closure')
        for path,row in source_rows.items():same(row,manifest['capture_source_rows'][path],'actual captured source/retained build receipt '+path)
        provenance.update(native_section_scope='complete canonical state/input/endpoint/all13-fact structural relations in this development packet; full comparison remains separate',source_program_adjudication=manifest.get('primary_program_authority','PENDING independent full source-clause/Program/template authority'),
            optional_control_joins=self.control_joins,completion=completion,ordered_body_publications=body_order,independent_primary_values=primary,globally_fully_qualified_cards=0,final_image_qualification=False)
        return report,provenance


def read_optional_capture(path:Path,manifest:dict[str,Any],expected_case:str|None=None)->MandatoryCapture:
    """Consume actual current10 through the maintained bounded raw reader."""
    return read_mandatory_capture(path,manifest,expected_case,index_factory=OptionalWitnessIndex)


def current10_predecessor_state_signature(state:dict[str,Any])->dict[str,Any]:
    """Project only the unchanged predecessor Header relation after full10 checks.

    MandatoryWitnessIndex._state separately digests the entire canonical state,
    including the new owner observations. Call/point/reexecution equality never
    uses this deliberately narrower Header projection as its state digest.
    """
    complete=optional_native_state_signature(state)
    return {key:complete[key] for key in _STATE_SIGNATURE_FIELDS|{'world_digest'}}


def current10_predecessor_header_signature(wire:MandatoryWire,header:dict[str,Any],world_digest:str)->dict[str,Any]:
    same(set(header['execution']),{'stack','resolution','waiting','pending_trigger_placement'},'unchanged predecessor execution observation field set')
    return ordinary_state_signature(wire,header,world_digest)


class Current10MandatoryWitnessIndex(MandatoryWitnessIndex):
    """Explicit fresh current10/witness8 adapter for the unchanged Report3 API."""
    CURRENT_VERSION=10
    WITNESS_SCHEMA='spellwake.current-type-context-witness.v8'
    STATE_SIGNATURE=staticmethod(current10_predecessor_state_signature)
    HEADER_SIGNATURE=staticmethod(current10_predecessor_header_signature)
    VALIDATE_OWNED_PAYLOADS=True

    def finish(self,outer:dict[str,Any],witness:dict[str,Any],manifest:dict[str,Any],expected_case:str|None=None)->tuple[dict[str,Any],dict[str,Any]]:
        report,provenance=super().finish(outer,witness,manifest,expected_case)
        provenance.update(current_epoch=10,witness_epoch=8,observation_epoch=3,
            canonical_additions='pending_work/copies/resolving validated and retained in every complete native state digest',
            header_projection='explicit unchanged Report3 projection; no packet field removal or epoch relabeling',
            historical_current9_reader_unchanged=True)
        return report,provenance


def read_current10_mandatory_capture(path:Path,manifest:dict[str,Any],expected_case:str|None=None)->MandatoryCapture:
    """Read a separately source-bound fresh10 packet; historical9 stays strict."""
    return read_mandatory_capture(path,manifest,expected_case,index_factory=Current10MandatoryWitnessIndex)


def predecessor_endpoints(run:dict[str,Any])->Any:
    """Use the maintained endpoint owner with the old API's absent token delta.

    Initial complete token observations remain real checkpoint values. Every
    represented checkpoint and every physical delta is still joined; the fixed
    Report1/2 source families do not author token creation.
    """
    return mandatory_endpoints(run,token_changes_required=False,complete_checkpoints=run['schema_version']==2)


def predecessor_replay(run:dict[str,Any])->dict[str,Any]:
    schema=run['schema_version'];require(schema in (1,2),'explicit Report1 or Report2 replay')
    fields=MANDATORY_REPLAY_FIELDS[:7]+['interaction_execution_stack_and_resolution','interaction_instruction_journal','complete_native_rules_journals','full_final_native_state'] if schema==2 else [*MANDATORY_REPLAY_FIELDS[:4],'all_twelve_current_fact_families',*MANDATORY_REPLAY_FIELDS[5:7]]
    accepted=run['accepted'];same([row['ordinal'] for row in accepted],list(range(1,len(accepted)+1)),'actual predecessor accepted ordinals');rerun=run['rerun'];same(set(rerun),{'all_inputs_consumed','checkpoint_ordinals_compared','compared_ordinals','comparison_fields','final_state_equal','initialization_equal','probe_ids_compared'},'exact complete predecessor rerun fields')
    for field in ('all_inputs_consumed','final_state_equal','initialization_equal'):same(rerun[field],True,'actual predecessor saved-input rerun '+field)
    same(rerun['comparison_fields'],fields,'exact unchanged predecessor replay comparison labels');same(rerun['compared_ordinals'],list(range(1,len(accepted)+1)),'every predecessor accepted input reexecuted');same(rerun['checkpoint_ordinals_compared'],sorted(int(key) for key in run['checkpoints']),'every actual predecessor checkpoint reexecuted')
    probes=unique(run['rejection_probes'],'probe_id','actual predecessor probes');same(rerun['probe_ids_compared'],list(probes),'every predecessor actual probe reexecuted')
    for probe in probes.values():
        same(probe['no_candidate'],True,'predecessor rejection has no candidate');same(probe['state_retained'],True,'predecessor rejection retains whole state');same(probe['actual_error'],probe['expected_error'],'predecessor typed rejected error')
    return probes


class Current10BlueWire(MandatoryWire):
    CONTEXT_PREFIX='interaction-context-v1-'
    JOURNAL_FIELD='interaction_journal'


class Current10OrdinaryWire(OptionalWire):
    JOURNAL_FIELD=None
    def primary_context_reference(self)->str:return 'ordinary-context-v1'


def current10_blue_state_signature(state:dict[str,Any])->dict[str,Any]:
    signature=current10_predecessor_state_signature(state)
    return {key:value for key,value in signature.items() if key not in ('waiting','pending_trigger_placement')}


def current10_blue_header_signature(wire:MandatoryWire,header:dict[str,Any],world_digest:str)->dict[str,Any]:
    execution=header['interaction_execution'];same(set(execution),{'stack','resolution'},'unchanged Report2 execution fields')
    return dict(ordinary_common_state_signature(wire,header,world_digest),stack=execution['stack'],resolution_state=execution['resolution'])


def predecessor_legacy_stack(wire:MandatoryWire,rows:list[dict[str,Any]])->list[dict[str,Any]]:
    result=[]
    for row in rows:
        same(row['kind']['kind'],'card_spell','finite authored Report1 legacy stack kind')
        result.append(dict(entry=row['entry'],controller=row['controller'],kind=dict(tag='CardSpell',object=wire.reference(row['kind']['card'])),targets=[dict(id='TargetId('+str(target['id'])+')',clause=target['clause'],target=wire.target(target['target'])) for target in row['targets']]))
    return result


def current10_ordinary_state_signature(state:dict[str,Any])->dict[str,Any]:
    signature=current10_predecessor_state_signature(state)
    result={key:value for key,value in signature.items() if key not in ('waiting','pending_trigger_placement','resolution_state')}
    result['stack']=[{key:row[key] for key in ('entry','controller','kind','targets')} for row in state['stack']]
    return result


def current10_ordinary_header_signature(wire:MandatoryWire,header:dict[str,Any],world_digest:str)->dict[str,Any]:
    return dict(ordinary_common_state_signature(wire,header,world_digest),stack=predecessor_legacy_stack(wire,header['stack']))


class Current10BlueWitnessIndex(Current10MandatoryWitnessIndex):
    """Fresh10 full canonical owner joins with the unchanged Report2 projection."""
    PROFILE=PROFILES[0]
    OUTER_PROFILE='interaction'
    REGISTRY_VERSION=6
    REPORT_VERSION=2
    SOURCE_COUNT=None
    PROGRAM_CONTEXT='blue-interaction-v1:registry6'
    MAX_PROBES=256
    WIRE=Current10BlueWire
    REPLAY=staticmethod(predecessor_replay)
    ENDPOINTS=staticmethod(predecessor_endpoints)
    STATE_SIGNATURE=staticmethod(current10_blue_state_signature)
    HEADER_SIGNATURE=staticmethod(current10_blue_header_signature)

    def validate_source_owners(self,report:dict[str,Any],witness:dict[str,Any],manifest:dict[str,Any])->dict[str,Any]:
        same(report['catalogue_sha256'],manifest['provenance']['compressedSha256'],'actual fresh Blue catalogue authority');same(witness['registry']['tag'],'Registry','actual fresh Blue Registry witness');validate_registry(witness['registry']['coverage'],manifest,self.PROFILE)
        vocabulary=read(Path(manifest['vocabulary']['path']));same(sha(Path(manifest['vocabulary']['path'])),manifest['vocabulary']['sha256'],'actual fresh predecessor vocabulary bytes');same(witness['vocabulary']['value'],vocabulary,'actual fresh predecessor complete vocabulary');validate_native_definitions(self.run['static_inputs']['definitions'],manifest);same(self.vocabulary_id,vocabulary['id'],'actual complete predecessor vocabulary identity')
        return vocabulary

    def validate_context_owner(self,primary:dict[str,Any],report:dict[str,Any],manifest:dict[str,Any],vocabulary:dict[str,Any])->None:
        same(primary,manifest['expected_context'],'actual whole preserved predecessor context source authority')

    def finish(self,outer:dict[str,Any],witness:dict[str,Any],manifest:dict[str,Any],expected_case:str|None=None)->tuple[dict[str,Any],dict[str,Any]]:
        # Use the shared raw owner directly: the Report3-specific provenance
        # suffix does not describe this unchanged Report2 projection.
        report,provenance=MandatoryWitnessIndex.finish(self,outer,witness,manifest,expected_case)
        same(report['runs'][0],manifest['expected_run'],'whole predecessor observation values preserved against immutable historical baseline')
        provenance.update(current_epoch=10,witness_epoch=8,observation_epoch=self.REPORT_VERSION,complete_canonical_additions_retained=True,whole_historical_observation_values_preserved=True,typed_api_bytes_order_not_claimed=True)
        return report,provenance


class Current10OrdinaryWitnessIndex(Current10BlueWitnessIndex):
    """Fresh10 NoRegistry authored fixture with the unchanged Report1 projection."""
    PROFILE='ordinary-game-trajectories'
    OUTER_PROFILE='ordinary'
    REGISTRY_VERSION=None
    REPORT_VERSION=1
    WIRE=Current10OrdinaryWire
    STATE_SIGNATURE=staticmethod(current10_ordinary_state_signature)
    HEADER_SIGNATURE=staticmethod(current10_ordinary_header_signature)
    REPORT_FACT_OMISSIONS={'append'}
    COMPLETE_OBSERVATIONS=False
    CASE_OBSERVATION_NAMES=False

    def observe_report(self,inner:dict[str,Any])->None:
        same(set(inner),{'status','data'},'exact ordinary API response');same(inner['status'],'success','actual successful authored ordinary fixture');report=inner['data'];same(set(report),{'schema_version','profile','source_build_ref','runs'},'unchanged Report1 aggregate fields');same(report['schema_version'],1,'ordinary aggregate schema1');same(report['profile'],self.PROFILE,'exact ordinary aggregate profile');same(len(report['runs']),1,'one actual ordinary fixture per capture');self._observe_run(report,report['runs'][0])

    def validate_source_owners(self,report:dict[str,Any],witness:dict[str,Any],manifest:dict[str,Any])->dict[str,Any]:
        same(witness['registry'],dict(tag='NotApplicable',reason='NoRegistryConstruction'),'actual authored ordinary NoRegistry boundary');same(self.run['static_inputs']['definitions'],manifest['expected_run']['static_inputs']['definitions'],'complete authored fixture Programs against immutable historical definitions')
        vocabulary=read(Path(manifest['vocabulary']['path']));same(sha(Path(manifest['vocabulary']['path'])),manifest['vocabulary']['sha256'],'actual authored fixture vocabulary bytes');same(witness['vocabulary']['value'],vocabulary,'complete authored fixture vocabulary');same(self.vocabulary_id,vocabulary['id'],'actual authored fixture vocabulary identity');return vocabulary


def read_current10_predecessor_capture(path:Path,manifest:dict[str,Any],profile:str,expected_case:str)->MandatoryCapture:
    factories={'interaction':Current10BlueWitnessIndex,'ordinary':Current10OrdinaryWitnessIndex,'mandatory-trigger':Current10MandatoryWitnessIndex}
    require(profile in factories,'explicit finite fresh predecessor outer profile')
    return read_mandatory_capture(path,manifest,expected_case,index_factory=factories[profile])


OPTIONAL_METHODS=MANDATORY_METHODS|{'confirmTrigger','payCostToPreventEffect','confirmAction','chooseColor','notifyOfValue','chooseNewTargetsFor'}
OPTIONAL_UNAVAILABLE=['native GameBasis/DecisionId/frame revision','native ResolutionCursor/instruction successors/locals','native PendingWork/payment parent/restoration','private Forge waiting queue','private Forge failed-draw flag','native canonical journals/ManaPaymentRecord/full-state replay','selected physical source for a Forge synthetic copy host']


OPTIONAL_RAW_FIELDS=set('schema schema_version case_id reference forge_commit inputs_sha256 fixture paper_cards actual_outcome physical_aliases registered_players callbacks events random operations cast_operations settlement_events settlements failures observations checkpoints runtime completion final_world profile trigger_receipts token_receipts causal_events used_player_choices unavailable optional_schema_version optional_choices resolution_payments mana_activations spell_copies raw_stack_additions decision_plan_consumption synthetic_copy_hosts'.split())
_RETIREMENT_FIELDS=set('status policy_id input_instruction planned_slot_index plan_cursor_before plan_cursor_after actual_event_sequence actual_event actual_event_payload actual_defending_player actual_attackers actual_available_creatures actual_can_block actual_validate_blocks_error actual_controller_callback_entries_this_turn callback_id input_issued accepted_game_action game_transition_performed_by_adapter observable_world_unchanged_except_adapter_cursor before_world after_world actual_stack_frames retirement_sequence'.split())
_ACCOUNTING_FIELDS=set('advanced_slots retired_not_issued_slots slots_advanced_by_existing_paths actions_consumed_includes_retired_plan_slots existing_paths_are_not_asserted_to_be_accepted_game_actions'.split())


def optional_wire_epoch(raw:dict[str,Any])->int:
    """Dispatch only the explicit integer writer epoch, keeping wire1 closed."""
    version=raw['optional_schema_version'];require(type(version) is int and version in (1,2),'exact integer optional Forge wire epoch1 or2')
    extra={'planned_input_retirements','plan_slot_accounting'}
    if version==1:require(not extra&set(raw),'strict Forge optional wire1 forbids wire2 retirement/accounting fields')
    same(set(raw),OPTIONAL_RAW_FIELDS|(extra if version==2 else set()),'exact optional Forge wire'+str(version)+' raw fields')
    return version


def optional_retirement_join(raw:dict[str,Any],case:dict[str,Any],entries:dict[int,Any])->list[dict[str,Any]]:
    """Observe genuine unissued plan slots, never inventing a callback or action."""
    if optional_wire_epoch(raw)==1:return []
    retirements=raw['planned_input_retirements'];require(isinstance(retirements,list),'typed wire2 retirement list');accounting=raw['plan_slot_accounting'];same(set(accounting),_ACCOUNTING_FIELDS,'closed wire2 plan-slot accounting')
    for field in ('advanced_slots','retired_not_issued_slots','slots_advanced_by_existing_paths'):require(type(accounting[field]) is int and accounting[field]>=0,'wire2 unsigned integer accounting '+field)
    same(accounting['advanced_slots'],raw['completion']['actions_consumed'],'wire2 advanced slots/actual plan cursor');same(accounting['retired_not_issued_slots'],len(retirements),'wire2 actual retired slot count');same(accounting['advanced_slots'],accounting['retired_not_issued_slots']+accounting['slots_advanced_by_existing_paths'],'wire2 separate retired and existing-path accounting');require(accounting['advanced_slots']<=raw['completion']['actions_planned'],'wire2 advanced slots within declared plan')
    for field in ('actions_consumed_includes_retired_plan_slots','existing_paths_are_not_asserted_to_be_accepted_game_actions'):same(accounting[field],True,'wire2 honest plan/action distinction '+field)
    for receipt in retirements:same(set(receipt),_RETIREMENT_FIELDS,'closed wire2 retirement receipt')
    same([row['retirement_sequence'] for row in retirements],sorted(row['retirement_sequence'] for row in retirements),'wire2 retirement list actual chronological operation order')
    events={r['global_sequence']:r for r in raw['events']};operations={r['global_sequence']:r for r in raw['operations']};seen_slots=set();seen_policies=set();seen_sequences=set();observations=unique(raw['observations'],'key','actual optional observation');results=[]
    for receipt in retirements:
        same(set(receipt),_RETIREMENT_FIELDS,'closed wire2 retirement receipt')
        for field in ('planned_slot_index','plan_cursor_before','plan_cursor_after','actual_event_sequence','retirement_sequence','actual_controller_callback_entries_this_turn'):require(type(receipt[field]) is int and receipt[field]>=0,'wire2 integer retirement '+field)
        slot=receipt['planned_slot_index'];require(slot<len(case['actions']) and slot not in seen_slots,'wire2 unique actual planned slot');seen_slots.add(slot);policy=case['actions'][slot];require(receipt['policy_id'] not in seen_policies,'wire2 unique original policy');seen_policies.add(receipt['policy_id']);same(receipt['policy_id'],policy['policy_id'],'wire2 exact original planned policy');same(receipt['input_instruction'],policy['input_instruction'],'wire2 complete unchanged source instruction');same(policy['kind'],'declare_blockers','wire2 admitted only unissued blocker plan');same(receipt['plan_cursor_before'],slot,'wire2 exact original one-slot before cursor');same(receipt['plan_cursor_after'],slot+1,'wire2 one-slot retirement interval')
        instruction=receipt['input_instruction'];same(instruction['kind'],'combat','wire2 actual declared combat instruction');same(instruction['blockers'],[],'wire2 original empty blockers plan');same(receipt['policy_id'],instruction['id']+'.blockers','wire2 original combat slot name');same(receipt['status'],'SKIPPED_NOT_ISSUED','wire2 distinct retirement disposition')
        for field in ('input_issued','accepted_game_action','game_transition_performed_by_adapter','actual_can_block'):same(receipt[field],False,'wire2 actual no '+field)
        for field in ('callback_id','actual_validate_blocks_error'):same(receipt[field],None,'wire2 actual absent '+field)
        same(receipt['observable_world_unchanged_except_adapter_cursor'],True,'wire2 observed cursor-only bookkeeping')
        before=receipt['before_world'];after=receipt['after_world'];same(before['header']['action_cursor'],slot,'wire2 actual before world plan cursor');same(after['header']['action_cursor'],slot+1,'wire2 actual after world plan cursor');same(dict(before,header={k:v for k,v in before['header'].items() if k!='action_cursor'}),dict(after,header={k:v for k,v in after['header'].items() if k!='action_cursor'}),'wire2 entire actual world unchanged except adapter cursor')
        for world in (before,after):optional_forge_world(world,raw,case)
        event=events[receipt['actual_event_sequence']];same(event['kind'],'GameEventBlockersDeclared','wire2 genuine blocker event');same(receipt['actual_event'],event['kind'],'wire2 retirement actual event family');same(receipt['actual_event_payload'],event['payload'],'wire2 complete actual event payload');same(receipt['actual_defending_player'],event['payload']['defendingPlayer'],'wire2 actual defending player');same(receipt['actual_defending_player'],policy['seat'],'wire2 exact planned defender');same((before['header']['individual_turn'],before['header']['step']),(policy['individual_turn'],policy['step']),'wire2 actual planned calendar cut');same(before['header']['active'],instruction['active'],'wire2 actual attacking player')
        attackers=sorted(row['alias'] for row in instruction['attackers']);require(bool(attackers) and len(set(attackers))==len(attackers),'wire2 genuine positive unique planned attackers');same(receipt['actual_attackers'],attackers,'wire2 actual complete source attackers');same(sorted(row['attacker'] for row in before['combat']),attackers,'wire2 actual full combat attacker membership')
        blockers=event['payload']['blockers'];same(sorted(row['attacker']['alias'] for row in blockers),attackers,'wire2 complete real event attacker membership');same([row for row in blockers if not row['unblocked_self_sentinel']],[],'wire2 actual no blocker assignment');require(all(row['value']==row['attacker'] and row['defender']==dict(kind='player',seat=receipt['actual_defending_player']) for row in blockers),'wire2 genuine unblocked self sentinel, not blocker')
        creatures=sorted(alias for alias in before['players'][receipt['actual_defending_player']]['battlefield'] if before['cards'][alias]['power'] is not None);same(receipt['actual_available_creatures'],creatures,'wire2 actual defender creature domain')
        calls=[r for r in entries.values() if r['method']=='declareBlockers' and r['context']['individual_turn']==policy['individual_turn']];same(calls,[],'wire2 no actual controller blocker input issued');same(receipt['actual_controller_callback_entries_this_turn'],len(calls),'wire2 actual callback count zero');require(event['parent_callback_id'] is None,'wire2 blocker event outside controller callback')
        frames=receipt['actual_stack_frames'];require(isinstance(frames,list) and all(set(frame)=={'class','method','descriptor'} for frame in frames),'wire2 actual complete stack frames');tags=[frame['class']+'.'+frame['method'] for frame in frames];same(tags.count('forge.game.phase.PhaseHandler.declareBlockersTurnBasedAction'),1,'wire2 actual pinned blocker-step owner')
        sequence=receipt['retirement_sequence'];require(sequence>receipt['actual_event_sequence'] and sequence not in seen_sequences,'wire2 unique bookkeeping sequence follows genuine event');seen_sequences.add(sequence);operation=operations[sequence];same(set(operation),(_RETIREMENT_FIELDS-{'retirement_sequence'})|{'ordinal','global_sequence','kind','case_id'},'closed wire2 retirement operation envelope');same(operation['kind'],'planned_input_retirement','wire2 distinct bookkeeping operation');same(operation['case_id'],case['case_id'],'wire2 operation actual case');same({k:v for k,v in operation.items() if k not in ('ordinal','global_sequence','kind','case_id')},{k:v for k,v in receipt.items() if k!='retirement_sequence'},'wire2 exact list/operation receipt projection')
        require('action.'+str(slot) not in observations,'wire2 retired slot has no accepted-action observation');observation=observations['planned-slot.'+str(slot)+'.not-issued'];same(observation['source_global_sequence'],sequence,'wire2 distinct bookkeeping observation owner');same(observation['evidence'],dict(status='SKIPPED_NOT_ISSUED',policy_id=receipt['policy_id'],actual_event_sequence=receipt['actual_event_sequence'],accepted_game_action=False),'wire2 exact honest retirement observation')
        results.append(dict(planned_slot=slot,policy=receipt['policy_id'],actual_event_sequence=event['global_sequence'],bookkeeping_sequence=sequence,accepted_game_action=False))
    same({r['global_sequence'] for r in raw['operations'] if r['kind']=='planned_input_retirement'},seen_sequences,'wire2 complete reciprocal retirement operation inventory')
    return results


def optional_forge_preflight(raw:dict[str,Any],case:dict[str,Any],inputs:dict[str,Any])->dict[int,dict[str,Any]]:
    """Reuse raw callback/source observers on a real optional Forge result."""
    same(raw['schema'],'interaction-game-forge-v1','actual maintained Forge raw schema')
    same(raw['schema_version'],1,'actual Forge raw schema1')
    optional_wire_epoch(raw)
    same(raw['profile'],OPTIONAL_PROFILE,'actual optional Forge profile')
    same(raw['forge_commit'],PIN,'actual pinned Forge dependency source')
    same(raw['case_id'],case['case_id'],'actual optional Forge case')
    same(raw['fixture'],case,'actual exact input-only Forge fixture')
    same(raw['failures'],[],'actual optional Forge has no sticky failures')
    same(raw['unavailable'],OPTIONAL_UNAVAILABLE,'honest actual Forge unavailable boundaries')
    entries=validate_streams(raw,OPTIONAL_METHODS,('P0','P1'))
    selected_forge_printings(raw,case,inputs)
    optional_retirement_join(raw,case,entries)
    complete=raw['completion']
    for consumed,planned in [('actions_consumed','actions_planned'),('decisions_consumed','decisions_planned'),('opening_consumed','opening_planned'),('random_operations_consumed','random_operations_planned')]:
        same(complete[consumed],complete[planned],'actual optional complete '+consumed)
    same(complete['actions_planned'],len(case['actions']),'actual optional declared action count')
    same(complete['decisions_planned'],len(case['decisions']),'actual optional declared decision count')
    same((complete['sticky_failure_count'],complete['callback_depth']),(0,0),'actual optional all callback leaves complete')
    plans=unique(case['decision_plans'],'id','actual optional source decision plan')
    same(len(raw['decision_plan_consumption']),len(plans),'actual complete optional source plan consumption count')
    same(set(raw['decision_plan_consumption']),set(plans),'actual exact optional source plan consumption inventory')
    return entries


def optional_physical_endpoint(run:dict[str,Any],row:dict[str,Any],endpoint:dict[str,Any],history:dict[str,str],lineage:dict[str,str]|None=None)->dict[str,Any]:
    """Use the shared physical/token world observer with real birth lineage."""
    aliases=unique(run['static_inputs']['aliases'],'alias','actual source-bound physical aliases')
    physical={alias:dict(oracle_id=value['definition_ref'],owner=mandatory_role(value['owner'])) for alias,value in aliases.items()}
    by_card={str(value['card_id']):alias for alias,value in aliases.items()}
    definitions=unique(run['static_inputs']['definitions'],'oracle_id','actual source-bound definitions')
    def remember(obj:dict[str,Any])->str:
        identity=obj['object']
        if obj['physical_card'] is None:
            require(lineage is not None and identity in lineage,'actual optional token joins existing native birth lineage');alias=lineage[identity]
        else:alias=by_card[str(obj['physical_card'])]
        if identity in history:same(history[identity],alias,'actual stable endpoint incarnation/alias')
        history[identity]=alias
        return alias
    return dict(ordinary_endpoint_world(row['header'],endpoint['cards'],endpoint['tokens'],endpoint['ordered_piles'],physical,definitions,history,remember),combat=ordinary_native_combat(row['header']))


def optional_common_world(native:dict[str,Any],forge:dict[str,Any],*,block_event:bool=False)->None:
    """Compare only independently observed world fields at the same declared cut."""
    same([{k:v for k,v in row.items() if not(block_event and k=='blocked')} for row in native['combat']],[{k:v for k,v in row.items() if not(block_event and k=='blocked')} for row in forge['combat']],'actual native/Forge complete combat cut')
    same(set(native['cards']),set(forge['cards']),'actual native/Forge complete physical cut inventory')
    for role,player in native['players'].items():
        for field in ('life','mana','library','hand','battlefield','graveyard','exile'):
            same(player[field],forge['players'][role][field],'actual native/Forge common cut '+role+'/'+field)
    for alias,card in native['cards'].items():
        for field in ('owner','controller','zone','tapped'):
            same(card[field],forge['cards'][alias][field],'actual native/Forge common physical '+alias+'/'+field)
        if card['zone']=='battlefield' and card['power'] is not None:
            for field in ('power','toughness','marked_damage','plus_one_counters','minus_one_counters'):
                same(card[field],forge['cards'][alias][field],'actual native/Forge common battlefield creature '+alias+'/'+field)


def bind_optional_primary_programs(table:dict[str,Any],export:dict[str,Any],compiled:dict[str,Any],registry:dict[str,Any],vocabulary:dict[str,Any])->dict[str,Any]:
    """Admit the accepted independent whole-source value table, never game-derived expectations."""
    same(table['kind'],'INDEPENDENT_WHOLE_SOURCE19_PROGRAM_PRIMARY_COMPARISON_V1','accepted independent source Program table kind')
    same(table['profile'],OPTIONAL_PROFILE,'accepted independent source Program profile')
    same(table['registry_version'],8,'accepted independent source Program Registry8')
    same(table['epochs'],OPTIONAL_EPOCHS,'accepted independent source Program epochs')
    same(table['counts'],dict(source_rows=19,whole_program_equal=19,complete_nested_templates_equal=2,blockers=0),'accepted independent source Program complete review counts')
    observed=bind_optional_program_export(export,compiled,registry,vocabulary)
    rows=unique(table['rows'],'oracle_id','independent primary Program source19')
    sources=unique(compiled['definitions'],'oracleId','strict source19 metadata')
    same(set(rows),set(sources),'independent primary exact source19 identities')
    programs={}
    for oracle,row in rows.items():
        same(row['source_definition'],sources[oracle],'independent primary complete source definition')
        same(row['whole_program_equal'],True,'independent primary whole Program accepted')
        same(row['expected_program'],observed[oracle],'separate export agrees with independent complete Program')
        same(row['observed_program'],observed[oracle],'independent primary review/export byte-value relation')
        programs[oracle]=row['expected_program']
    templates=table['nested_token_templates'];same(len(templates),2,'independent primary complete2 templates')
    for template in templates:
        same(template['whole_definition_equal'],True,'independent primary complete template accepted')
        same(template['complete_expected_definition'],template['complete_observed_definition'],'independent primary complete template equality')
        program=programs[template['source_oracle_id']]
        creations=[]
        if program['kind']['tag']=='Spell':creations.extend(c for op in program['kind']['body']['instructions'] if op['op']=='CreateTokens' for c in op['creations'])
        creations.extend(c for trigger in program['triggered'] for op in trigger['body']['instructions'] if op['op']=='CreateTokens' for c in op['creations'])
        same([c['definition'] for c in creations],[template['complete_expected_definition']],'independent primary actual complete nested template in whole Program')
    return programs


def validate_optional_stack_payloads(state:dict[str,Any],objects:dict[str,int|None],seen:dict[str,Any])->None:
    """Join actual installed/resolving/copy bodies to source and original payload owners."""
    programs={row['card_id']:row['definition'] for row in state['programs']}
    payload_fields=['body','copiable_card_types','copiable_creature_subtypes','copiable_names','copiable_supertypes','copiable_triggered']
    rows=state['stack']
    if state['resolving'] is not None:
        match=[row for row in rows if row['entry']==state['resolving']['entry']]
        if match:same(match,[state['resolving']],'actual queued/resolving payload is the same owned entry')
        else:rows=[*rows,state['resolving']]
    for row in rows:
        kind=row['kind']['tag'];entry=row['entry']
        if kind=='CardSpell':
            require(row['kind']['object'] in objects,'actual source stack incarnation known')
            card=objects[row['kind']['object']];require(card in programs,'actual source stack Program known')
            program=programs[card]
            same(row['body'],program['kind'].get('body'),'actual complete installed card-spell source body')
            for field,source in [('copiable_card_types','card_types'),('copiable_creature_subtypes','creature_subtypes'),('copiable_names','names'),('copiable_supertypes','supertypes'),('copiable_triggered','triggered')]:same(row[field],program[source],'actual installed complete source '+field)
        elif kind=='SpellCopy':
            original=row['kind']['copied_from'];require(original in seen,'actual copied original payload retained')
            same({field:row[field] for field in payload_fields},seen[original],'actual complete copied body/traits equal original owned payload')
        elif kind=='TriggeredAbility' and row['body'] is not None:
            binding=row['bindings']['trigger'];source=binding['source']['object'];card=source['physical_card']
            require(card in programs,'actual captured trigger source Program known')
            abilities=unique(programs[card]['triggered'],'id','actual source trigger definition')
            same(row['body'],abilities[binding['definition']]['body'],'actual complete installed captured trigger source body')
        if kind=='TriggeredAbility' and row['body'] is None:continue
        payload={field:row[field] for field in payload_fields}
        if entry in seen:same(payload,seen[entry],'actual installed body/traits immutable across full capture')
        else:seen[entry]=payload


def optional_forge_world(world:dict[str,Any],raw:dict[str,Any],case:dict[str,Any])->dict[str,Any]:
    """Keep actual Forge synthetic hosts separate from selected physical inventory."""
    aliases=set(raw['synthetic_copy_hosts'].values());copies=unique(raw['spell_copies'],'copy_label','actual copied entry label')
    by_host={row['copy_ability']['source']:row for row in copies.values()}
    same(aliases,set(by_host),'actual synthetic copy host/creation bijection')
    extra=set(world['cards'])-set(raw['physical_aliases'])-set(row['alias'] for row in raw['token_receipts'])
    require(extra<=aliases,'only actual registered synthetic copy hosts outside physical inventory')
    for alias in extra:
        card=world['cards'][alias];copy=by_host[alias]
        same(raw['synthetic_copy_hosts'][card['physical_id']],alias,'actual copy host physical identity')
        same(card['owner'],copy['actor'],'actual synthetic copy host owner/copy creator')
        same(card['token'],False,'synthetic spell host is not a creature token')
    for row in world['stack']:
        if not row['ability']['copied']:continue
        ability=row['ability'];require(ability['source'] in by_host,'actual copied stack entry registered')
        copy=by_host[ability['source']]
        same(ability,copy['installed_copy_ability'],'actual installed copied ability payload')
        same(row['stack_id'],copy['copy_stack_id'],'actual installed copy stack ID')
        same(row['controller'],copy['actor'],'actual copied spell controller')
    physical=dict(world,cards={key:value for key,value in world['cards'].items() if key not in aliases},stack=[row for row in world['stack'] if not row['ability']['copied']])
    projected=mandatory_world(physical,raw,case)
    return dict(projected,combat=sorted([dict(row,blockers=sorted(row['blockers'])) for row in world['combat']],key=lambda row:row['attacker']),actual_copy_hosts={key:world['cards'][key] for key in extra},actual_copy_stack=[row for row in world['stack'] if row['ability']['copied']])


def optional_entry_name(reference:dict[str,Any])->str:
    return reference[{'cast_entry':'operation','trigger_entry':'trigger','copy_entry':'copy'}[reference['kind']]]


def optional_native_common_index(run:dict[str,Any],case:dict[str,Any])->dict[str,Any]:
    """Index real endpoint/journal facts and bind declared labels to actual entry IDs."""
    rows=[run['initialization'],*run['accepted']];history={};worlds={};events={};entries={};casts={};triggers={};copies={};controls={};completions={};life=[];draws=[];failed=[];lineage={};births={}
    aliases=unique(run['static_inputs']['aliases'],'alias','actual native source aliases');by_card={int(row['card_id']):alias for alias,row in aliases.items()}
    def appearances(value:Any)->None:
        if isinstance(value,dict):
            if 'physical_card' in value and value['physical_card'] is not None:history[value['object']]=by_card[value['physical_card']]
            else:
                for member in value.values():appearances(member)
        elif isinstance(value,list):
            for member in value:appearances(member)
    for row,endpoint in mandatory_endpoints(run):
        appearances(row['current_facts']['rules_journal'])
    plans=unique(case['decision_plans'],'id','actual source plans')
    for row in rows:
        ordinal=row['ordinal'];facts=row['current_facts'];matching=[]
        if ordinal:
            command=row['input']['command'];reference=row['policy_entry_ref']
            if command['kind'] in ('cast_untargeted','cast_paid'):
                payment=facts['mana_payment'];require(payment is not None,'actual cast has native shared payment record')
                entry=payment['source'];require(entry not in entries,'actual source cast entry unique');entries[entry]=reference
                casts[reference]=dict(row=row,entry=entry,source=command['spell']['alias'],actor=mandatory_role(row['input']['actor']),payment=payment)
            matching=[plan for plan in plans.values() if reference==plan['id']+'.reply.'+row['input']['basis']['revision']]
            if matching:
                same(len(matching),1,'one actual source plan per native reply');plan=matching[0]
                controls[plan['id']]=dict(row=row,before=rows[ordinal-1],plan=plan)
        for record in facts['rules_journal']:
            tag=record['tag']
            if tag=='Event':
                require(record['event'] not in events,'actual event identity unique');events[record['event']]=record
                if record['cause']['tag']=='ResolutionCompleted':completions[record['cause']['entry']]={'Resolved':'resolved','AllTargetsIllegal':'all_targets_illegal_nonresolution'}[record['cause']['completion']]
                elif record['cause']['tag']=='CounterSpell':completions[record['cause']['target']]='countered'
                member_counts={}
                for occurrence in record['occurrences']:
                    value=occurrence['value'];kind=value['tag']
                    if kind=='CopyCreated':
                        same(len(matching),1,'actual copy birth is bound to its real source reply');plan=matching[0];same(plan['kind'],'copy_targets','actual copy birth source plan kind')
                        entries[value['entry']]=plan['bind_copy'];copies[plan['bind_copy']]=dict(row=row,value=value,plan=plan)
                    elif kind=='EntryCompleted':completions[value['entry']]=value['completion'].lower()
                    elif kind=='LifeChanged':
                        change=value['change'];life.append(dict(player=mandatory_role(change['player']),old=change['before'],new=change['after']))
                    elif kind=='TokenCreated':
                        cause=record['cause'];same(cause['tag'],'Instruction','actual optional token creating instruction');require(cause['entry'] in entries,'actual optional token has named source entry');creator=mandatory_role(value['creator']);coordinate=(cause['entry'],cause['instruction'],creator);member=member_counts.get(coordinate,0);member_counts[coordinate]=member+1;alias=token_birth_alias(entries[cause['entry']],cause['instruction'],creator,member);identity=value['after']['object']['object'];require(alias not in births and identity not in lineage,'actual unique optional token birth');lineage[identity]=alias;history[identity]=alias;births[alias]=dict(object=identity,appearance=value['after'],entry=cause['entry'],instruction=cause['instruction'],creator=creator,member=member,event=record['event'],ordinal=ordinal)
                    elif kind=='ZoneChange':
                        before=value['before']['object'];after=value['after']['object']
                        if before['object'] in lineage:lineage[after['object']]=lineage[before['object']];history[after['object']]=lineage[after['object']]
            elif tag=='TriggerDiscovered':triggers[record['value']['instance']]=dict(record['value'])
            elif tag=='TriggerPlaced':
                trigger=triggers[record['instance']];source=by_card[trigger['source']['object']['physical_card']]
                subjects=[]
                for member in trigger['causes']['members']:
                    related=member['related']
                    if related['tag']=='SpellCast':subjects.append(by_card[related['cast']['spell']['object']['physical_card']])
                    elif related['tag']=='ZoneChange':subjects.append(by_card[related['before']['object']['physical_card']] if related['before']['object']['physical_card'] is not None else lineage[related['before']['object']['object']])
                    elif related['tag']=='TokenCreated':subjects.append(lineage[related['after']['object']['object']])
                    else:raise EvidenceError('finite source trigger has unsupported related occurrence '+related['tag'])
                declared=[value for group in case['trigger_groups'] for value in group['triggers'] if reference_alias(value['source'])==source and value['captured_controller']==mandatory_role(trigger['controller']) and subjects==[reference_alias(value['subject'])] and str(value['definition_id'])==str(trigger['definition'])]
                same(len(declared),1,'actual native trigger/source/complete related subject label')
                name=declared[0]['name'];entries[record['entry']]=name;trigger['source_label']=source;trigger['entry']=record['entry'];trigger['name']=name;trigger['placement_observed_at_ordinal']=ordinal
        for draw in facts['draw_journal']:
            if draw['kind']=='drawn':draws.append(dict(player=mandatory_role(draw['player']),alias=draw['after']['alias']))
            else:failed.append(dict(player=mandatory_role(draw['player']),attempts=draw['attempts'],ordinal=ordinal))
    same(set(controls),set(plans),'actual all source plans have exactly one native accepted reply')
    for row,endpoint in mandatory_endpoints(run):worlds[row['ordinal']]=optional_physical_endpoint(run,row,endpoint,history,lineage)
    return dict(rows=rows,worlds=worlds,history=history,events=events,entries=entries,casts=casts,triggers=triggers,copies=copies,controls=controls,completions=completions,life=life,draws=draws,failed=failed,tokens=births)


def optional_callback_join(row:dict[str,Any],method:str,entries:dict[int,Any],returns:dict[int,Any])->tuple[dict[str,Any],dict[str,Any]]:
    call=entries[row['callback_id']];returned=returns[row['callback_id']]
    same((call['method'],call['actor'],call['global_sequence'],returned['global_sequence']),(method,row['actor'],row['entry_sequence'],row['return_sequence']),'actual receipt/callback method actor and causal edges')
    same(row['actual_ability']['entry_label'],row['entry_label'],'actual receipt/resolving entry label')
    same(call['arguments']['ability'],row['actual_ability'],'actual receipt/callback ability identity')
    return call,returned


def optional_no_offer_join(native:dict[str,Any],raw:dict[str,Any])->None:
    """Bind real all-targets-illegal settlements to absent offer publications."""
    require('resolution_payments' in raw,'actual no-offer proof requires complete payment receipt collection')
    illegal={entry for entry,value in native['completions'].items() if value=='all_targets_illegal_nonresolution'}
    if not illegal:return
    offered=set()
    for row in native['rows']:
        decision=row['header']['pending']
        if decision is not None and decision['frame']['kind']=='resolution_payment':
            entry=decision['frame']['frame']['entry'];require(entry not in illegal,'actual all-targets-illegal entry has no issued payment offer');offered.add(native['entries'][entry])
        for record in row['current_facts']['rules_journal']:
            if record['tag'] in ('OptionalEffectOutcome','ResolutionPaymentOutcome'):require(record['entry'] not in illegal,'actual all-targets-illegal entry has no control outcome')
    require(all(row['entry_label'] in offered for row in raw['resolution_payments']),'actual all-targets-illegal path has no invented payment callback/outcome')


def optional_settlement_cut(native:dict[str,Any],label:str,world:dict[str,Any])->tuple[int,dict[str,Any]]:
    """Map actual settled Ready only through its real same-cause placement replies."""
    completed=[row for row in native['rows'] if any(record['tag']=='Event' and record['cause']['tag']=='ResolutionCompleted' and native['entries'].get(record['cause']['entry'])==label for record in row['current_facts']['rules_journal'])];same(len(completed),1,'actual resolved settlement/native completed source entry');base=completed[0];ordinal=base['ordinal'];left=[native['entries'][entry['entry']] for entry in base['header']['execution']['stack']];right=[entry['ability']['entry_label'] for entry in world['stack']]
    if left==right:return ordinal,dict(kind='ActualCompletedCandidate',completion_ordinal=ordinal,observation_ordinal=ordinal)
    additions=[name for name in right if name not in left];triggers={value['name']:value for value in native['triggers'].values()};require(bool(additions) and all(name in triggers for name in additions),'settled Ready difference has actual newly placed source triggers');same([name for name in right if name not in additions],left,'settled Ready placement preserves complete underlying source stack');instances={triggers[name]['instance'] for name in additions};discovered={record['value']['instance'] for record in base['current_facts']['rules_journal'] if record['tag']=='TriggerDiscovered'};require(instances<=discovered,'settled Ready added triggers discovered in actual completing source call');pending=base['header']['pending'];require(pending is not None and pending['frame']['kind']=='triggers','settled Ready source completion retains actual pre-placement order frame');require(instances<=set(pending['frame']['frame']['domain']),'settled Ready instances belong to actual issued APNAP source domain')
    observed=max(triggers[name]['placement_observed_at_ordinal'] for name in additions);require(observed>ordinal,'settled Ready placement observation follows source completion');placed=[]
    for candidate in native['rows'][ordinal+1:observed+1]:
        before=native['rows'][candidate['ordinal']-1];issued=before['header']['pending'];command=candidate['input']['command'];require(issued is not None and command['kind']=='reply' and command['choice']['kind'] in ('order_triggers','trigger_target'),'settled Ready requires actual intervening APNAP/target reply');same(command['decision'],issued['id'],'settled Ready actual placement decision identity');same(candidate['input']['basis'],before['header']['basis'],'settled Ready actual placement source basis');same(candidate['input']['actor'],issued['chooser'],'settled Ready actual placement chooser')
        expected='triggers' if command['choice']['kind']=='order_triggers' else 'trigger_target';same(issued['frame']['kind'],expected,'settled Ready matching actual issued placement frame')
        if expected=='triggers':same(set(command['choice']['ordered']),set(issued['frame']['frame']['domain']),'settled Ready actual complete trigger-order domain');same(len(command['choice']['ordered']),len(issued['frame']['frame']['domain']),'settled Ready trigger order unique permutation')
        for record in candidate['current_facts']['rules_journal']:
            require(record['tag']!='Event','settled Ready placement-only chain has no primitive body transition')
            if record['tag']=='TriggerPlaced':
                require(record['instance'] in instances,'settled Ready actual placement belongs to completing cause');require(record['entry'] in native['entries'] and native['entries'][record['entry']] in triggers,'settled Ready actual placement entry known');same(triggers[native['entries'][record['entry']]]['instance'],record['instance'],'settled Ready actual placement entry/captured instance');placed.append(record['instance'])
    same(set(placed),instances,'settled Ready exact actual completing-cause placement inventory');same(len(placed),len(instances),'settled Ready each trigger actually placed once');same([native['entries'][entry['entry']] for entry in native['rows'][observed]['header']['execution']['stack']],right,'settled Ready exact post-order native/Forge source stack');require(native['rows'][observed]['header']['pending'] is None,'settled Ready mapped native boundary has no placement frame')
    return observed,dict(kind='ActualSameCauseAPNAPPlacementReplyChain',completion_ordinal=ordinal,observation_ordinal=observed,actual_placed_instances=placed,pre_placement_completed_candidate_stack_equality_claim=False)


def optional_common_stack_cut(native:dict[str,Any],raw:dict[str,Any],case:dict[str,Any],ordinal:int,world:dict[str,Any],cut_key:str)->dict[str,Any]:
    """Keep actual pre-placement Forge cuts distinct from post-placement native cuts."""
    require(all(row['entry'] in native['entries'] for row in native['rows'][ordinal]['header']['execution']['stack']),'actual source stack entry has actual named cast/copy/trigger placement')
    left=[native['entries'][row['entry']] for row in native['rows'][ordinal]['header']['execution']['stack']];right=[row['ability']['entry_label'] for row in world['stack']]
    if left==right:return dict(cut=cut_key,status='COMPLETE_ORDERED_STACK_EQUAL',entries=left)
    require(cut_key.startswith('action.'),'actual differing stack cut must be named cast action');slot=int(cut_key.removeprefix('action.'));policy=case['actions'][slot];same(policy['kind'],'cast','actual stack timing difference only after named cast');same(native['rows'][ordinal]['policy_entry_ref'],policy['policy_id'],'actual post-cast native source instruction')
    observation=next(row for row in raw['observations'] if row['key']==cut_key);sequence=observation['source_global_sequence'];source=next(row for row in raw['callbacks'] if row['global_sequence']==sequence);same((source['kind'],source['method']),('return','playChosenSpellAbility'),'actual Forge cast-return pre-placement cut');same(source['reply']['policy'],policy,'actual Forge cast-return complete source policy')
    deferred=[name for name in left if name not in right];require(bool(deferred),'actual native stack contains newly placed trigger');triggers={row['name']:row for row in native['triggers'].values()};reference=unique(raw['trigger_receipts'],'name','actual source trigger');entries={row['callback_id']:row for row in raw['callbacks'] if row['kind']=='entry'}
    same([name for name in left if name not in deferred],right,'actual pre/post placement cut preserves entire underlying ordered stack')
    for name in deferred:
        require(name in triggers and name in reference,'actual differing entry is known source trigger');trigger=triggers[name];same(trigger['source_label'],reference[name]['actual_ability']['source'],'actual deferred trigger captured source agrees with real Forge source');same(mandatory_role(trigger['controller']),reference[name]['captured_controller'],'actual deferred trigger captured controller');same(trigger['placement_observed_at_ordinal'],ordinal,'actual native trigger placement at this accepted cast');members=trigger['causes']['members'];same(len(members),1,'actual cast has singular captured occurrence');same(members[0]['related']['tag'],'SpellCast','actual added trigger caused by this cast');same(members[0]['related']['cast']['entry'],native['casts'][policy['policy_id']]['entry'],'actual captured source cast entry');require(entries[reference[name]['callback_id']]['global_sequence']>sequence,'actual Forge trigger placement callback follows retained pre-placement cast return')
    return dict(cut=cut_key,status='PHYSICAL_PLAYER_CALENDAR_CUT_WITH_EXPLICIT_TRIGGER_PLACEMENT_TIMING_DIFFERENCE',native_post_placement=left,forge_pre_placement=right,deferred_trigger_placements=deferred,complete_stack_equality_claim=False)


def observed_draw_before_life(draw_coordinates:list[int],life_coordinates:list[int],label:str)->None:
    """One shared publication-order guard, independent of source card names."""
    if draw_coordinates and life_coordinates:
        require(max(draw_coordinates)<min(life_coordinates),label+' draw publications precede life publications')


def optional_body_publications(native:dict[str,Any],raw:dict[str,Any]|None=None)->dict[str,Any]:
    """Observe actual source-entry/instruction/event order, without executing bodies.

    The whole native owner binds these immutable bodies to independent Programs.
    Literal linear primitive bodies have an exact publication denominator; other
    reached bodies retain instruction chronology and their existing CFG guards.
    Forge's private failed-draw cardinality is never manufactured.
    """
    bodies={};publications={};completions={};rows=native['rows']
    for row in rows:
        execution=row['header']['execution']
        values=[*execution['stack'],*([execution['resolving']] if execution['resolving'] is not None else [])]
        for value in values:
            if value['body'] is None:continue
            old=bodies.get(value['entry'])
            if old is not None:same(old['body'],value['body'],'actual immutable body publication owner')
            bodies[value['entry']]=value
    for trigger in native['triggers'].values():
        bodies.setdefault(trigger['entry'],dict(body=trigger['body'],controller=trigger['controller'],targets=[]))
    previous=-1
    for event in native['events'].values():
        number=integer(event['event'],'actual ordered body EventId');require(number>previous,'actual ordered body EventId chronology');previous=number;cause=event['cause']
        if cause['tag']=='ResolutionCompleted':completions[cause['entry']]=number
        origin=cause if cause['tag']=='Instruction' else cause.get('origin',{}) if cause['tag']=='Draw' else {}
        if origin.get('tag')!='Instruction':continue
        entry=origin['entry'];instruction=origin['instruction'] if type(origin['instruction']) is int else integer(origin['instruction'],'actual publication instruction');require(entry in bodies,'actual publication owns retained immutable body');body=bodies[entry]['body']['instructions'];require(0<=instruction<len(body),'actual publication instruction belongs to immutable body')
        for occurrence in event['occurrences']:
            value=occurrence['value'];family=value['tag'];pub=dict(event=number,occurrence=occurrence['id']['index'],instruction=instruction)
            if family=='LifeChanged':
                change=value['change'];same(body[instruction]['op'] in ('AdjustControllerLife','AdjustTargetLife'),True,'actual life publication source primitive');pub.update(family='life',player=mandatory_role(change['player']),delta=integer(change['after'],'actual life after')-integer(change['before'],'actual life before'),attempts=1)
            elif family=='FailedDraw':
                same(body[instruction]['op'],'DrawController','actual failed draw source primitive');pub.update(family='failed_draw',player=mandatory_role(value['player']),attempts=integer(value['attempts'],'actual failed attempts'))
            elif family=='ZoneChange' and value['before']['object']['zone']['kind']=='library' and value['after']['object']['zone']['kind']=='hand':
                same(body[instruction]['op'],'DrawController','actual successful draw source primitive');pub.update(family='draw',player=mandatory_role(value['after']['object']['owner']),alias=native['history'][value['after']['object']['object']],attempts=1)
            else:continue
            publications.setdefault(entry,[]).append(pub)
    result=[];forge_comparisons=[];linear={'DrawController','AdjustControllerLife','AdjustTargetLife'}
    for entry,value in bodies.items():
        pubs=publications.get(entry,[]);indices=[p['instruction'] for p in pubs];same(indices,sorted(indices),'actual source instruction publication chronology')
        if entry not in completions:continue
        completion=completions[entry]
        require(all(p['event']<completion for p in pubs),'actual ordered body publications precede completion')
        if pubs:
            first=pubs[0]['event'];require(not any(e['cause']['tag']=='StateBasedActions' and first<=integer(e['event'],'actual body SBA EventId')<completion for e in native['events'].values()),'actual ordered body completion precedes state-based checks')
        instructions=value['body']['instructions'];is_linear=all(i['op'] in linear for i in instructions)
        if is_linear:
            expected=[]
            for index,instruction in enumerate(instructions):
                selected=[p for p in pubs if p['instruction']==index]
                if instruction['op']=='DrawController':
                    same(instruction['amount']['tag'],'Literal','finite source draw publication amount');amount=integer(instruction['amount']['value'],'finite source draw publication denominator');same(sum(p['attempts'] for p in selected),amount,'actual linear body complete draw-attempt denominator');require(all(p['family'] in ('draw','failed_draw') and p['player']==mandatory_role(value['controller']) for p in selected),'actual source draw publication recipient');expected.extend([index]*amount)
                else:
                    same(len(selected),1,'actual linear body exact life publication denominator');adjustment=instruction['adjustment'];delta=integer(adjustment['amount'],'finite source life amount')*(1 if adjustment['tag']=='Gain' else -1)
                    if instruction['op']=='AdjustControllerLife':player=mandatory_role(value['controller'])
                    else:
                        targets=[t['target']['player'] for t in value['targets'] if t['target']['tag']=='Player'];same(len(targets),1,'actual source life target binding');player=mandatory_role(targets[0])
                    same((selected[0]['family'],selected[0]['player'],selected[0]['delta']),('life',player,delta),'actual source life publication operand/amount');expected.append(index)
            same([p['instruction'] for p in pubs for _ in range(p['attempts'])],expected,'actual linear source complete instruction publication order')
            draw_positions=[i for i,x in enumerate(instructions) if x['op']=='DrawController'];life_positions=[i for i,x in enumerate(instructions) if x['op']!='DrawController']
            if draw_positions and life_positions and max(draw_positions)<min(life_positions):observed_draw_before_life([p['event'] for p in pubs if p['family'] in ('draw','failed_draw')],[p['event'] for p in pubs if p['family']=='life'],'actual optional native source body')
        label=native['entries'].get(entry,entry);result.append(dict(entry=entry,label=label,linear_literal_primitive_body=is_linear,publications=pubs,completion_event=completion))
        if raw is None or entry not in {t['entry'] for t in native['triggers'].values()}:continue
        returns=[r for r in raw['callbacks'] if r['kind']=='return' and r['method']=='playSpellAbilityNoStack' and r['reply']['trigger']==label]
        if not returns:
            require(not is_linear and not pubs,'actual reached primitive trigger owns real Forge body callback');continue
        same(len(returns),1,'actual ordered source body callback completion');returned=returns[0];same(returned['reply']['body_complete'],True,'actual ordered source body completed');start=next(r for r in raw['callbacks'] if r['kind']=='entry' and r['callback_id']==returned['callback_id']);during=[e for e in raw['events'] if start['global_sequence']<e['global_sequence']<returned['global_sequence']]
        require(not any(e['kind']=='GameEventGameOutcome' for e in during),'actual Forge body completes before state-based loss');require(not any(c['kind']=='entry' and c['method'] in ('chooseSpellAbilityToPlay','orderAndPlaySimultaneousSa') and start['global_sequence']<c['global_sequence']<returned['global_sequence'] for c in raw['callbacks']),'actual Forge ordered body has no intervening priority')
        observed=[]
        for event in during:
            payload=event['payload']
            if event['kind']=='GameEventPlayerLivesChanged':observed.append(dict(family='life',player=payload['player'],delta=integer(payload['newLives'],'Forge body after')-integer(payload['oldLives'],'Forge body before')))
            elif event['kind']=='GameEventCardChangeZone' and payload['from'] and payload['to'] and payload['from']['zone_type']=='library' and payload['to']['zone_type']=='hand':observed.append(dict(family='draw',player=payload['to']['player'],alias=payload['card']['alias']))
        expected=[{k:v for k,v in p.items() if k in ('family','player','delta','alias')} for p in pubs if p['family']!='failed_draw'];same(observed,expected,'actual cross-family native Forge source-body publication chronology')
        forge_comparisons.append(dict(entry=label,callback_id=start['callback_id'],entry_sequence=start['global_sequence'],completion_sequence=returned['global_sequence'],ordered_successful_draw_life_publications=observed,private_failed_draw_cardinality_compared=False))
    return dict(native_bodies=result,forge_body_comparisons=forge_comparisons,authority='actual immutable source body instruction and journal/NoStack callback coordinates; no transition evaluation')


def optional_full_expected(compiled:dict[str,Any],case_id:str)->dict[str,Any]|None:
    """Select independent approved full-game facts from the pinned C03 input."""
    rows=unique(compiled['full_game_recipes']['cases'],'id','independent primary full games')
    return rows.get(case_id)


def optional_primary_compare(observed:dict[str,Any],expected:dict[str,Any],owner:str)->dict[str,Any]:
    """Compare every supplied observable primary field to its independent value."""
    same(set(observed),set(expected)-({'draw_failures'} if owner=='Forge' else set()),'complete independent primary observed fields '+owner)
    for key,value in observed.items():same(value,sorted(expected[key]) if key=='physical_graveyard_aliases' else expected[key],'independent full-game primary '+owner+' '+key)
    return dict(owner=owner,compared_fields=list(observed),actual=observed)


def optional_full_primary(native:dict[str,Any],compiled:dict[str,Any],case_id:str,raw:dict[str,Any]|None=None)->dict[str,Any]|None:
    """Consume fixed primary arithmetic over actual journals/callbacks/endpoints."""
    source=optional_full_expected(compiled,case_id)
    if source is None:return None
    expected=source['primary_expected'];fields=set('winner loser terminal_global_turn terminal_step life combat_damage_to_P1 unblocked_vanguard_hits casts ordinary_trigger_entries resolution_payments resolution_paid_mana optional_outcomes mana_color_replies successful_draws_excluding_opening remaining_library_cards physical_graveyard_aliases draw_failures mulligans cleanup_discards'.split())
    if 'mandatory_life_loss_P1' in expected:fields.add('mandatory_life_loss_P1')
    same(set(expected),fields,'complete independent full-game primary field inventory');same(expected['terminal_step'],'CombatDamage postdamage state-based loss before priority','approved primary terminal cut meaning')
    rows=native['rows'];last=rows[-1]['header'];world=native['worlds'][rows[-1]['ordinal']];events=list(native['events'].values());journals=[r for row in rows for r in row['current_facts']['rules_journal']];commands=[row['input']['command'] for row in rows[1:]];roles=('P0','P1');payments=[r for r in journals if r['tag']=='ResolutionPaymentOutcome'];optionals=[r for r in journals if r['tag']=='OptionalEffectOutcome']
    damages=[];losses=[];draws=[];cleanup=0;mandatory_loss=0
    attacking_aliases={alias for turn in source['turns'] for alias in turn['attackers']};physical={c['alias']:c for deck in source['decks'] for c in deck['physical_cards']};require(all(physical[a]['oracle_id']=='b3b9a87d-cb95-435c-90b6-037406cab32e' for a in attacking_aliases),'primary Vanguard hit source aliases have approved Oracle identity')
    trigger_entries={t['entry'] for t in native['triggers'].values() if all(i['op'] in ('DrawController','AdjustControllerLife','AdjustTargetLife') for i in t['body']['instructions'])}
    for event in events:
        cause=event['cause']
        for occurrence in event['occurrences']:
            value=occurrence['value'];tag=value['tag']
            if tag=='DamageDealt' and value['kind']=='Combat':
                for damage in value['damage']:
                    if damage['target']['kind']=='player' and mandatory_role(damage['target']['player'])=='P1' and integer(damage['amount'],'actual primary combat damage')>0:
                        alias=native['history'][damage['source']['object']['object']];require(alias in attacking_aliases,'primary combat hit has actual source-authored attacker');damages.append(dict(event=integer(event['event'],'actual damage EventId'),alias=alias,amount=integer(damage['amount'],'actual damage amount')))
            elif tag=='PlayerLost':losses.append(dict(event=integer(event['event'],'actual loss EventId'),player=mandatory_role(value['player']),reasons=value['reasons'],cause=cause))
            elif tag=='ZoneChange':
                before=value['before']['object']['zone'];after=value['after']['object']['zone']
                if before['kind']=='library' and after['kind']=='hand' and cause['tag']=='Draw' and cause['origin']['tag']!='Opening':draws.append(mandatory_role(after['player']))
                if before['kind']=='hand' and after['kind']=='graveyard' and cause['tag']=='CleanupDiscard':cleanup+=1
            elif tag=='LifeChanged' and cause['tag']=='Instruction' and cause['entry'] in trigger_entries and mandatory_role(value['change']['player'])=='P1':mandatory_loss+=max(0,integer(value['change']['before'],'actual mandatory life before')-integer(value['change']['after'],'actual mandatory life after'))
    same(last['turn']['position']['step'],'combat_damage','independent primary native terminal combat damage');same(last['priority_holder'],None,'independent primary native terminal before priority');same(len(losses),1,'independent primary native one actual terminal loss');same(losses[0]['reasons'],['NonpositiveLife'],'independent primary native terminal life cause');same(losses[0]['cause']['tag'],'StateBasedActions','independent primary native terminal SBA owner');require(bool(damages) and damages[-1]['event']<losses[0]['event'],'independent primary native damage before terminal loss')
    observed=dict(winner=mandatory_role(last['outcome']['seat']),loser=losses[0]['player'],terminal_global_turn=last['turn']['position']['number'],terminal_step=expected['terminal_step'],life={p:world['players'][p]['life'] for p in roles},combat_damage_to_P1=str(sum(d['amount'] for d in damages)),unblocked_vanguard_hits=len(damages),casts=len(native['casts']),ordinary_trigger_entries=len(native['triggers']),resolution_payments={k:sum(p['outcome']['tag']==k for p in payments) for k in ('Paid','Declined')},resolution_paid_mana={role:str(sum(integer(s['amount'],'actual resolution debit') for p in payments if p['outcome']['tag']=='Paid' and mandatory_role(p['payer'])==role for s in p['outcome']['payment']['spend'])) for role in roles},optional_outcomes={k:sum(o['outcome']==k for o in optionals) for k in ('Take','Decline')},mana_color_replies=sum(c['kind']=='reply' and c['choice']['kind']=='mana_color' for c in commands),successful_draws_excluding_opening={p:draws.count(p) for p in roles},remaining_library_cards={p:len(world['players'][p]['library']) for p in roles},physical_graveyard_aliases=sorted(a for a,c in world['cards'].items() if c['zone']=='graveyard'),draw_failures=sum(integer(d['attempts'],'actual failed draw attempts') for d in native['failed']),mulligans=sum(c['kind']=='reply' and c['choice']['kind']=='mulligan' and c['choice']['choice']=='mulligan' for c in commands),cleanup_discards=cleanup)
    if 'mandatory_life_loss_P1' in expected:observed['mandatory_life_loss_P1']=str(mandatory_loss)
    same(set(observed),set(expected),'all independent full-game primary fields consumed');result=dict(native=optional_primary_compare(observed,expected,'native'),reference=None,primary_case=case_id,source='pinned C03 full_game_recipes.cases[].primary_expected',qualification_credit=0)
    if raw is None:return result
    outcome=raw['actual_outcome'];final=raw['final_world'];reference_events=raw['events'];outcomes=[e for e in reference_events if e['kind']=='GameEventGameOutcome'];same(len(outcomes),1,'independent primary Forge terminal outcome publication');terminal=outcomes[0];same(terminal['context']['step'],'combat_damage','independent primary Forge terminal combat damage');same(outcome['players'][expected['loser']]['loss_reason'],'LifeReachedZero','independent primary Forge terminal life cause')
    damage=[e for e in reference_events if e['kind']=='GameEventPlayerDamaged' and e['payload']['combat'] and e['payload']['target']=='P1' and integer(e['payload']['amount'],'primary Forge damage')>0];require(bool(damage) and damage[-1]['global_sequence']<terminal['global_sequence'],'independent primary Forge damage before terminal outcome');require(not any(c['kind']=='entry' and c['method']=='chooseSpellAbilityToPlay' and damage[-1]['global_sequence']<c['global_sequence']<terminal['global_sequence'] for c in raw['callbacks']),'independent primary Forge terminal loss before priority');require(all(e['payload']['source']['alias'] in attacking_aliases for e in damage),'independent primary Forge source-authored Vanguard hit')
    opening=next(o['source_global_sequence'] for o in raw['observations'] if o['key']=='opening.complete');actual_draws=[e for e in reference_events if e['kind']=='GameEventCardChangeZone' and e['payload']['from'] and e['payload']['to'] and e['payload']['from']['zone_type']=='library' and e['payload']['to']['zone_type']=='hand' and e['global_sequence']>opening];returns=[c for c in raw['callbacks'] if c['kind']=='return'];cleanup_events=[e for e in reference_events if e['kind']=='GameEventCardChangeZone' and e['context']['step']=='cleanup' and e['payload']['from'] and e['payload']['to'] and e['payload']['from']['zone_type']=='hand' and e['payload']['to']['zone_type']=='graveyard']
    reference=dict(winner=outcome['winning_seat'],loser=next(p for p,v in outcome['players'].items() if v['lost']),terminal_global_turn=terminal['payload']['lastTurnNumber'],terminal_step=expected['terminal_step'],life={p:final['players'][p]['life'] for p in roles},combat_damage_to_P1=str(sum(integer(e['payload']['amount'],'primary Forge damage sum') for e in damage)),unblocked_vanguard_hits=len(damage),casts=len([c for c in raw['cast_operations']]),ordinary_trigger_entries=len(raw['trigger_receipts']),resolution_payments={k:sum(p['outcome']==k.lower() for p in raw['resolution_payments']) for k in ('Paid','Declined')},resolution_paid_mana={p:str(sum(len(r['spent']) for r in raw['resolution_payments'] if r['actor']==p)) for p in roles},optional_outcomes={k:sum(r['choice']==k.lower() for r in raw['optional_choices']) for k in ('Take','Decline')},mana_color_replies=sum(c['method']=='chooseColor' for c in returns),successful_draws_excluding_opening={p:sum(e['payload']['to']['player']==p for e in actual_draws) for p in roles},remaining_library_cards={p:final['header']['players'][p]['counts']['library'] for p in roles},physical_graveyard_aliases=sorted(a for a,c in final['cards'].items() if c['zone']=='graveyard'),mulligans=sum(c['method']=='mulliganKeepHand' and not c['reply']['keep'] for c in returns),cleanup_discards=len(cleanup_events))
    if 'mandatory_life_loss_P1' in expected:
        body_order=optional_body_publications(native,raw);reference['mandatory_life_loss_P1']=str(sum(max(0,-p['delta']) for body in body_order['forge_body_comparisons'] if next(b for b in body_order['native_bodies'] if b['label']==body['entry'])['linear_literal_primitive_body'] for p in body['ordered_successful_draw_life_publications'] if p['family']=='life' and p['player']=='P1'))
    result['reference']=optional_primary_compare(reference,expected,'Forge');result['reference_unavailable_fields']={'draw_failures':'Private attempt cardinality is not published by this Forge capture; the independent primary field is required and compared from actual native FailedDraw publications.'};return result


def optional_reduced_common(run:dict[str,Any],raw:dict[str,Any],case:dict[str,Any],inputs:dict[str,Any],compiled:dict[str,Any]|None=None)->dict[str,Any]:
    """Join actual full/reduced optional, payment, copy and common world observations."""
    same(run['fixture_ref'],case['case_id'],'actual native/Forge reduced case identity')
    entries=optional_forge_preflight(raw,case,inputs);returns={row['callback_id']:row for row in raw['callbacks'] if row['kind']=='return'}
    selected_forge_casts(raw,case,entries)
    opening_facts=selected_forge_opening(raw,case,entries,returns)
    native=optional_native_common_index(run,case);body_order=optional_body_publications(native,raw);require(compiled is not None or case['initialization']['kind']!='opening','full-game common joins require independent compiled primary facts');primary=optional_full_primary(native,compiled,case['case_id'],raw) if compiled is not None else None;optional_no_offer_join(native,raw);plans=unique(case['decision_plans'],'id','source decision plan');cut_count=0;stack_cut_dispositions=[];calendar_cut_dispositions=[]
    same(raw['decision_plan_consumption'],[name for name,actual in sorted(native['controls'].items(),key=lambda pair:pair[1]['row']['ordinal'])],'actual native/Forge chronological control plan consumption')
    def common_cut(ordinal:int,world:dict[str,Any],*,block_event:bool=False,cut_key:str='control')->None:
        nonlocal cut_count
        optional_common_world(native['worlds'][ordinal],optional_forge_world(world,raw,case),block_event=block_event);header=native['rows'][ordinal]['header'];position=header['turn']['position'] if header['turn'] is not None else None
        if position is not None:
            native_calendar=(position['number'],position['step'],mandatory_role(header['active']));forge_calendar=(world['header']['individual_turn'],world['header']['step'],world['header']['active'])
            if cut_key=='prepared.initialized' and case['initialization']['kind']=='scheduled' and case['initialization']['setup']['step']=='untap':
                setup=case['initialization']['setup'];same(ordinal,0,'actual initialized calendar difference only at constructor');same(forge_calendar,(setup['individual_turn'],'untap',setup['seed_active_role']),'actual Forge source scheduled pre-loop Untap');same(native_calendar,(setup['individual_turn'],'upkeep',setup['seed_active_role']),'actual native constructor completed source forced Untap');same([transition['to']['step'] for transition in native['rows'][0]['current_facts']['turn_transitions']],['untap','upkeep'],'actual native initializer forced Untap/Upkeep publication');calendar_cut_dispositions.append(dict(cut=cut_key,status='SOURCE_SCHEDULED_UNTAP_BEFORE_FORGE_LOOP_NATIVE_AFTER_FORCED_UNTAP',native=native_calendar,forge=forge_calendar,equal_calendar_claim=False))
            else:same(native_calendar,forge_calendar,'actual native/Forge complete calendar cut');calendar_cut_dispositions.append(dict(cut=cut_key,status='COMPLETE_CALENDAR_EQUAL',calendar=native_calendar))
        stack_cut_dispositions.append(optional_common_stack_cut(native,raw,case,ordinal,world,cut_key));cut_count+=1
    def no_priority(start:int,end:int,label:str)->None:
        require(not any(row['method'] in ('chooseSpellAbilityToPlay','orderAndPlaySimultaneousSa') and start<row['global_sequence']<end for row in entries.values()),'no ordinary priority/placement callback inside '+label)
    def current_plan(row:dict[str,Any])->tuple[dict[str,Any],dict[str,Any]]:
        require(row['plan_id'] in plans,'actual receipt has declared source plan');plan=plans[row['plan_id']];actual=native['controls'][row['plan_id']]
        same(row['entry_label'],optional_entry_name(plan['entry']),'actual source receipt resolving entry label')
        frame=actual['before']['header']['pending']['frame']['frame'];same(native['entries'][frame['entry']],row['entry_label'],'actual native issued entry/source/Forge label')
        same(str(frame['instruction']),str(plan['instruction']),'actual native source instruction origin')
        same(mandatory_role(actual['row']['input']['actor']),row['actor'],'actual native/Forge source-plan actor')
        return plan,actual
    optional_rows=unique(raw['optional_choices'],'plan_id','actual optional receipt');payments=unique(raw['resolution_payments'],'plan_id','actual payment receipt');copies=unique(raw['spell_copies'],'plan_id','actual copy receipt')
    same(set(optional_rows),{key for key,value in plans.items() if value['kind']=='optional'},'complete source/Forge optional receipts')
    same(set(payments),{key for key,value in plans.items() if value['kind']=='payment'},'complete source/Forge payment receipts')
    same(set(copies),{key for key,value in plans.items() if value['kind']=='copy_targets'},'complete source/Forge copy receipts')
    for receipt in optional_rows.values():
        plan,actual=current_plan(receipt);method={'trigger_optional':'confirmTrigger','draw_optional':'confirmAction'}[receipt['kind']]
        call,returned=optional_callback_join(receipt,method,entries,returns)
        same((receipt['actor'],receipt['choice']),(plan['chooser'],plan['choice']),'actual source optional chooser/outcome')
        same(returned['reply']['result'],receipt['choice']=='take','actual optional callback returned Boolean')
        same(returned['reply']['plan_id'],receipt['plan_id'],'actual optional returned plan binding')
        same(receipt['before_world'],receipt['after_world'],'actual optional input has no primitive side effects yet')
        same(actual['row']['input']['command']['choice'],dict(kind='optional_effect',choice=receipt['choice']),'actual native/Forge optional reply')
        frame=actual['before']['header']['pending']['frame']['frame'];same([x.lower() for x in frame['options']],plan['options'],'actual complete source/native optional domain')
        common_cut(actual['row']['ordinal']-1,receipt['before_world']);no_priority(receipt['entry_sequence'],receipt['return_sequence'],'optional callback')
    activations=unique(raw['mana_activations'],'operation_id','actual payment mana operation')
    expected_funding=[]
    short={'white':'W','blue':'U','black':'B','red':'R','green':'G','colorless':'C'}
    for receipt in payments.values():
        plan,actual=current_plan(receipt);call,returned=optional_callback_join(receipt,'payCostToPreventEffect',entries,returns)
        require('forge.game.ability.AbilityUtils.handleUnlessCost' in call['actual_stack_tags'],'actual resolution payment native Forge owner')
        same(receipt['actor'],plan['payer'],'actual source/native/Forge payment payer')
        same(receipt['quote']['generic'],plan['cost']['generic'],'actual source/Forge full generic payment quote')
        same(receipt['quote']['required'],plan['cost']['required'],'actual source/Forge full required payment quote')
        same(receipt['already_paid'],False,'actual selected offer has not already been paid');same(receipt['all_payers'],[receipt['actor']],'actual exact current payer collection')
        frame=actual['before']['header']['pending']['frame']['frame'];native_cost=frame['cost'];same(native_cost['generic'],receipt['quote']['generic'],'actual native/Forge quote generic')
        same(native_cost['required'],{},'selected source resolution costs are generic-only; zero/mixed/native-only scopes separate')
        before_candidates=[row for row in native['rows'] if row['ordinal']<=actual['row']['ordinal'] and row['header']['pending'] is not None and row['header']['pending']['frame']['kind']=='resolution_payment' and row['header']['pending']['frame']['frame']['entry']==frame['entry'] and row['header']['pending']['frame']['frame']['instruction']==frame['instruction']]
        require(bool(before_candidates),'actual first native owned payment cut present');common_cut(before_candidates[0]['ordinal'],receipt['before_world'])
        no_priority(receipt['entry_sequence'],receipt['return_sequence'],'resolution payment')
        fund_ids=[plan['id']+'.funding.'+str(index) for index in range(len(plan['funding']))];same(receipt['funding_operations'],fund_ids,'actual exact ordered source payment funding operations');expected_funding.extend(fund_ids)
        for index,funding in enumerate(plan['funding']):
            operation=activations[fund_ids[index]];same((operation['plan_id'],operation['parent_plan_id'],operation['entry_label'],operation['actor'],operation['parent_callback_id']),(plan['id'],plan['id'],receipt['entry_label'],receipt['actor'],receipt['callback_id']),'actual mana operation/owned payment parent')
            same(operation['source'],reference_alias(funding['source']),'actual source-qualified mana funding source')
            child=entries[operation['callback_id']];same(child['method'],'getCostDecisionMaker','actual nested mana cost callback, not invented activation callback')
            require(receipt['entry_sequence']<operation['operation_entry_sequence']<child['global_sequence']<operation['operation_return_sequence']<receipt['return_sequence'],'actual nested payment mana operation interval')
            native_rows=[row for row in run['accepted'] if row['policy_entry_ref']==plan['id']+'.fund.'+str(index)+'.reply.'+row['input']['basis']['revision']];same(len(native_rows),1,'one actual native payment mana prefix');prefix=native_rows[0]
            same(prefix['input']['command']['kind'],'activate_mana','actual native payment prefix command');same(prefix['input']['command']['source']['alias'],operation['source'],'actual native/Forge payment mana physical source')
            common_cut(prefix['ordinal']-1,operation['before_world'])
            produced=operation['produced'];same([x['production_operation_id'] for x in produced],[operation['operation_id']]*len(produced),'actual token production operation attribution')
            same([x['source'] for x in produced],[operation['source']]*len(produced),'actual token production source attribution')
            delta={key:str(integer(operation['pool_after'][key],'actual produced pool after')-integer(operation['pool_before'][key],'actual produced pool before')) for key in 'WUBRGC'}
            observed={key:str(sum(1 for token in produced if short[token['color']]==key)) for key in 'WUBRGC'};same(delta,observed,'actual production token/pool delta')
            for token in produced:same(token['source_timestamp'],operation['actual_ability']['source_timestamp'],'actual production source incarnation')
            complete=prefix
            if funding['kind']=='color':
                require(operation['color_callback_id'] is not None,'actual chosen-color callback required');color=entries[operation['color_callback_id']];color_return=returns[operation['color_callback_id']]
                same(color['method'],'chooseColor','actual color child callback kind');same(color['actor'],receipt['actor'],'actual color child actor')
                require('forge.game.ability.effects.ManaEffect.resolve' in color['actual_stack_tags'],'actual ManaEffect owns color callback')
                same(color['arguments']['pool'],operation['pool_before'],'actual color child has no premature production');same(color_return['reply']['pool'],operation['pool_before'],'actual color reply still precedes production')
                colors=[row for row in run['accepted'] if row['policy_entry_ref']==plan['id']+'.color.'+str(index)+'.reply.'+row['input']['basis']['revision']];same(len(colors),1,'one actual native color child completion');complete=colors[0]
                same(complete['input']['command']['choice']['mana_type'],funding['choice'].lower(),'actual source/native chosen color')
                same([token['color'] for token in produced],[funding['choice'].lower()]*integer(funding['amount'],'source chosen-color amount'),'actual source/Forge selected color production')
                common_cut(prefix['ordinal'],color['arguments']['before_world'])
                notifications=[row for row in entries.values() if row['method']=='notifyOfValue' and color['global_sequence']<row['global_sequence']<operation['operation_return_sequence'] and row['arguments']['ability']['ability_id']==operation['actual_ability']['ability_id']];same(len(notifications),1,'actual one color notification before production')
                same(notifications[0]['arguments']['value'],color_return['reply']['symbol'],'actual color notification/selected symbol')
            else:same(operation['color_callback_id'],None,'fixed output has no color child')
            common_cut(complete['ordinal'],operation['after_world'])
            native_production=complete['current_facts']['mana_production'];require(native_production is not None,'actual native prefix/color completion has production record')
            same({key:str(integer(value,'native production')) for key,value in native_production['produced'].items()},{color:str(sum(1 for token in produced if token['color']==color)) for color in dict.fromkeys(token['color'] for token in produced)},'actual native/Forge complete production amount/color')
        expected_outcome='paid' if plan['outcome']['kind']=='pay' else 'declined';same(receipt['outcome'],expected_outcome,'actual source/Forge payment outcome');same(returned['reply']['result'],expected_outcome=='paid','actual payment returned Boolean')
        same(returned['reply']['payment'],receipt['cost_payment_identity'],'actual returned genuine CostPayment identity')
        same(receipt['effect'],True if expected_outcome=='paid' else None,'actual effect flag exists only for genuine CostPayment')
        control=[row for row in actual['row']['current_facts']['rules_journal'] if row['tag']=='ResolutionPaymentOutcome'];same(len(control),1,'actual once-only native payment outcome')
        same(control[0]['outcome']['tag'],'Paid' if expected_outcome=='paid' else 'Declined','actual native/Forge payment branch')
        same(receipt['pool_before_debit'],{color:actual['before']['header']['mana'][str(actual['row']['input']['actor'])][long] for color,long in MANA.items()},'actual native/Forge payment post-prefix pool')
        same(receipt['pool_after'],{color:actual['row']['header']['mana'][str(actual['row']['input']['actor'])][long] for color,long in MANA.items()},'actual native/Forge payment after pool')
        if expected_outcome=='declined':same((receipt['spent'],receipt['cost_payment_identity'],receipt['pool_after']),([],None,receipt['pool_before_debit']),'actual declined payment retains all accepted mana without debit')
        else:
            require(receipt['cost_payment_identity'] is not None and receipt['cost_payment_identity']['actual_class']=='forge.game.cost.CostPayment','actual Paid has genuine CostPayment')
            owned_cost_calls=[row for row in entries.values() if receipt['entry_sequence']<row['global_sequence']<receipt['return_sequence'] and row['method'] in ('visit_CostPartMana','payManaCost','applyManaToCost') and row['arguments'].get('ability',{}).get('ability_id')==receipt['actual_ability']['ability_id']]
            same([row['method'] for row in owned_cost_calls],['visit_CostPartMana','payManaCost','applyManaToCost'],'actual genuine resolution CostPayment owner chain')
            for owned in owned_cost_calls:
                same(owned['arguments']['effect'],True,'actual resolving cost owner uses effect=true')
                same(owned['actor'],receipt['actor'],'actual resolving cost owner payer')
            for owned in owned_cost_calls[:2]:
                scope=owned['arguments']['payment_scope' if owned['method']=='visit_CostPartMana' else 'scope']
                same(scope['payment'],receipt['cost_payment_identity'],'actual cost visitor references offer CostPayment')
                for field in ('stack_part_is_actual_part','payment_ability_is_actual_offer','payment_is_offer_payment'):same(scope[field],True,'actual resolution owner identity '+field)
                same(scope['actual_part'],receipt['quote']['actual_part'],'actual quote/cost part identity')
            actual_debit_reply=returns[owned_cost_calls[-1]['callback_id']]['reply'];same(actual_debit_reply['spent'],receipt['spent'],'actual payment callback/receipt complete token debit rows')
            payment=control[0]['outcome']['payment'];normalized={row['type'].lower().replace('colourless','colorless'):integer(row['amount'],'actual native Paid spend') for row in payment['spend']}
            observed={color:sum(1 for row in receipt['spent'] if row['token']['color']==color) for color in dict.fromkeys(row['token']['color'] for row in receipt['spent'])};same(normalized,observed,'actual normalized native spend/Forge token debit color amounts')
            same(receipt['spent'][-1]['unpaid_after'],'0','actual genuine payment leaves no unpaid cost')
            for spent in receipt['spent']:require(spent['token']['production_operation_id'] in fund_ids,'actual debit token came from accepted payment funding')
    same(set(activations),set(expected_funding),'exact no extra/omitted payment mana operations')
    for receipt in copies.values():
        plan,actual=current_plan(receipt);call=entries[receipt['callback_id']];returned=returns[receipt['callback_id']]
        same((call['method'],call['actor'],call['global_sequence'],returned['global_sequence']),('orderAndPlaySimultaneousSa',receipt['actor'],receipt['entry_sequence'],receipt['return_sequence']),'actual copy owner callback/receipt interval')
        require('forge.game.ability.effects.CopySpellAbilityEffect.resolve' in call['actual_stack_tags'],'actual native Forge copy owner')
        same(len(call['arguments']['domain']),1,'actual one offered source copy');same(call['arguments']['domain'][0]['ability_id'],receipt['copy_ability']['ability_id'],'actual offered/installed copy ability identity')
        target_calls=[row for row in entries.values() if row['method']=='chooseNewTargetsFor' and receipt['entry_sequence']<row['global_sequence']<receipt['return_sequence']];same(len(target_calls),1,'actual copy owner has one target child')
        target_call=target_calls[0];target_return=returns[target_call['callback_id']]
        same(target_call['actor'],receipt['actor'],'actual copy target child actor');same(target_call['arguments']['ability']['ability_id'],receipt['copy_ability']['ability_id'],'actual copy target child identity')
        same((target_call['arguments']['filter_is_null'],target_call['arguments']['optional']),(True,True),'actual CopySpell setupNewTargets domain')
        same(target_return['reply']['choice'],receipt['choice'],'actual returned copy target choice')
        same(target_return['reply']['actual_ability']['targets'],receipt['targets_after'],'actual target child return/installed targets')
        common_cut(actual['row']['ordinal']-1,receipt['before_world'])
        same(receipt['copy_label'],plan['bind_copy'],'actual source/new copy label');same(receipt['original_entry_label'],optional_entry_name(plan['original']),'actual copied original source entry')
        same(receipt['choice'],plan['choice']['kind'],'actual source/Forge copy target choice')
        copy=native['copies'][plan['bind_copy']];same(native['entries'][copy['value']['original']],receipt['original_entry_label'],'actual native/Forge copied original entry')
        same(mandatory_role(copy['value']['controller']),receipt['actor'],'actual native/Forge new copy controller')
        require(receipt['copy_ability']['copied'] is True and receipt['copy_ability']['synthetic_copy_host'] is True,'actual Forge copied ability and synthetic host distinction')
        same(receipt['installed_copy_ability']['targets'],receipt['targets_after'],'actual installed copy targets equal returned target choice')
        same(receipt['installed_copy_ability']['ability_id'],receipt['copy_ability']['ability_id'],'actual copy choice/installation ability identity')
        stack_events=[event for event in raw['events'] if event['global_sequence']==receipt['copy_stack_add_sequence']];same(len(stack_events),1,'actual copy installation event present')
        additions=[row for row in raw['raw_stack_additions'] if row['actual_stack_id']==receipt['copy_stack_id']];same(len(additions),1,'actual raw copied stack addition unique');same(additions[0]['classification'],'copy_addition','copy raw event is never a cast')
        require(not any(row['ability_id']==receipt['copy_ability']['ability_id'] for row in raw['cast_operations']),'actual direct copy creates no cast operation')
        if receipt['choice']=='retain':same(receipt['targets_before'],receipt['targets_after'],'actual retained copy target choices')
        native_targets=copy['value']['targets'];same(len(native_targets),len(receipt['targets_after']),'actual copied target cardinality')
        for left,right in zip(native_targets,receipt['targets_after'],strict=True):
            same(left['target']['tag'],'Stack','selected copied target family');same(native['entries'][left['target']['entry']],right.get('entry_label',next((row['actual_ability']['entry_label'] for row in raw['cast_operations'] if row['ability_id']==right['ability_id']),None)),'actual native/Forge copied target exact current entry')
    forge_casts=unique(raw['cast_operations'],'action_id','actual source cast operation');same(set(forge_casts),set(native['casts']),'complete native/Forge actual cast inventory')
    for name,left in native['casts'].items():
        right=forge_casts[name];same((left['source'],left['actor']),(right['source'],right['payer']),'actual native/Forge cast source/controller')
        same(cost_tuple(right['required_cost']),left['payment']['cost'],'actual complete native/Forge printed casting cost')
        same(sum(integer(row['amount'],'actual cast spend') for row in left['payment']['spend']),len(right['paid_sources']),'actual native/Forge cast debit quantity')
    declared={row['name']:row for group in case['trigger_groups'] for row in group['triggers']};actual_triggers=unique(raw['trigger_receipts'],'name','actual Forge trigger receipt');same(set(actual_triggers),set(declared),'complete declared/Forge trigger inventory');same({row['name'] for row in native['triggers'].values()},set(declared),'complete source/native trigger inventory')
    for trigger in native['triggers'].values():
        right=actual_triggers[trigger['name']];selected_forge_trigger_callback(raw,right,declared[trigger['name']],next(group for group in case['trigger_groups'] if trigger['name'] in [row['name'] for row in group['triggers']]),entries,returns,case['player_choices']);same((trigger['source_label'],mandatory_role(trigger['controller'])),(right['actual_ability']['source'],right['captured_controller']),'actual native/Forge captured trigger source/controller')
        members=trigger['causes']['members'];same(len(members),1,'actual source EachOccurrence retains complete one-member cause')
        member=members[0];event=native['events'][trigger['causes']['event']];occurrences=[row for row in event['occurrences'] if row['id']==member['occurrence']];same(len(occurrences),1,'actual captured cause occurrence joins full native publication')
        if member['related']['tag']=='SpellCast':
            cast=member['related']['cast'];same(native['entries'][cast['entry']],right['causal_events'][0]['actual_ability']['entry_label'],'actual complete cast trigger cause/current caster entry');same(mandatory_role(member['related_player']),right['causal_events'][0]['caster'],'actual retained related caster player')
        else:require(any(row['subject']==right['subject'] for row in right['causal_events']),'actual death/entry trigger subject in complete Forge causal event set')
    token_births=selected_forge_token_births(raw,forge_casts,actual_triggers);join_common_token_births(native,token_births)
    combat_facts=optional_declaration_damage_join(native,raw,case,entries)
    expected_settlements={label:native['completions'][entry] for entry,label in native['entries'].items() if entry in native['completions'] and (label in native['casts'] or label in native['copies'])}
    same(raw['settlements'],expected_settlements,'complete actual native/Forge source-entry settlements')
    observation_cuts=[];checkpoint_refs=set()
    for observation in raw['observations']:
        same(observation['scope'],'full_checkpoint','actual complete Forge observation scope')
        checkpoint=observation['checkpoint_ref'];require(checkpoint in raw['checkpoints'],'actual observation owns existing full Forge checkpoint');checkpoint_refs.add(checkpoint)
        world=raw['checkpoints'][checkpoint];optional_forge_world(world,raw,case)
        key=observation['key'];ordinal=None;reason=None;cut_relation=None
        if key=='prepared.initialized':ordinal=0
        elif key=='opening.complete':
            matches=[row for row in native['rows'] if row['header']['opening'] is not None and row['header']['opening']['complete']];require(bool(matches),'actual completed opening native cut');ordinal=matches[0]['ordinal']
        elif key in ('terminal','terminal.return'):ordinal=len(run['accepted'])
        elif key.startswith('action.'):
            evidence=observation['evidence']
            if 'action' in evidence:
                policy=evidence['action']['policy_id'];matches=[row for row in run['accepted'] if row['policy_entry_ref']==policy]
            else:
                family='declare_attackers' if 'attackersMap' in evidence else 'declare_blockers'
                matches=[row for row in run['accepted'] if row['input']['command']['kind']=='reply' and row['input']['command']['choice']['kind']==family and row['header']['turn']['position']['number']==world['header']['individual_turn']]
                same(len(matches),1,'actual source combat observation/native accepted declaration')
                actual=matches[0]['input']['command']['choice']
                if family=='declare_attackers':
                    same(mandatory_role(matches[0]['input']['actor']),evidence['player'],'actual native/Forge attacker chooser')
                    same([(row['attacker']['alias'],mandatory_role(row['defender'])) for row in actual['attackers']],[(row['attacker']['alias'],row['defender']['seat']) for row in evidence['attackersMap']],'actual complete native/Forge attacker declaration')
                else:
                    same(mandatory_role(matches[0]['input']['actor']),evidence['defendingPlayer'],'actual native/Forge blocker chooser')
                    same([(row['attacker']['alias'],row['blocker']['alias']) for row in actual['blockers']],[(row['attacker']['alias'],row['value']['alias']) for row in evidence['blockers'] if not row['unblocked_self_sentinel']],'actual complete native/Forge blocker declaration')
            same(len(matches),1,'actual source action observation/native accepted input');ordinal=matches[0]['ordinal']
        elif key=='prepared.first_ready' or key.startswith('priority.'):
            header=world['header'];matches=[row for row in native['rows'] if row['header']['turn'] is not None and row['header']['turn']['position']['number']==header['individual_turn'] and row['header']['turn']['position']['step']==header['step'] and row['header']['priority_holder'] is not None and mandatory_role(row['header']['priority_holder'])==header['raw_priority']]
            if matches:ordinal=matches[0]['ordinal']
            else:reason='No actual native input boundary at this reference calendar/priority cut'
        elif key.startswith('settle.') and observation['evidence']['disposition']=='resolved':
            label=key.removeprefix('settle.');ordinal,cut_relation=optional_settlement_cut(native,label,world)
        elif key=='bounded.stop':ordinal=len(run['accepted'])
        else:reason='Reference internal event/body/counter-target cut has no separately returned native input-boundary GameState'
        if ordinal is not None:
            is_block=key.startswith('action.') and 'blockers' in observation['evidence'];common_cut(ordinal,world,block_event=is_block,cut_key=key);observation_cuts.append(dict(key=key,native_ordinal=ordinal,status='COMMON_PHYSICAL_PLAYER_CUT_COMPARED',native_cut_relation=cut_relation))
        else:observation_cuts.append(dict(key=key,status='REFERENCE_INTERNAL_CUT_NATIVE_FULL_STATE_UNAVAILABLE',reason=reason))
    same(checkpoint_refs,set(raw['checkpoints']),'actual every full reference checkpoint has source observation')
    common_cut(len(run['accepted']),raw['final_world'])
    same(len(native['rows'][-1]['header']['execution']['stack']),raw['final_world']['header']['stack_size'],'actual settled stack count including copies')
    forge_life=[dict(player=row['payload']['player'],old=row['payload']['oldLives'],new=row['payload']['newLives']) for row in raw['events'] if row['kind']=='GameEventPlayerLivesChanged'];same(native['life'],forge_life,'actual complete ordered native/Forge life publications')
    forge_draws=[dict(player=row['payload']['to']['player'],alias=row['payload']['card']['alias']) for row in raw['events'] if row['kind']=='GameEventCardChangeZone' and row['payload']['from'] is not None and row['payload']['to'] is not None and row['payload']['from']['zone_type']=='library' and row['payload']['to']['zone_type']=='hand'];same(native['draws'],forge_draws,'actual complete ordered native/Forge successful draws')
    if native['failed']:
        same(len(native['failed']),1,'actual selected single failed-draw instruction');same(native['failed'][0]['attempts'],'1','actual selected one failed draw attempt')
        player=native['failed'][0]['player'];same(raw['final_world']['players'][player]['lost'],True,'actual failed draw produces observed player loss');same(raw['final_world']['players'][player]['loss_reason'],'Milled','actual Forge empty-library loss reason')
    outcome=run['final'].get('outcome');actual_outcome=raw['actual_outcome']
    if outcome is None:same(actual_outcome,None,'actual optional bounded native/Forge nonterminal outcome')
    else:
        same(outcome['kind'],'winner','actual natural native outcome class');same(mandatory_role(outcome['seat']),actual_outcome['winning_seat'],'actual native/Forge natural winner');same({mandatory_role(seat) for seat in native['rows'][-1]['header']['losses']},{role for role,value in actual_outcome['players'].items() if value['lost']},'actual native/Forge complete loser partition');same(raw['completion']['natural_start_game_return'],case['initialization']['kind']=='opening','actual optional natural start-game source loop returned');same(raw['completion']['natural_prepared_loop_return'],case['initialization']['kind']=='scheduled','actual optional natural prepared source loop returned');same(raw['completion']['game_finished_event'],case['initialization']['kind']=='opening','actual optional source-owned Match GameFinished event disposition');same(raw['completion']['game_over'],True,'actual optional natural game over')
    return dict(status='ACTUAL_OPTIONAL_COMMON_JOINS_PASS',case=case['case_id'],depth=raw['completion']['kind'],common_world_cuts=cut_count,physical_cards=len(native['worlds'][0]['cards']),casts=len(forge_casts),triggers=len(actual_triggers),token_births=len(token_births),opening=opening_facts,combat_and_damage=combat_facts,ordered_body_publications=body_order,independent_primary_values=primary,natural_outcome_equal=outcome is not None,optional_controls=len(optional_rows),payment_controls=len(payments),mana_prefixes=len(activations),copies=len(copies),failed_draws=len(native['failed']),source_observation_cut_dispositions=observation_cuts,stack_cut_dispositions=stack_cut_dispositions,calendar_cut_dispositions=calendar_cut_dispositions,full_source_value_authority='independent primary19/two-template table',full_wave_qualification=False,globally_fully_qualified_cards=0)



def main()->None:
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--reference-run',type=Path);parser.add_argument('--case',action='append');parser.add_argument('--native',type=Path,action='append',default=[]);parser.add_argument('--native-binding',type=Path);parser.add_argument('--supplemental-binding',type=Path);parser.add_argument('--allow-uncaptured',action='store_true');parser.add_argument('--out',type=Path,required=True)
    parser.add_argument('--profile',choices=PROFILES,default=PROFILES[0]);parser.add_argument('--primary-index',type=Path);parser.add_argument('--compiled-input',type=Path);parser.add_argument('--requirements',type=Path)
    parser.add_argument('--optional-admission',type=Path)
    args=parser.parse_args()
    if args.profile==OPTIONAL_PROFILE:
        if args.optional_admission is None:parser.error('optional-payment-v1 requires --optional-admission from actual frozen current26 acceptance')
        if any((args.reference_run,args.case,args.native,args.native_binding,args.supplemental_binding,args.allow_uncaptured,args.primary_index,args.compiled_input,args.requirements)):parser.error('optional admission envelope owns exact26 inputs; unrelated/partial/uncaptured options forbidden')
        import interaction_reference_optional_admission as optional_admission
        result=optional_admission.compare_final26(__import__(__name__),args.optional_admission)
        with args.out.open('x') as stream:json.dump(result,stream,indent=2);stream.write('\n')
        print(json.dumps(dict(status=result['status'],cases=result['case_count'],final_image_adopted=False)));return
    if args.optional_admission is not None:parser.error('optional authority envelope is outside historical Blue/mandatory grammar')
    if args.reference_run is None:parser.error('historical Blue/mandatory requires --reference-run')
    explicit=[args.primary_index,args.compiled_input,args.requirements]
    if args.profile==PROFILES[1]:
        if not all(explicit) or args.native_binding is None or args.supplemental_binding is None:parser.error('mandatory-trigger-v1 requires explicit --primary-index, --compiled-input, --requirements, --native-binding and --supplemental-binding')
        if args.allow_uncaptured:parser.error('mandatory-trigger-v1 forbids --allow-uncaptured')
        result=compare_mandatory_reference(args.reference_run,args.primary_index,args.compiled_input,args.requirements,args.case);binding_before=sha(args.native_binding);supplemental_before=sha(args.supplemental_binding);source=read(args.reference_run/'authority-binding.json');authority=mandatory_authorities(Path(source['source_root']),args.primary_index,args.compiled_input,args.requirements);result['native_join']=compare_mandatory_native_files(args.reference_run,args.native,read(args.native_binding),read(args.supplemental_binding),authority,args.case);same(sha(args.native_binding),binding_before,'actual native binding stable during read');same(sha(args.supplemental_binding),supplemental_before,'actual supplemental binding stable during read');result['status']=result['native_join']['status'];result['native_numeric_agreement_claim']=result['native_join']['final_image_adopted']
        with args.out.open('x') as stream:json.dump(result,stream,indent=2);stream.write('\n')
        print(json.dumps(dict(status=result['status'],cases=result['case_count'],native_numeric_agreement_claim=result['native_numeric_agreement_claim'])));return
    if any(explicit):parser.error('mandatory authorities are outside the Blue grammar')
    result=compare_reference(args.reference_run,args.case)
    require(args.supplemental_binding is None or args.native_binding is not None,'supplemental binding requires native image binding')
    if args.native or args.native_binding:
        binding_sha=sha(args.native_binding) if args.native_binding else None
        binding=read(args.native_binding) if args.native_binding else None
        supplemental_sha=sha(args.supplemental_binding) if args.supplemental_binding else None;supplemental=read(args.supplemental_binding) if args.supplemental_binding else None
        result['native_join']=compare_native_files(args.reference_run,args.native,args.allow_uncaptured,binding,args.case,supplemental)
        if args.native_binding:
            same(sha(args.native_binding),binding_sha,'actual native image index stable during join');result['native_join']['native_image_binding_file']=dict(path=str(args.native_binding.resolve()),sha256=binding_sha)
        if args.supplemental_binding:
            same(sha(args.supplemental_binding),supplemental_sha,'actual supplemental index stable during join');result['native_join']['supplemental_binding_file']=dict(path=str(args.supplemental_binding.resolve()),sha256=supplemental_sha)
        result['status']=result['native_join']['status']
        result['native_numeric_agreement_claim']=result['native_join']['final_image_adopted']
    with args.out.open('x') as stream:json.dump(result,stream,indent=2);stream.write('\n')
    print(json.dumps(dict(status=result['status'],cases=result['case_count'],native_numeric_agreement_claim=result['native_numeric_agreement_claim'])))


if __name__=='__main__':main()
