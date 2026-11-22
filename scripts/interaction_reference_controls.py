#!/usr/bin/env python3
"""Exercise the selected profile's finite corruption classes through actual reader substrates and branch copies."""
from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import tempfile
from pathlib import Path
from typing import Any, Callable

import interaction_reference_compare as compare_reader
from interaction_reference_compare import (
    EvidenceError, evidence_boundary, read, require, same, pointer, forge_projection, assert_expected_facts,
    bound_file, equal_api_bytes, mandatory_native_projection, mandatory_native_primary_facts, compare_mandatory_native_files, join_mandatory_common, validate_token_observer,
    compare_reference, compare_mandatory_reference, mandatory_forge_projection, mandatory_primary_facts, mandatory_authorities, validate_runtime, validate_mandatory_process, PROFILES, native_projection, unwrap_native, validate_image_binding, validate_captured_sources, join_native_reference, sha,
)

DELETE=object()

# Atomic obligations expand the frozen descriptions without changing them. A
# passing class label is never evidence for an unexecuted member of this list.
MANDATORY_VARIANTS:dict[str,tuple[str,...]]={
    'RC.case-inventory':('reference_remove','reference_duplicate','reference_foreign','index_remove','index_duplicate','index_foreign','native_case_remove','native_case_duplicate','native_case_foreign'),
    'RC.completion':('natural_to_ready','intents_omitted','native_incomplete','bounded_phase','bounded_boundary'),
    'RC.winner-loss-partition':('forge_winner','forge_losers','native_final','native_header','native_losses','coherent_native_outcome'),
    'RC.source-authority':('compressed_source','metadata','vocabulary','publication','primary','compiled_input','selected_source_record'),
    'RC.program-definition':('paragraph_removed','paragraph_reordered','whole_program','default_definition','typed_definition','printed_mana'),
    'RC.physical-inventory':('alias_duplicate','alias_omitted','alias_foreign','source_foreign','owner','controller','global_order','pile_order','owner_counts','controller_counts'),
    'RC.opening-order':('starter_first','actual_chooser'),
    'RC.paid-source-cost':('mana_source','source_incarnation','mana_type','mana_amount','spend','cast_funding'),
    'RC.cast-stack-binding':('source_incarnation','entry','target_relation','counter_disposition'),
    'RC.entry-filter':('orator_invented','warden_omitted','self_exclusion'),
    'RC.simultaneous-cohort':('birth_omitted','batch_split','instance_omitted'),
    'RC.trigger-definition':('definition','source','controller'),
    'RC.trigger-complete-causes':('member_omitted','member_duplicated','member_replaced','event_incarnation'),
    'RC.apnap-order':('controller_order','member_order','installed_stack_order'),
    'RC.provisional-install':('body_only','binding_only','body_and_binding','installed_relation','cursor_omitted','cursor_entry','cursor_instruction','header_instruction_index','waiting_omitted','waiting_duplicated','order_waiting_omitted','order_waiting_duplicated','stack_source_relation'),
    'RC.trigger-player-target':('selected_player','outer_chooser','inner_chooser','inner_chooser_omitted','inner_event_omitted','inner_boundary_omitted','domain','provisional_target','installed_target'),
    'RC.graveyard-order':('artist_dissenter_order','singleton_prompt'),
    'RC.departed-trigger':('recast_incarnation','captured_draw_cancelled'),
    'RC.instruction-life-draw-order':('draw_loss_order','target_loss_gain_order','draw_omitted','life_omitted','intermediate_sba','branch_forbidden','jump_forbidden','creature_type_choice_forbidden'),
    'RC.failed-draw-count':('attempt_omitted','body_completion_omitted','failure_partition'),
    'RC.death-birth-entry-chain':('creation_omitted','watcher_omitted','body_priority'),
    'RC.token-template':('name','color','power','toughness','subtypes','mana','keywords','activated','triggered','mana_abilities','continuous','creator','controller'),
    'RC.token-birth-lineage':('entry','instruction','cohort','member','birth_identity','incarnation'),
    'RC.token-stored-marks':('field_omitted','nonzero_to_zero'),
    'RC.token-stored-counters':('field_omitted','nonzero_to_zero'),
    'RC.token-endpoint-delta':('added_omitted','added_after','updated_omitted','updated_before','updated_after','removed_omitted','removed_before','identity','required_empty_omitted'),
    'RC.token-transient-event':('death_omitted','graveyard_appearance','cessation_omitted','before_incarnation'),
    'RC.creature-mana-age':('young','stale','next_turn_omitted','printed_one_green'),
    'RC.decision-authority':('error_omitted','error_type','prefix','rejected_adopted','continuation_omitted'),
    'RC.checkpoint-replay':('checkpoint_omitted','checkpoint_tokens','journal_mismatch','input_mismatch','final_state_mismatch','initialization_checkpoint','checkpoint_replay_duplicate','probe_replay_duplicate'),
    'RC.native-witness-bindings':('point_basis','point_context','point_vocabulary','point_pointer','point_source_owner','call_actor','call_input','call_result_basis','call_adoption','call_append','call_before','call_after','call_owner_method','call_error_leaf_domain','source_digest','source_omitted','source_empty','source_foreign','record_pointers_empty','coverage_incomplete','vocabulary','image','registry','profile','current_schema','witness_schema'),
    'RC.reference-runtime-bindings':('producer','class','resource','loaded_origin','compile_process','run_process','stdout','stderr','interrupted','evidence_digest','prepost_guard'),
    'RC.old-api-bytes':('field_order','field_name','field_omitted','field_content'),
    'RC.blue-reference-preservation':('winner','draw','blocked','field_omitted'),
}


def named_variant(row:dict[str,Any],variant:str,expected_guard:str|tuple[str,...])->dict[str,Any]:
    """Credit a named obligation only after its intended guard rejected it."""
    require(variant in MANDATORY_VARIANTS.get(row['id'],()),'known explicit mandatory variant '+row['id']+'/'+variant)
    guards=(expected_guard,) if isinstance(expected_guard,str) else expected_guard
    require(bool(guards) and all(guard for guard in guards),'nonempty intended variant guards')
    require(any(guard in row['reason'] for guard in guards),'named variant reached intended guard '+row['id']+'/'+variant+': '+row['reason'])
    return dict(row,variant_id=variant,intended_guards=list(guards))


def mandatory_variant_dispositions(needs:dict[str,Any],rows:list[dict[str,Any]],complete:bool)->list[dict[str,Any]]:
    """Account for every frozen class and each independently executed variant.

    Untagged historical component controls remain additional evidence; they grant
    no named-variant credit. Missing variants remain explicit in component output
    and reject a final completion claim. Duplicate exercises cannot fill a gap.
    """
    classes=needs['reader_control_classes']
    same(set(MANDATORY_VARIANTS),{row['id'] for row in classes},'exact frozen34 explicit variant classes')
    for row in rows:
        if 'variant_id' not in row:continue
        require(row['id'] in MANDATORY_VARIANTS and row['variant_id'] in MANDATORY_VARIANTS[row['id']],'known exercised variant')
        same((row['status'],row['baseline_passed'],row['restoration_passed']),('EXERCISED_REJECTED',True,True),'effective named variant proof')
        require(bool(row.get('intended_guards')),'named variant must bind intended rejection guard')
        require(isinstance(row.get('reason'),str) and any(isinstance(guard,str) and bool(guard) and guard in row['reason'] for guard in row['intended_guards']),'named variant rejection must match an intended guard')
    output=[]
    for control in classes:
        exercises=[row for row in rows if row['id']==control['id']]
        variants=[]
        for name in MANDATORY_VARIANTS[control['id']]:
            actual=[row for row in exercises if row.get('variant_id')==name]
            variants.append(dict(id=name,status='EXERCISED_REJECTED' if actual else 'NOT_EXERCISED',actual_exercises=len(actual),scopes=[row['scope'] for row in actual]))
        missing=[row['id'] for row in variants if row['status']=='NOT_EXERCISED']
        if complete:require(not missing,'mandatory variants remain unexercised '+control['id']+': '+','.join(missing))
        output.append(dict(id=control['id'],proof_depth=control['proof_depth'],status='EXERCISED_REJECTED' if not missing else 'PARTIAL_VARIANTS_PENDING',actual_exercises=len(exercises),required_mutations=control['required_mutations'],variants=variants,missing_variants=missing,remaining_prerequisites=control['prerequisite'] if missing else None))
    same(len(output),34,'all34 finite required control dispositions')
    return output


def replace(document:Any,path:list[Any],value:Any)->Any:
    """Persistent path replacement: copy ancestors only; unchanged full witness values are shared."""
    if not path:return value
    key=path[0];out=dict(document) if isinstance(document,dict) else list(document)
    if len(path)==1:
        if value is DELETE:
            if isinstance(out,list):out.pop(key)
            else:del out[key]
        else:out[key]=value
    else:out[key]=replace(document[key],path[1:],value)
    return out


def changed(document:Any,patches:list[tuple[list[Any],Any]])->Any:
    """Apply a finite named mutation without altering the retained substrate."""
    out=document
    for path,value in patches:out=replace(out,path,value)
    return out


def digest(value:Any)->str:
    """Hash a compact reader result for baseline/restoration identity."""
    return hashlib.sha256(json.dumps(value,sort_keys=True,separators=(',',':')).encode()).hexdigest()


def remap_sequences(document:dict[str,Any],mapping:Callable[[int],int])->dict[str,Any]:
    """Reorder causal IDs coherently so timing controls reach the semantic timing assertion."""
    out=dict(document)
    for stream in ['callbacks','events','random','operations','failures']:
        rows=[]
        for original in document[stream]:
            row=dict(original,global_sequence=mapping(original['global_sequence']))
            for field in ['callback_return_sequence','shuffle_event_sequence']:
                if field in row:row[field]=mapping(row[field])
            rows.append(row)
        rows.sort(key=lambda r:r['global_sequence']);out[stream]=[dict(row,ordinal=i) for i,row in enumerate(rows)]
    out['settlement_events']=[dict(row,event_sequence=mapping(row['event_sequence'])) for row in document['settlement_events']]
    keymap={key:'observation-'+str(mapping(int(key.split('-')[-1]))) for key in document['checkpoints']}
    out['checkpoints']={keymap[key]:value for key,value in document['checkpoints'].items()}
    out['observations']=[dict(row,source_global_sequence=mapping(row['source_global_sequence']),observation_id='observation-'+str(mapping(row['source_global_sequence'])),checkpoint_ref=keymap[row['checkpoint_ref']],**({'key':'event.'+str(mapping(row['source_global_sequence']))} if row['key'].startswith('event.') else {})) for row in document['observations']]
    def extra(value:Any)->Any:
        if isinstance(value,dict):
            updates={}
            for key,member in value.items():
                mapped=mapping(member) if key in ('source_global_sequence','observed_before_sequence','event_sequence') and type(member) is int else extra(member)
                if mapped is not member:updates[key]=mapped
            return dict(value,**updates) if updates else value
        if isinstance(value,list):
            mapped=[extra(member) for member in value];return mapped if any(a is not b for a,b in zip(mapped,value,strict=True)) else value
        return value
    for field in ['trigger_receipts','token_receipts','causal_events']:
        if field in document:out[field]=extra(document[field])
    return out


def insert_body_priority(document:dict[str,Any],position:int)->dict[str,Any]:
    """Insert a structurally valid copied real priority callback into an effect's draw interval."""
    source=next(r for r in document['callbacks'] if r['kind']=='entry' and r['method']=='chooseSpellAbilityToPlay' and r['global_sequence']>position)
    returned=next(r for r in document['callbacks'] if r['kind']=='return' and r['callback_id']==source['callback_id'])
    threshold=min(row['callback_id'] for row in document['callbacks'] if row['kind']=='entry' and row['global_sequence']>=position);out=remap_sequences(document,lambda seq:seq+2 if seq>=position else seq);parents=[]
    for row in document['callbacks']:
        if row['global_sequence']>=position:break
        if row['kind']=='entry':parents.append(row['callback_id'])
        else:same(parents.pop(),row['callback_id'],'actual insertion source parent nesting')
    callbacks=[]
    for row in out['callbacks']:
        row=dict(row,callback_id=row['callback_id']+1 if row['callback_id']>=threshold else row['callback_id'])
        if row['kind']=='entry' and row['parent_callback_id'] is not None and row['parent_callback_id']>=threshold:row['parent_callback_id']+=1
        callbacks.append(row)
    callbacks +=[dict(source,callback_id=threshold,parent_callback_id=parents[-1] if parents else None,global_sequence=position),dict(returned,callback_id=threshold,global_sequence=position+1)]
    callbacks.sort(key=lambda row:row['global_sequence']);out['callbacks']=[dict(row,ordinal=i) for i,row in enumerate(callbacks)]
    if 'trigger_receipts' in out:out['trigger_receipts']=[dict(row,callback_id=row['callback_id']+1 if row['callback_id']>=threshold else row['callback_id']) for row in out['trigger_receipts']]
    return out


def exercise_mutation(control_id:str,scope:str,substrate:Any,mutate:Callable[[Any],Any],reader:Callable[[Any],Any],detail:str)->dict[str,Any]:
    """Exercise one actual reader substrate with shared baseline/rejection/restoration checks."""
    baseline=reader(substrate);baseline_digest=digest(baseline);mutated=mutate(substrate)
    try:reader(mutated)
    except EvidenceError as error:
        reason=str(error);failure_type=type(error).__name__;underlying_error_type=None if error.__cause__ is None else type(error.__cause__).__name__
    else:raise EvidenceError('ineffective actual reader mutation '+control_id+' '+detail)
    if control_id=='IC19':require('no actual priority callback between sequential draws' in reason,'priority control must exercise effect timing guard: '+reason)
    if control_id=='IC21':require('actual Divination completion/exit precedes failed-draw loss' in reason,'loss control must exercise completion timing guard: '+reason)
    if control_id.startswith('OC01') or control_id.startswith('OC02'):require('actual final outcome equals terminal native header' in reason,'winner consistency control must reach final/header binding: '+reason)
    if control_id.startswith('OC03'):require('actual native/reference terminal winner and losses' in reason,'coherent winner control must reach actual cross-engine winner join: '+reason)
    if control_id.startswith('DC01') or control_id.startswith('DC02'):require('actual native Divination total attempted draws' in reason,'draw quantity control must reach semantic attempt total: '+reason)
    if control_id.startswith('DC03'):require('actual native draw effects precede body completion' in reason,'draw order control must reach body completion guard: '+reason)
    if control_id.startswith('DC04'):require('no intermediate native state-based check before Divination completes' in reason,'draw loss order control must reach SBA boundary guard: '+reason)
    restored=reader(substrate);same(digest(restored),baseline_digest,'actual baseline/restoration '+control_id)
    return dict(id=control_id,scope=scope,mutation=detail,status='EXERCISED_REJECTED',reason=reason,failure_type=failure_type,underlying_error_type=underlying_error_type,baseline_passed=True,restoration_passed=True,copy_scope='Named mutation paths and ancestor containers only; all unchanged values shared, no full-packet deep copy')


def exercise_optional_atomic(obligation:dict[str,Any],case_id:str,substrate:Any,patches:list[tuple[list[Any],Any]],reader:Callable[[Any],Any],guards:tuple[str,...],anchors:list[dict[str,Any]])->dict[str,Any]:
    """Bind one frozen obligation to real fields and its actual semantic rejection.

    The caller supplies retained, successful real inputs. This component entry
    point is also used by the complete reader; it grants no game qualification.
    """
    require(case_id in obligation['case_ids'],'actual optional mutation eligible case '+obligation['id'])
    require(bool(patches) and bool(anchors),'actual optional mutation pointers required')
    row=exercise_mutation(obligation['id'],case_id,substrate,lambda value:changed(value,patches),evidence_boundary(reader),obligation['mutation']+' '+obligation['semantic_selector'])
    require(any(guard in row['reason'] for guard in guards),'optional atomic mutation reached intended guard '+obligation['id']+': '+row['reason'])
    if ' malformed evidence: ' in row['reason']:
        omitted=[path[-1] for path,value in patches if value is DELETE and path]
        require(row['underlying_error_type']=='KeyError' and any(row['reason']==reader.__name__+' malformed evidence: '+repr(key) for key in omitted),'optional omission reaches exact component and omitted key '+obligation['id'])
    return dict(row,semantic_selector=obligation['semantic_selector'],intended_guard=obligation['intended_guard'],actual_intended_guards=list(guards),actual_field_anchors=anchors,proof_depth='ACTUAL_READER_COMPONENT_MUTATION_ONLY',source_card_qualification=False)


class Controls:
    """Actual reader entry points and lazy bounded substrates for one control epoch."""
    def __init__(self,run_dir:Path,native_paths:list[Path],binding:dict[str,Any]|None,allow_uncaptured:bool)->None:
        self.run_dir=run_dir;self.inputs=read(run_dir/'inputs.json');self.full=read(run_dir/'cases-v1.json');self.manifest=read(run_dir/'manifest.json');self.focus=read(run_dir/'focused-cases-v3.json');self.plan=read(run_dir/'reader-controls-v3.json');self.binding=binding;self.allow_uncaptured=allow_uncaptured;self.raws:dict[str,Any]={};self.native_paths=native_paths;self.native_runs:dict[str,Any]={};self.native_envelope:dict[str,Any]|None=None;self.native_case:str|None=None;self.rows=[];self.baselines={};self.file_before={str(p):sha(p) for p in native_paths}
        if binding is not None:self.file_before.update({r['path']:r['sha256'] for r in binding['cases']})
    def raw(self,name:str)->dict[str,Any]:
        if name not in self.raws:
            p=self.run_dir/'evidence'/f'{name}.json';self.raws[name]=read(p);self.file_before[str(p)]=sha(p)
        return self.raws[name]
    def case(self,name:str)->dict[str,Any]:return next(c for c in self.inputs['cases'] if c['case_id']==name)
    def focus_case(self,name:str)->dict[str,Any]|None:return next((c for c in self.focus['scenarios'] if c['id']==name),None)
    @evidence_boundary
    def raw_check(self,name:str,value:dict[str,Any])->dict[str,Any]:
        projected=forge_projection(value,self.case(name),self.full,self.manifest,self.inputs);assert_expected_facts(projected,value,next((c for c in self.full['cases'] if c['id']==name),None),self.focus_case(name));return projected
    def load_native(self)->None:
        paths=self.native_paths or ([Path(r['path']) for r in self.binding['cases']] if self.binding else [])
        indexed={r['caseId']:r for r in self.binding['cases']} if self.binding else None
        if indexed is not None:
            known={str(Path(r['path']).resolve()):r for r in indexed.values()};require(all(str(p.resolve()) in known for p in paths),'actual controls paths belong to final image index')
            for path in paths:same(sha(path),known[str(path.resolve())]['sha256'],'actual controls indexed file bytes')
        for path in paths:
            document=read(path);report,provenance=unwrap_native(document,self.allow_uncaptured,self.manifest)
            for run in report['runs']:
                name=run['fixture_ref']
                if indexed is not None:
                    require(name in indexed,'actual control case belongs to final index');same(path.resolve(),Path(indexed[name]['path']).resolve(),'actual indexed control case file');same(provenance['captured'],True,'actual final control witness');validate_captured_sources(document,self.binding,indexed[name])
                if name in ['IF01','IF09','IF11','IF12']:self.native_runs[name]=run
            if provenance['captured'] and self.native_envelope is None and any(r['fixture_ref']=='IF01' for r in report['runs']):self.native_envelope=document;self.native_case='IF01'
            # Other complete witness values are released immediately; retain only small selected RunReports.
            del document,report
    def native_check(self,name:str,value:dict[str,Any])->dict[str,Any]:return native_projection(value,self.case(name),self.full,self.manifest,self.focus_case(name),qualify_probes=self.binding is not None)
    def exercise(self,control_id:str,scope:str,substrate:Any,mutate:Callable[[Any],Any],reader:Callable[[Any],Any],detail:str)->None:
        self.rows.append(exercise_mutation(control_id,scope,substrate,mutate,reader,detail))
    def ref(self,i:str,name:str,patches:list[tuple[list[Any],Any]],detail:str)->None:
        self.exercise(i,'actual_reference:'+name,self.raw(name),lambda d:changed(d,patches),lambda d:self.raw_check(name,d),detail)
    def native(self,i:str,name:str,patches:list[tuple[list[Any],Any]],detail:str)->None:
        if name not in self.native_runs:self.unavailable(i,'actual native '+name+' RunReport absent');return
        self.exercise(i,'actual_native_run:'+name,self.native_runs[name],lambda d:changed(d,patches),lambda d:self.native_check(name,d),detail)
    def envelope(self,i:str,path:list[Any],value:Any,detail:str)->None:
        if self.native_envelope is None:self.unavailable(i,'actual current9/witness7 IF01 capture absent');return
        self.exercise(i,'actual_native_witness:IF01',self.native_envelope,lambda d:replace(d,path,value),lambda d:unwrap_native(d,False,self.manifest),detail)
    def unavailable(self,i:str,reason:str)->None:self.rows.append(dict(id=i,status='NOT_EXERCISED',reason=reason))
    def callback(self,name:str,method:str,kind:str,predicate:Callable[[Any],bool]|None=None)->int:
        return next(i for i,r in enumerate(self.raw(name)['callbacks']) if r['method']==method and r['kind']==kind and (predicate is None or predicate(r)))
    def cast_index(self,name:str,alias:str,predicate:Callable[[Any],bool]|None=None)->int:
        return next(i for i,r in enumerate(self.raw(name)['cast_operations']) if r['source']==alias and (predicate is None or predicate(r)))
    def cut(self,name:str,key:str)->list[Any]:
        raw=self.raw(name);return ['checkpoints',next(o['checkpoint_ref'] for o in raw['observations'] if o['key']==key)]


def run_controls(c:Controls)->None:
    """Map each frozen logical class to an actual evidence mutation and actual reader assertion."""
    c.load_native();b=c.raw('BI02');f=c.raw('IF15');one=c.raw('IF01')
    c.ref('RC01','BI02',[(['callbacks',1],DELETE)],'remove an actual callback return')
    c.ref('RC02','BI02',[(['callbacks'],b['callbacks']+[b['callbacks'][0]])],'duplicate actual callback ID/ordinal')
    i=c.callback('BI02','chooseSpellAbilityToPlay','entry');c.ref('RC03','BI02',[(['callbacks',i,'method'],'unsupportedSyntheticChoice')],'change real callback method to unknown method')
    c.ref('RC04','BI02',[(['callbacks',i,'parent_callback_id'],999999)],'break actual callback parent/order')
    c.ref('RC05','BI02',[(['completion','actions_consumed'],b['completion']['actions_consumed']-1)],'leave one actual scripted action unconsumed')
    c.native('RC06','IF01',[(['rerun','final_state_equal'],False)],'actual full native final-state rerun=false')
    c.native('RC07','IF01',[(['rejection_probes',0,'state_retained'],False)],'actual rejected-input state retention=false')
    c.ref('RC08','BI02',[(['completion','natural_start_game_return'],False)],'full game loses its actual natural return receipt')
    c.ref('RC09','BI02',[(['actual_outcome','players','U','won'],False)],'actual winner flag contradicts actual winning role')
    c.ref('RC10','BI02',[(['registered_players','U','player_id'],999999)],'registered role/player ID no longer matches actual callback/card IDs')
    c.ref('RC11','BI02',[(['physical_aliases','U.island.1'],b['physical_aliases']['U.island.2']),(['physical_aliases','U.island.2'],b['physical_aliases']['U.island.1'])],'swap two actual same-definition physical aliases')
    reg=['type_context_witness','registry','coverage']
    c.envelope('RC12',reg+['cards',0,'oracleId'],'00000000-0000-0000-0000-000000000000','foreign actual registry Oracle identity')
    c.envelope('RC13',reg+['cards',0,'binding','program'],'interaction-draw-two-v1','replace actual selected registry program family')
    c.envelope('RC14',reg+['registryVersion'],5,'replace actual registry6 with predecessor5')
    if c.native_envelope is not None:c.envelope('RC15',reg+['cards'],c.native_envelope['type_context_witness']['registry']['coverage']['cards'][:-1],'drop one actual selected identity')
    else:c.unavailable('RC15','actual registry witness absent')
    c.ref('RC16','BI02',[(['paper_cards','Island','set'],'UNKNOWN')],'alter actual loaded edition while preserving saved source name')
    if c.binding is not None:c.exercise('RC17','actual_native_image_index',c.binding,lambda d:replace(d,['runtime','sha256'],'0'*64),validate_image_binding,'change actual copied final runtime image SHA')
    else:c.unavailable('RC17','actual final native image index absent')
    if c.native_envelope is not None:
        w=c.native_envelope['type_context_witness'];c.envelope('RC18',['type_context_witness','coverage','record_pointers',0],'/observations/data/runs/0/does-not-exist','actual witness dangling JSONpointer, typed EvidenceError required');c.envelope('RC19',['type_context_witness','snapshots'],w['snapshots'][:-1],'drop actual required witness point');c.envelope('RC20',['type_context_witness','snapshots'],w['snapshots']+[w['snapshots'][0]],'duplicate actual witness point ID')
    else:
        for k in ['RC18','RC19','RC20']:c.unavailable(k,'actual current9/witness7 capture absent')
    c.ref('RC21','BI02',[(['final_world','header','players','U','counts','library'],1)],'change actual header count without changing actual pile')
    c.ref('RC22','BI02',[(['observations',0,'key'],'unknown.extra.selector')],'rename real selector to an unknown selected observation')
    random_index=next(i for i,r in enumerate(b['random']) if r['kind']=='operation_commit');perm=b['random'][random_index]['actual_committed'];c.ref('RC23','BI02',[(['random',random_index,'actual_committed'],[perm[1],perm[0]]+perm[2:])],'swap actual full60 shuffle permutation entries')
    c.ref('RC24','BI02',[(['final_world','players','G','life'],'0'),(['final_world','header','players','G','life'],'0')],'coherently clamp actual negative terminal life to zero')
    cancel=c.cast_index('BI02','U.cancel.1');cs=c.cast_index('BI02','U.counterspell.1');uns=c.cast_index('BI02','U.unsummon.2')
    c.ref('IC01','BI02',[(['cast_operations',cancel,'stack_bottom_first'],list(reversed(b['cast_operations'][cancel]['stack_bottom_first'])))],'reverse actual three-entry live stack order')
    c.ref('IC02','BI02',[(['cast_operations',uns,'actual_ability','targets',0,'alias'],'G.grizzly-bears.2')],'replace actual creature target by another physical copy')
    c.ref('IC03','BI02',[(['cast_operations',uns,'actual_ability','targets',0,'timestamp'],'999999')],'replace selected current target incarnation with stale timestamp')
    c.ref('IC04','BI02',[(['cast_operations',cs,'actual_ability','targets',0,'stack_id'],b['cast_operations'][cs]['stack_id'])],'change actual target to another live/new entry')
    c.ref('IC05','BI02',[(['cast_operations',cs,'actual_ability','targets',0,'stack_id'],b['physical_aliases']['U.unsummon.2'])],'substitute physical card ID for actual target stack ID')
    c.ref('IC06','IF06',[(['final_world','players','U','hand'],['G.grizzly-bears.1']),(['final_world','players','G','hand'],[])],'send actual bounced opponent card to caster hand')
    c.ref('IC07','BI02',[(['cast_operations',cs,'actual_ability','targets'],[])],'omit an actually required counter target')
    i=c.callback('BI02','applyManaToCost','entry');c.ref('IC08','BI02',[(['callbacks',i,'arguments','payer'],'G')],'change actual mana payer independently of callback actor')
    i=c.callback('BI02','applyManaToCost','return');c.ref('IC09','BI02',[(['callbacks',i,'reply','spent'],[])],'delete actual paid debit quantity/source list')
    c.ref('IC10','BI02',[(['callbacks',i,'reply','spent',0,'token','source_timestamp'],'999999')],'change actual mana production source incarnation')
    recast=c.cast_index('BI02','U.merfolk-of-the-pearl-trident.1',lambda r:'recast' in r['action_id']);operation=b['cast_operations'][recast]['operation_id'];entry_id=next(r['callback_id'] for r in b['callbacks'] if r['kind']=='entry' and r['method']=='applyManaToCost' and r['operation_id']==operation);i=next(i for i,r in enumerate(b['callbacks']) if r['kind']=='return' and r['callback_id']==entry_id)
    c.ref('IC11','BI02',[(['callbacks',i,'reply','spent',0,'token','source'],'U.island.1')],'collapse Merfolk recast payment into its earlier Island1 cast instead of actual Island2')
    cut=c.cut('IF15','settle.IF15.cast.counter2');c.ref('IC12','IF15',[(cut+['players','U','mana','U'],'2'),(cut+['header','players','U','mana','U'],'2')],'coherently refund2U after an actual countered spell')
    raw=c.raw('BI01');action=next(r['action_id'] for r in raw['cast_operations'] if r['source']=='G.grizzly-bears.1');cut=c.cut('BI01','settle.'+action)
    c.ref('IC13','BI01',[(cut+['cards','G.grizzly-bears.1','zone'],'battlefield')],'actual countered creature incorrectly enters battlefield')
    action=b['cast_operations'][cancel]['action_id'];cut=c.cut('BI02','settle.'+action);c.ref('IC14','BI02',[(cut+['stack'],[])],'remove original Unsummon after responding Cancel resolves')
    raw=c.raw('BI01');lower=next(r['action_id'] for r in raw['cast_operations'] if r['source']=='U.unsummon.1');c.ref('IC15','BI01',[(['settlements',lower],'resolved')],'lower all-illegal Unsummon falsely reports another successful body')
    i=next(i for i,r in enumerate(raw['settlement_events']) if r['action_id']==lower and r['raw_event']=='GameEventSpellResolved');c.ref('IC16','BI01',[(['settlement_events',i,'payload','hasFizzled'],False)],'change actual all-illegal completion flag to success')
    draw=[i for i,e in enumerate(raw['events']) if e['kind']=='GameEventCardChangeZone' and e['context']['step']=='precombat_main' and e['payload']['from'] is not None and e['payload']['from']['zone_type']=='library' and e['payload']['to']['zone_type']=='hand'];require(len(draw)==2,'actual two-draw mutation prerequisite')
    c.ref('IC17','BI01',[(['events',draw[0],'payload','card'],raw['events'][draw[1]]['payload']['card']),(['events',draw[1],'payload','card'],raw['events'][draw[0]]['payload']['card'])],'swap actual ordered draw card views coherently, preserving final hand membership')
    c.ref('IC18','BI01',[(['events',draw[1]],DELETE)],'drop one actual successful Divination draw')
    c.exercise('IC19','actual_reference:BI01',raw,lambda d:insert_body_priority(d,raw['events'][draw[0]]['global_sequence']+1),lambda d:c.raw_check('BI01',d),'insert a structurally coherent real-shaped priority callback between actual effect draws')
    c.ref('IC20','IF11',[(['actual_outcome','players','U','loss_reason'],'LifeReachedZero')],'replace real failed-draw cause with life loss')
    edge=c.raw('IF11');outcome=next(i for i,e in enumerate(edge['events']) if e['kind']=='GameEventGameOutcome');exit_=next(e['global_sequence'] for e in edge['events'] if e['kind']=='GameEventCardChangeZone' and e['payload']['card']['alias']=='U.divination.1' and e['payload']['to']['zone_type']=='graveyard');outcome_sequence=edge['events'][outcome]['global_sequence'];c.exercise('IC21','actual_reference:IF11',edge,lambda d:remap_sequences(d,lambda seq:exit_ if seq==outcome_sequence else seq+1 if exit_<=seq<outcome_sequence else seq),lambda d:c.raw_check('IF11',d),'coherently move actual loss event before completed sorcery exit')
    i=c.cast_index('IF11','U.divination.1');c.ref('IC22','IF11',[(['cast_operations',i,'actual_ability','targets'],[dict(kind='object',alias='G.forest.1',physical_id=str(edge['physical_aliases']['G.forest.1']),timestamp='1',zone='battlefield')])],'add an unsolicited target to actual untargeted Divination')
    i=c.callback('BI02','declareAttackers','entry',lambda r:r['actor']=='U' and r['context']['individual_turn']==8);domain=b['callbacks'][i]['arguments']['eligible'];c.ref('IC23','BI02',[(['callbacks',i,'arguments','eligible'],domain+['U.merfolk-of-the-pearl-trident.1'])],'include fresh U4 Merfolk in actual eligible attackers too early')
    recasts=[r for r in b['cast_operations'] if r['source']=='U.merfolk-of-the-pearl-trident.1'];first=c.cut('BI02','settle.'+recasts[0]['action_id']);second=c.cut('BI02','settle.'+recasts[1]['action_id']);old_stamp=pointer(b,'/'+ '/'.join(map(str,first+['cards','U.merfolk-of-the-pearl-trident.1','timestamp'])))
    c.ref('IC24','BI02',[(second+['cards','U.merfolk-of-the-pearl-trident.1','timestamp'],old_stamp)],'reuse former battlefield incarnation on actual recast')
    raw9=c.raw('IF09');spell=raw9['cast_operations'][0]['action_id'];cut=c.cut('IF09','settle.'+spell);c.ref('IC25','IF09',[(cut+['combat',0,'blocked'],False)],'turn actual still-blocked attacker into unblocked after blocker bounce')
    raw8=c.raw('IF08');i=next(i for i,e in enumerate(raw8['events']) if e['kind']=='GameEventCardChangeZone');c.ref('IC26','IF08',[(['events',i,'kind'],'GameEventPlayerDamaged')],'give departed attacker a spurious damage event')
    cut=c.cut('IF15','settle.IF15.cast.counter2');c.ref('IC27','IF15',[(cut+['header','raw_priority'],'G')],'change actual active-player holder after top resolution')
    action=f['cast_operations'][2]['action_id'];obs=next(i for i,o in enumerate(f['observations']) if o['key']=='settle.'+action);admission=next(o['checkpoint_ref'] for o in f['observations'] if o['key']=='action.7');c.ref('IC28','IF15',[(['observations',obs,'checkpoint_ref'],admission)],'bind settled selector to actual initial cast admission')
    c.ref('IC29','IF15',[(['native_cursor'],dict(filled_from='expected'))],'fill a reference-unavailable native cursor route from expectation')
    i=next(i for i,o in enumerate(f['observations']) if o['key']=='settle.IF15.cast.counter1');c.ref('IC30','IF15',[(['observations',i],DELETE)],'drop mandatory lower all-illegal settlement row')
    cut=c.cut('IF15','settle.IF15.cast.counter2');c.ref('IC31','IF15',[(cut+['stack',0,'ability','targets'],[])],'erase surviving counter original-target history after top counter removes its target')
    c.ref('IC32','IF15',[(['cast_operations',2,'actual_ability','targets',0,'source'],'U.counterspell.1')],'top Counterspell2 targets Counterspell1 instead of shared original Unsummon')
    c.ref('IC33','IF15',[(['completion','kind'],'natural'),(['completion','natural_prepared_loop_return'],True)],'mislabel actual bounded prepared prefix as natural game')
    i=c.callback('IF09','declareAttackers','return');c.ref('IC34','IF09',[(['callbacks',i],DELETE)],'replace real attack declaration prefix by absent/fabricated receipt')
    if 'IF09' in c.native_runs:
        run=c.native_runs['IF09'];card=next(k for k,v in run['checkpoints']['0']['cards'].items() if v['alias']=='G.kalonian-tusker.1');c.native('IC35','IF09',[(['checkpoints','0','cards',card,'object','continuous_control_since_turn_start'],False)],'remove actual Untap-established active creature control age')
    else:c.unavailable('IC35','actual IF09 native RunReport absent')
    raw9=c.raw('IF09');i=next(i for i,e in enumerate(raw9['events']) if e['kind']=='GameEventCardChangeZone' and e['context']['step']=='draw');c.ref('IC36','IF09',[(['events',i,'payload','card','alias'],'G.forest.5')],'change actual mandatory prefix draw Forest4 to Forest5')
    i=next(i for i,r in enumerate(raw9['operations']) if r['kind']=='prepared_initialized');order=raw9['operations'][i]['global_battlefield_order'];c.ref('IC37','IF09',[(['operations',i,'global_battlefield_order'],[order[1],order[0]]+order[2:])],'swap actual global initialized battlefield order')


def run_postreview_controls(c:Controls)->list[dict[str,Any]]:
    """Seven additional outcome/draw controls from result review; preserve frozen61."""
    start=len(c.rows)
    ids=['OC01.final-only-wrong-winner','OC02.header-only-wrong-winner','OC03.coherent-wrong-winner-and-losses']
    if 'IF11' not in c.native_runs:
        for name in ids:c.unavailable(name,'actual final-image IF11 natural RunReport absent')
    else:
        run=c.native_runs['IF11'];last=len(run['accepted'])-1;wrong=dict(kind='winner',seat=41)
        reference=c.raw_check('IF11',c.raw('IF11'))
        def check(value:dict[str,Any])->dict[str,Any]:return join_native_reference(c.native_check('IF11',value),reference,c.case('IF11'))
        c.exercise(ids[0],'actual_native_run:IF11',run,lambda d:replace(d,['final','outcome'],wrong),check,'Change actual final winner G to U while preserving the actual terminal header')
        c.exercise(ids[1],'actual_native_run:IF11',run,lambda d:replace(d,['accepted',last,'header','outcome'],wrong),check,'Change actual terminal-header winner G to U while preserving final outcome')
        c.exercise(ids[2],'actual_native_run:IF11',run,lambda d:changed(d,[(['final','outcome'],wrong),(['accepted',last,'header','outcome'],wrong),(['accepted',last,'header','losses'],[3])]),check,'Coherently change final/header winner and loser partition; the unchanged actual Forge winner must reject')
    draw_ids=['DC01.dropped-failed-attempt-transaction','DC02.changed-failed-attempt-total','DC03.failed-attempt-after-completion','DC04.state-based-loss-before-completion']
    if 'IF12' not in c.native_runs:
        for name in draw_ids:c.unavailable(name,'actual final-image IF12 natural RunReport absent')
    else:
        run=c.native_runs['IF12'];row=next(i for i,r in enumerate(run['accepted']) if any(e['cause']['tag']=='Draw' and any(o['value']['tag']=='FailedDraw' for o in e['occurrences']) for e in r['current_facts']['interaction_journal']));journal=run['accepted'][row]['current_facts']['interaction_journal'];draw=next(i for i,e in enumerate(journal) if e['cause']['tag']=='Draw');completion=next(i for i,e in enumerate(journal) if e['cause']['tag']=='ResolutionCompleted');sba=next(i for i,e in enumerate(journal) if e['cause']['tag']=='StateBasedActions');path=['accepted',row,'current_facts','interaction_journal'];reference=c.raw_check('IF12',c.raw('IF12'))
        def check_draw(value:dict[str,Any])->dict[str,Any]:return join_native_reference(c.native_check('IF12',value),reference,c.case('IF12'))
        c.exercise(draw_ids[0],'actual_native_run:IF12',run,lambda d:replace(d,path+[draw],DELETE),check_draw,'Drop the actual single FailedDraw transaction carrying both empty attempts')
        occurrence=next(i for i,o in enumerate(journal[draw]['occurrences']) if o['value']['tag']=='FailedDraw');c.exercise(draw_ids[1],'actual_native_run:IF12',run,lambda d:replace(d,path+[draw,'occurrences',occurrence,'value','attempts'],'1'),check_draw,'Change actual compressed empty-attempt quantity2 to1 while preserving the one real event')
        after=list(journal);after[draw],after[completion]=after[completion],after[draw];c.exercise(draw_ids[2],'actual_native_run:IF12',run,lambda d:replace(d,path,after),check_draw,'Move the actual failure transaction after Divination completion; preserve its actual quantity and payload')
        early=list(journal);early[completion],early[sba]=early[sba],early[completion];c.exercise(draw_ids[3],'actual_native_run:IF12',run,lambda d:replace(d,path,early),check_draw,'Move actual failed-draw state-based loss before spell completion')
    extra=c.rows[start:];del c.rows[start:];return extra


def mandatory_blue_controls(run_dir:Path)->list[dict[str,Any]]:
    """Exercise preserved winner/draw/blocked/field semantics in actual Blue data."""
    compare_reference(run_dir);controls=Controls(run_dir,[],None,False);rows=[]
    def exercise(variant:str,name:str,path:list[Any],value:Any,guard:str)->None:
        row=exercise_mutation('RC.blue-reference-preservation','actual_preserved_Blue:'+name,controls.raw(name),lambda document:replace(document,path,value),lambda document:controls.raw_check(name,document),'alter actual preserved Blue '+variant);rows.append(named_variant(row,variant,guard))
    winner=controls.raw('IF11');exercise('winner','IF11',['actual_outcome','winning_seat'],'U' if winner['actual_outcome']['winning_seat']=='G' else 'G','actual won flag')
    draw_index=next(index for index,row in enumerate(winner['events']) if row['kind']=='GameEventCardChangeZone' and row['context']['step']=='precombat_main' and row['payload']['from'] is not None and row['payload']['from']['zone_type']=='library' and row['payload']['to']['zone_type']=='hand')
    exercise('draw','IF11',['events',draw_index,'payload','to','zone_type'],'graveyard','actual ordered successful Divination draws')
    bounced=controls.raw('IF09');entry=bounced['cast_operations'][0]['action_id'];cut=controls.cut('IF09','settle.'+entry)
    exercise('blocked','IF09',cut+['combat',0,'blocked'],False,'blocked status survives blocker departure')
    exercise('field_omitted','IF09',cut+['combat',0,'blocked'],DELETE,"raw_check malformed evidence: 'blocked'")
    for path,before in controls.file_before.items():same(sha(Path(path)),before,'actual preserved Blue control original unchanged')
    return rows


def remaining_reference_controls(raw:Callable[[str],dict[str,Any]],check:Callable[[str,dict[str,Any]],Any])->list[dict[str,Any]]:
    """Exercise genuine controller order, singleton prompt and counter disposition."""
    rows=[]
    # Move the two real, complete adjacent controller callback subtrees, retaining
    # their actual actor/domain/admitted bodies and every reciprocal causal join.
    name='MT01';document=raw(name);entries={row['callback_id']:row for row in document['callbacks'] if row['kind']=='entry'};returns={row['callback_id']:row for row in document['callbacks'] if row['kind']=='return'};groups={}
    for call in entries.values():
        if call['method']=='orderAndPlaySimultaneousSa':groups.setdefault(returns[call['callback_id']]['reply']['group'],[]).append(call)
    calls=next(value for value in groups.values() if len(value)==2 and len({row['actor'] for row in value})==2);first,second=calls;left=first['global_sequence'];middle=second['global_sequence'];right=returns[second['callback_id']]['global_sequence']+1
    same(returns[first['callback_id']]['global_sequence']+1,middle,'actual adjacent controller callback subtrees')
    def exchange(value:dict[str,Any])->dict[str,Any]:
        out=remap_sequences(value,lambda sequence:sequence+(right-middle) if left<=sequence<middle else sequence-(middle-left) if middle<=sequence<right else sequence);ids={row['callback_id']:index+1 for index,row in enumerate(row for row in out['callbacks'] if row['kind']=='entry')};callbacks=[]
        for row in out['callbacks']:
            updates=dict(callback_id=ids[row['callback_id']])
            if row['kind']=='entry':updates['parent_callback_id']=None if row['parent_callback_id'] is None else ids[row['parent_callback_id']]
            callbacks.append(dict(row,**updates))
        return dict(out,callbacks=callbacks,trigger_receipts=[dict(row,callback_id=ids[row['callback_id']]) for row in out['trigger_receipts']])
    rows.append(named_variant(exercise_mutation('RC.apnap-order','actual_reference:MT01',document,exchange,lambda value:check(name,value),'exchange two actual complete controller callback subtrees in one real two-controller APNAP group'),'controller_order','actual APNAP controller callback order'))

    # T11 really has singleton physical/token entrants for separate owners. Add a
    # counterfactual owner-order prompt using the real maintained method's shape
    # and the actual T11 physical entrant, with complete callback pairing/IDs.
    name='T11.token-death-cessation';document=raw(name);template=raw('T06.artist-top');source=next(row for row in template['callbacks'] if row['kind']=='entry' and row['method']=='orderMoveToZoneList');returned=next(row for row in template['callbacks'] if row['kind']=='return' and row['callback_id']==source['callback_id']);death=next(row for row in document['events'] if row['kind']=='GameEventCardChangeZone' and row['payload']['card']['alias']=='P0.grizzly-bears.1' and row['payload']['from'] and row['payload']['from']['zone_type']=='battlefield' and row['payload']['to']['zone_type']=='graveyard');position=death['global_sequence']+1;alias=death['payload']['card']['alias'];actor=death['payload']['card']['owner']
    def singleton_prompt(value:dict[str,Any])->dict[str,Any]:
        out=insert_body_priority(value,position);callbacks=list(out['callbacks']);entry_index=next(index for index,row in enumerate(callbacks) if row['kind']=='entry' and row['global_sequence']==position);identity=callbacks[entry_index]['callback_id'];return_index=next(index for index,row in enumerate(callbacks) if row['kind']=='return' and row['callback_id']==identity)
        call=callbacks[entry_index];reply=callbacks[return_index];policy=dict(returned['reply']['input_policy'],cause='counterfactual-singleton-owner-prompt',chooser=actor,seat=actor,domain=[dict(kind='physical_current',alias=alias)],entrants_top_first=[dict(kind='physical_current',alias=alias)],aliases=[alias],individual_turn=call['context']['individual_turn'],step=call['context']['step'])
        callbacks[entry_index]=dict(call,method=source['method'],actor=actor,actor_player_id=value['registered_players'][actor]['player_id'],arguments=dict(source['arguments'],offered_sequence=[alias]),actual_stack_tags=source['actual_stack_tags'],actual_stack_frames=source['actual_stack_frames']);callbacks[return_index]=dict(reply,method=source['method'],actor=actor,reply=dict(input_policy=policy,selected_top_to_bottom=[alias],returned_sequence=[alias]));return dict(out,callbacks=callbacks)
    rows.append(named_variant(exercise_mutation('RC.graveyard-order','actual_reference:'+name,document,singleton_prompt,lambda value:check(name,value),'insert a counterfactual paired real-method owner-order prompt for the actual single P0 physical graveyard entrant'),'singleton_prompt','primary/Forge T11.token-death-cessation/T11.token-death-cessation.singleton-no-order-prompt/owner_order_prompts'))

    name='T03.countered';document=raw(name);action=next(row['action_id'] for row in document['cast_operations'] if row['source']=='P0.elvish-visionary.1');same(document['settlements'][action],'countered','actual countered creature substrate')
    rows.append(named_variant(exercise_mutation('RC.cast-stack-binding','actual_reference:'+name,document,lambda value:changed(value,[(['settlements',action],'resolved')]),lambda value:check(name,value),'change the actual countered Visionary disposition to resolved while retaining its real CounterEffect removal and no resolution event'),'counter_disposition','one actual resolved/fizzled event'))
    return rows


@evidence_boundary
def mandatory_reference_controls(run_dir:Path,primary_index:Path,compiled_input:Path,requirements:Path,blue_run:Path|None=None)->dict[str,Any]:
    """Exercise actual reference components now; native/observer/image prerequisites retain explicit open gates."""
    baseline=compare_mandatory_reference(run_dir,primary_index,compiled_input,requirements)
    inputs=read(run_dir/'inputs.json');binding=read(run_dir/'authority-binding.json');authority=mandatory_authorities(Path(binding['source_root']),primary_index,compiled_input,requirements);needs=authority['requirements'];primary={r['id']:r for r in authority['payloads']['full-games.json']['cases']+authority['payloads']['reduced-cases.json']['variants']};cases={r['case_id']:r for r in inputs['cases']};rows=[];raws={};guards={};files={}
    def raw(name:str)->dict[str,Any]:
        if name not in raws:
            path=run_dir/'evidence'/f'{name}.json';raws[name]=read(path);files[str(path.resolve())]=sha(path)
        return raws[name]
    def check(name:str,value:dict[str,Any])->dict[str,Any]:
        projected=mandatory_forge_projection(value,cases[name],inputs);facts=mandatory_primary_facts(value,cases[name],projected,primary[name]);return dict(final=projected['final'],facts=facts,trigger_count=len(projected['triggers']),token_count=len(projected['tokens']))
    def mutation(control:str,name:str,path:list[Any],value:Any,detail:str,guard:str,variant:str|None=None)->None:
        row=exercise_mutation(control,'actual_reference:'+name,raw(name),lambda document:replace(document,path,value),lambda document:check(name,document),detail)
        require(guard in row['reason'],'mandatory control reached intended guard '+control+': '+row['reason']);row['changed_path']=path;rows.append(named_variant(row,variant,guard) if variant is not None else row)
    def callback(name:str,method:str,kind:str)->int:return next(i for i,row in enumerate(raw(name)['callbacks']) if row['method']==method and row['kind']==kind)
    one='T01.opponent-active';token='T02.own-main';death='T06.artist-top';failed='T04.empty';departed='T03.departed-recast'
    same(set(cases),{r['id'] for r in needs['cases']},'required all23 controls substrate')
    row=exercise_mutation('RC.case-inventory','actual_reference_inputs',inputs,lambda d:replace(d,['cases'],d['cases'][:-1]),lambda d:same([c['case_id'] for c in d['cases']],[c['id'] for c in needs['cases']],'exact required23 reference cases'),'drop one real reference recipe');rows.append(named_variant(row,'reference_remove','exact required23 reference cases'))
    mutation('RC.completion',failed,['completion','kind'],'bounded','replace real natural loss with bounded ready','required natural completion','natural_to_ready')
    mutation('RC.completion',one,['completion','actions_consumed'],0,'skip real intended commands','actual complete actions_consumed','intents_omitted')
    mutation('RC.winner-loss-partition','T08.opponent',['actual_outcome','winning_seat'],'P1','alter actual registered winner','actual outcome winner identity','forge_winner')
    mutation('RC.source-authority',one,['paper_cards','Blood Artist','source_receipt','raw_line_sha256'],'0'*64,'alter selected actual source row hash','mandatory printing Blood Artist/source_receipt','selected_source_record')
    mutation('RC.physical-inventory',one,['physical_aliases','P0.soul-warden.1'],999999,'replace actual physical identity','actual stable within-engine physical ID')
    i=callback('MT02','mulliganKeepHand','return');mutation('RC.opening-order','MT02',['callbacks',i,'reply','input_policy','chooser'],'P0','swap real starter-first keep chooser','actual opening reply policy','actual_chooser')
    opening=raw('MT02');keeps=[(index,row) for index,row in enumerate(opening['callbacks']) if row['method']=='mulliganKeepHand' and row['kind']=='entry'];same(len(keeps),2,'actual two opening keep controls prerequisite');patches=[]
    for (index,entry),(other_index,other) in [(keeps[0],keeps[1]),(keeps[1],keeps[0])]:
        for field in ['actor','arguments']:patches.append((['callbacks',index,field],other[field]))
        returned=next((j,row) for j,row in enumerate(opening['callbacks']) if row['callback_id']==entry['callback_id'] and row['kind']=='return');other_return=next(row for row in opening['callbacks'] if row['callback_id']==other['callback_id'] and row['kind']=='return')
        for field in ['actor','reply']:patches.append((['callbacks',returned[0],field],other_return[field]))
    rows.append(named_variant(exercise_mutation('RC.opening-order','actual_reference:MT02',opening,lambda document:changed(document,patches),lambda document:check('MT02',document),'exchange both actual keep actors/arguments/replies while preserving paired IDs and stream chronology'),'starter_first','actual starter-first keep callback order'))
    i=callback('MT01','applyManaToCost','return');mutation('RC.paid-source-cost','MT01',['callbacks',i,'reply','spent',0,'token','source_timestamp'],'999999','alter actual paid mana source incarnation','actual paid mana source incarnation','source_incarnation')
    debit=callback(token,'applyManaToCost','return');spent=raw(token)['callbacks'][debit]['reply']['spent'];require(len(spent)>1,'actual two credited payment units')
    mutation('RC.paid-source-cost',token,['callbacks',debit,'reply','spent',0,'token','source'],spent[1]['token']['source'],'replace first paid source by another actual credited source','actual spent source receipts','mana_source')
    mutation('RC.paid-source-cost',token,['callbacks',debit,'reply','spent',0,'token','color'],'green' if spent[0]['token']['color']!='green' else 'white','change real paid mana type','actual produced color','mana_type')
    mutation('RC.paid-source-cost',token,['callbacks',debit,'reply','spent'],spent[:-1],'remove one actual paid unit while retaining cast cost','actual paid amount','mana_amount')
    mutation('RC.paid-source-cost',token,['cast_operations',0,'paid_sources'],list(reversed(raw(token)['cast_operations'][0]['paid_sources'])),'reverse actual cast funding attribution','actual funding-source names','cast_funding')
    mutation('RC.cast-stack-binding','T03.countered',['cast_operations',0,'stack_id'],999999,'replace actual cast stack entry ID','actual emitted stack ID binding','entry')
    mutation('RC.entry-filter',one,['trigger_receipts'],[],'omit the actually applicable Warden instance','exact mandatory trigger inventory','warden_omitted')
    actual=raw(one)['trigger_receipts'][0];orator='P0.impassioned-orator.1';orator_state=raw(one)['final_world']['cards'][orator];invented=dict(actual,name=actual['name']+'.invented-orator',actual_ability=dict(actual['actual_ability'],source=orator,source_physical_id=str(raw(one)['physical_aliases'][orator]),source_timestamp=orator_state['timestamp']),actual_source_state=orator_state)
    mutation('RC.entry-filter',one,['trigger_receipts'],raw(one)['trigger_receipts']+[invented],'invent Orator on actual opponent entry using the real existing Orator source','exact mandatory trigger inventory','orator_invented')
    own='T01.own-then-opponent';actual=raw(own)['trigger_receipts'][0];self_trigger=dict(actual,name=actual['name']+'.invented-self',subject='P0.soul-warden.1')
    mutation('RC.entry-filter',own,['trigger_receipts'],raw(own)['trigger_receipts']+[self_trigger],'invent an excluded self-entry Warden trigger','exact mandatory trigger inventory','self_exclusion')
    mutation('RC.simultaneous-cohort',token,['token_receipts'],raw(token)['token_receipts'][:-1],'collapse actual two-token creation cohort','source-exact token amount','birth_omitted')
    mutation('RC.trigger-definition',one,['trigger_receipts',0,'actual_ability','source'],'P0.impassioned-orator.1','replace captured trigger source definition','actual source relation','source')
    mutation('RC.trigger-complete-causes',death,['trigger_receipts',0,'causal_events'],raw(death)['trigger_receipts'][0]['causal_events'][:-1],'omit one actual two-death complete cause member','actual complete cause members','member_omitted')
    i=callback(death,'orderAndPlaySimultaneousSa','return');permutation=raw(death)['callbacks'][i]['reply']['selected_bottom_first'];mutation('RC.apnap-order',death,['callbacks',i,'reply','selected_bottom_first'],list(reversed(permutation)),'reverse actual complete trigger permutation','complete APNAP controller permutation','member_order')
    i=callback(death,'chooseTargetsFor','return');mutation('RC.trigger-player-target',death,['callbacks',i,'reply','actual_ability','target_players'],['P1'],'alter actual selected player target','actual selected player target','selected_player')
    mutation('RC.graveyard-order',death,['final_world','players','P1','graveyard'],list(reversed(raw(death)['final_world']['players']['P1']['graveyard'])),'reverse actual Artist-above-Dissenter graveyard order','final_top_first','artist_dissenter_order')
    mutation('RC.departed-trigger',departed,['trigger_receipts',0,'actual_source_state','timestamp'],raw(departed)['final_world']['cards']['P0.elvish-visionary.1']['timestamp'],'replace departed capture with actual recast incarnation','actual captured source incarnation','recast_incarnation')
    i=next(i for i,r in enumerate(raw(failed)['events']) if r['kind']=='GameEventPlayerLivesChanged');mutation('RC.instruction-life-draw-order',failed,['events',i,'payload','newLives'],'18','change actual ordered life instruction amount','actual ordered complete life instructions')
    i=callback(failed,'playSpellAbilityNoStack','return');mutation('RC.failed-draw-count',failed,['callbacks',i,'reply','body_complete'],False,'remove independently observed full body completion before failed-draw outcome','body completed before SBA','body_completion_omitted')
    chain='T07.death-birth-entry';mutation('RC.death-birth-entry-chain',chain,['trigger_receipts'],raw(chain)['trigger_receipts'][:-1],'omit actual post-birth entry watcher','exact mandatory trigger inventory','watcher_omitted')
    creation_start=next(row for row in raw(chain)['callbacks'] if row['method']=='playSpellAbilityNoStack' and row['kind']=='entry' and any(f['method']=='resolve' for f in row['actual_stack_frames']));rows.append(named_variant(exercise_mutation('RC.death-birth-entry-chain','actual_reference:'+chain,raw(chain),lambda data:insert_body_priority(data,creation_start['global_sequence']+1),lambda data:check(chain,data),'insert a paired real-shaped priority callback inside actual creating trigger body'),'body_priority','no priority inside mandatory body'))
    for key,value,guard in [('name','Soldier','exact default token name'),('color_mask',4,'token exact color'),('subtypes',['Zombie'],'token exact subtypes'),('mana_cost','{1}','token absent casting cost'),('keywords',['Haste'],'token no keyword abilities'),('activated_count',1,'token no activated or mana abilities')]:mutation('RC.token-template',token,['token_receipts',0,key],value,'alter token '+key,guard,{'name':'name','color_mask':'color','subtypes':'subtypes','mana_cost':'mana','keywords':'keywords','activated_count':'activated'}[key])
    mutation('RC.token-template',token,['token_receipts',0,'state','power'],'9','alter actual printed token PT','token actual power','power')
    mutation('RC.token-birth-lineage',token,['token_receipts',0,'member'],1,'replace creation member coordinate','actual creation coordinate','member')
    trade='T11.token-death-cessation';i=next(i for i,r in enumerate(raw(trade)['events']) if r['kind']=='GameEventCardChangeZone' and r['payload']['card']['alias'].startswith('token:') and r['payload']['to']['zone_type']=='graveyard');mutation('RC.token-transient-event',trade,['events',i,'payload','to','zone_type'],'exile','replace actual token death destination after later absence','actual cause to','graveyard_appearance')
    age='T09.fresh-next-turn';i=next(i for i,r in enumerate(raw(age)['operations']) if r['kind']=='mana_legality_probe' and r['policy']['mutation']=='young');mutation('RC.creature-mana-age',age,['operations',i,'can_play'],True,'admit actual young creature mana ability','new_entry_allowed','young')
    for variant,value in [('reference_duplicate',inputs['cases']+[inputs['cases'][0]]),('reference_foreign',[dict(inputs['cases'][0],case_id='foreign-case')]+inputs['cases'][1:])]:
        row=exercise_mutation('RC.case-inventory','actual_reference_inputs',inputs,lambda document,v=value:replace(document,['cases'],v),lambda document:same([case['case_id'] for case in document['cases']],[case['id'] for case in needs['cases']],'exact required23 reference cases'),variant);rows.append(named_variant(row,variant,'exact required23 reference cases'))
    mutation('RC.winner-loss-partition',failed,['actual_outcome','players','P0','lost'],False,'change actual Forge loser partition','actual terminal/header loss partition','forge_losers')
    actual_token=raw(token)['token_receipts'][0]
    for variant,path,value,guard in [
        ('toughness',['state','toughness'],'9','token actual toughness'),
        ('triggered',['triggered_count'],1,'token no trigger abilities'),
        ('creator',['creator'],'P1','actual creation coordinate'),
        ('controller',['state','controller'],'P1','actual token controller'),
    ]:mutation('RC.token-template',token,['token_receipts',0]+path,value,'alter token '+variant,guard,variant)
    for variant,path,value,guard in [
        ('instruction',['instruction'],actual_token['instruction']+1,'actual creation coordinate'),
        ('birth_identity',['physical_id'],'999999','birth identity'),
        ('incarnation',['timestamp'],'999999','birth timestamp'),
    ]:mutation('RC.token-birth-lineage',token,['token_receipts',0]+path,value,'alter birth '+variant,guard,variant)
    cause_members=raw(death)['trigger_receipts'][0]['causal_events']
    mutation('RC.trigger-complete-causes',death,['trigger_receipts',0,'causal_events'],cause_members+[cause_members[0]],'duplicate one actual death cause member','actual complete cause members','member_duplicated')
    mutation('RC.trigger-complete-causes',death,['trigger_receipts',0,'causal_events',0],cause_members[1],'replace one actual death cause with another while preserving count','actual complete cause members','member_replaced')
    mutation('RC.trigger-complete-causes',death,['trigger_receipts',0,'causal_events',0,'card','timestamp'],'999999','change actual captured death incarnation','actual complete cause members','event_incarnation')
    mutation('RC.trigger-definition',one,['trigger_receipts',0,'captured_controller'],'P1','change actual captured trigger controller','actual captured controller','controller')
    mutation('RC.cast-stack-binding','T03.countered',['cast_operations',0,'actual_ability','source_timestamp'],'999999','change actual cast source incarnation','actual cast source incarnation','source_incarnation')
    targeted=next(i for i,cast in enumerate(raw('T03.countered')['cast_operations']) if cast['actual_ability']['targets'])
    mutation('RC.cast-stack-binding','T03.countered',['cast_operations',targeted,'actual_ability','targets',0,'stack_id'],999999,'change actual targeted stack entry relation','actual target entry binding','target_relation')
    printing=raw(one)['paper_cards']['Blood Artist'];paragraphs=printing['oracle_text'].split('\n')
    require(bool(paragraphs),'actual source paragraph control prerequisite')
    mutation('RC.program-definition',one,['paper_cards','Blood Artist','oracle_text'],'\n'.join(paragraphs[1:]),'drop actual Oracle source paragraph','mandatory printing Blood Artist/oracle_text','paragraph_removed')
    # Forge's Oracle wire retains the script paragraph separator as literal
    # backslash+n. The saved source semantics separately contains a real newline.
    separator=chr(92)+'n';source_case='MT01';name,card=next((name,card) for name,card in raw(source_case)['paper_cards'].items() if separator in card['oracle_text']);paragraphs=card['oracle_text'].split(separator)
    require(len(paragraphs)>1 and list(reversed(paragraphs))!=paragraphs,'actual distinct source paragraphs required')
    mutation('RC.program-definition',source_case,['paper_cards',name,'oracle_text'],separator.join(reversed(paragraphs)),'reverse actual encoded Oracle source paragraphs','mandatory printing '+name+'/oracle_text','paragraph_reordered')
    mutation('RC.program-definition',one,['paper_cards','Blood Artist','mana_cost'],'{9}','change actual printed casting mana','mandatory printing Blood Artist/mana_cost','printed_mana')
    rows.extend(remaining_reference_controls(raw,check))
    commands=read(run_dir/'commands.json');r=raw(one);i=next(i for i,row in enumerate(r['runtime']['loader_classes']) if row['class']=='forge.game.trigger.WrappedAbility');rows.append(named_variant(exercise_mutation('RC.reference-runtime-bindings','actual_loaded_reference_class',r,lambda d:replace(d,['runtime','loader_classes',i,'sha256'],'0'*64),lambda d:validate_runtime(d,run_dir,commands),'alter actual loaded wrapper class hash'),'class','actual retained loaded class bytes forge.game.trigger.WrappedAbility'))
    producer_index=next(index for index,value in enumerate(r['runtime']['loader_classes']) if value['class']=='forge.ai.SpellwakeBlueInteractionReferenceTest')
    rows.append(named_variant(exercise_mutation('RC.reference-runtime-bindings','actual_loaded_producer_class',r,lambda document:replace(document,['runtime','loader_classes',producer_index,'sha256'],'0'*64),lambda document:validate_runtime(document,run_dir,commands),'alter actual loaded producer class bytes'),'producer','actual retained loaded class bytes forge.ai.SpellwakeBlueInteractionReferenceTest'))
    rows.append(named_variant(exercise_mutation('RC.reference-runtime-bindings','actual_loaded_class_origin',r,lambda document:replace(document,['runtime','loader_classes',i,'code_source'],None),lambda document:validate_runtime(document,run_dir,commands),'declare a real non-JDK class as having no code origin'),'loaded_origin','declared JDK loaded-class boundary'))
    mutation('RC.reference-runtime-bindings',one,['paper_cards','Blood Artist','source_receipt','forge_resources',0,'sha256'],'0'*64,'alter actual retained Forge resource binding','mandatory printing Blood Artist/source_receipt','resource')
    results=read(run_dir/'results.json');stability=read(run_dir/'source-stability.json')
    for path,value,detail,variant,guard in [(['run','exit'],1,'actual JVM process failure','run_process','actual run process exit'),(['compile','exit'],1,'actual compiler failure','compile_process','actual compile process exit'),(['run','stdout','sha256'],'0'*64,'actual stdout bytes mismatch','stdout','actual process stream bytes'),(['run','stderr','sha256'],'0'*64,'actual stderr bytes mismatch','stderr','actual process stream bytes'),(['run','interrupted'],True,'actual process interrupted','interrupted','actual process uninterrupted'),(['run','evidence','evidence/'+one+'.json','sha256'],'0'*64,'actual immediate postprocess evidence binding','evidence_digest','actual as-produced raw evidence bytes')]:rows.append(named_variant(exercise_mutation('RC.reference-runtime-bindings','actual_reference_process_receipt',results,lambda d,p=path,v=value:replace(d,p,v),lambda d:validate_mandatory_process(run_dir.resolve(),commands,d,stability,binding),detail),variant,guard))
    guarded_authority=next(iter(stability['authority_after']));rows.append(named_variant(exercise_mutation('RC.reference-runtime-bindings','actual_reference_prepost_guard',stability,lambda document:replace(document,['authority_after',guarded_authority],'0'*64),lambda document:validate_mandatory_process(run_dir.resolve(),commands,results,document,binding),'change actual authority postguard while preserving preguard'),'prepost_guard','actual complete authority postguard inventory'))
    if blue_run is not None:
        rows.extend(mandatory_blue_controls(blue_run))
    required=mandatory_variant_dispositions(needs,rows,False)
    for path,digest_before in files.items():same(sha(Path(path)),digest_before,'actual raw control substrate restored and immutable')
    return dict(schema_version=1,profile=PROFILES[1],status='REFERENCE_COMPONENT_CONTROLS_PASSED_FINAL_NATIVE_AND_SUPPLEMENTAL_PREREQUISITES_OPEN',all_required_exercised=False,baseline_case_count=baseline['case_count'],reader_sha256=sha(Path(compare_reader.__file__)),controls_sha256=sha(Path(__file__)),exercises=rows,required_control_classes=required,not_exercised=[r['id'] for r in required if r['missing_variants']],native_numeric_agreement_claim=False)


def remap_native_events(value:Any,threshold:int|None,moved:tuple[str,str]|None=None,permutation:dict[str,str]|None=None)->Any:
    """Shift actual later EventIds and one moved occurrence, copying changed ancestors only."""
    if isinstance(value,dict):
        old=None if moved is None else dict(event=moved[0],index=moved[1]);new=None if moved is None else dict(event=str(int(moved[0])+1),index='0')
        if old is not None and value==old:return new
        updates={}
        for key,member in value.items():
            if key=='event' and isinstance(member,str) and member.isdecimal():mapped=permutation[member] if permutation is not None and member in permutation else str(int(member)+1) if threshold is not None and int(member)>=threshold else member
            else:mapped=remap_native_events(member,threshold,moved,permutation)
            if mapped is not member:updates[key]=mapped
        key=value.get('key')
        if old is not None and isinstance(key,dict) and key.get('tag')=='Occurrence' and key.get('occurrence')==old:updates['event']=new['event']
        return dict(value,**updates) if updates else value
    if isinstance(value,list):
        mapped=[remap_native_events(member,threshold,moved,permutation) for member in value];return mapped if any(a is not b for a,b in zip(mapped,value,strict=True)) else value
    return value


def insert_native_body_check(run:dict[str,Any],row_index:int,coherent:bool)->dict[str,Any]:
    """Insert an actual-shaped empty SBA within a real body, preserving optional ID coherence."""
    journal=run['accepted'][row_index]['current_facts']['rules_journal'];completion=next(index for index,event in enumerate(journal) if event['tag']=='Event' and event['cause']['tag']=='ResolutionCompleted');threshold=int(journal[completion]['event']);source=next(event for event in journal if event['tag']=='Event' and event['cause']['tag']=='StateBasedActions')
    output=remap_native_events(run,threshold) if coherent else run;current=output['accepted'][row_index]['current_facts']['rules_journal'];inserted=dict(source,event=str(threshold) if coherent else '999999',occurrences=[])
    return replace(output,['accepted',row_index,'current_facts','rules_journal'],current[:completion]+[inserted]+current[completion:])


def split_native_token_birth(run:dict[str,Any],row_index:int,journal_index:int)->dict[str,Any]:
    """Separate two genuine simultaneous births, coherently retaining all later causal IDs."""
    event=run['accepted'][row_index]['current_facts']['rules_journal'][journal_index];same(event['tag'],'Event','actual birth Event prerequisite');same([row['value']['tag'] for row in event['occurrences']],['TokenCreated','TokenCreated'],'actual exactly two simultaneous birth occurrences');identity=event['event'];output=remap_native_events(run,int(identity)+1,(identity,'1'));journal=output['accepted'][row_index]['current_facts']['rules_journal'];actual=journal[journal_index];first=dict(actual,occurrences=actual['occurrences'][:1]);second=dict(actual,event=str(int(identity)+1),occurrences=actual['occurrences'][1:]);same(second['occurrences'][0]['id'],dict(event=second['event'],index='0'),'actual moved birth occurrence has coherent identity');return replace(output,['accepted',row_index,'current_facts','rules_journal'],journal[:journal_index]+[first,second]+journal[journal_index+1:])


def exchange_native_events(run:dict[str,Any],row_index:int,left:int,right:int)->dict[str,Any]:
    """Reverse two actual effects while preserving chronological IDs and all causal references."""
    journal=run['accepted'][row_index]['current_facts']['rules_journal'];a,b=journal[left],journal[right];same((a['tag'],b['tag']),('Event','Event'),'actual two effect events');require(left<right and int(a['event'])<int(b['event']),'actual original chronological effects');output=remap_native_events(run,None,permutation={a['event']:b['event'],b['event']:a['event']});changed_journal=list(output['accepted'][row_index]['current_facts']['rules_journal']);changed_journal[left],changed_journal[right]=changed_journal[right],changed_journal[left];return replace(output,['accepted',row_index,'current_facts','rules_journal'],changed_journal)


def mandatory_lifecycle_controls(runs:dict[str,Any],check:Callable[[str,dict[str,Any]],Any])->list[dict[str,Any]]:
    """Mutate distinct actual capture/placement/installation/cursor relationships."""
    rows=[];death='T06.artist-top';token='T02.own-main';one='T01.opponent-active'
    def exercise(control:str,variant:str,name:str,patches:list[tuple[list[Any],Any]],guard:str|tuple[str,...])->None:
        row=exercise_mutation(control,'actual_native_Report3:'+name,runs[name],lambda document:changed(document,patches),lambda document:check(name,document),variant)
        rows.append(named_variant(row,variant,guard))
    actual=runs[death];pending_index=next(i for i,row in enumerate(actual['accepted']) if row['header']['pending'] is not None and row['header']['pending']['frame']['kind']=='trigger_target');pending=actual['accepted'][pending_index]['header'];entry=pending['execution']['pending_trigger_placement']['entry'];stack_index=next(i for i,row in enumerate(pending['execution']['stack']) if row['entry']==entry)
    installed_index,installed_stack_index,installed=next((i,j,value) for i,row in enumerate(actual['accepted'][pending_index+1:],pending_index+1) for j,value in enumerate(row['header']['execution']['stack']) if value['entry']==entry and value['bindings'] is not None)
    provisional=['accepted',pending_index,'header','execution','stack',stack_index]
    for variant,patches,guard in [
        ('body_only',[(provisional+['body'],installed['body'])],'actual provisional entry has no installed body'),
        ('binding_only',[(provisional+['bindings'],installed['bindings'])],'actual provisional entry has no installed binding'),
        ('body_and_binding',[(provisional+['body'],installed['body']),(provisional+['bindings'],installed['bindings'])],'actual provisional entry has no installed body'),
        ('installed_relation',[(['accepted',installed_index,'header','execution','stack',installed_stack_index,'bindings'],None)],'actual installed trigger requires complete binding'),
        ('waiting_omitted',[(['accepted',pending_index,'header','execution','waiting'],[])],'actual complete waiting inventory equals discovered minus installed'),
        ('waiting_duplicated',[(['accepted',pending_index,'header','execution','waiting'],pending['execution']['waiting']+[pending['execution']['waiting'][0]])],'actual waiting instance'),
    ]:exercise('RC.provisional-install',variant,death,patches,guard)
    foreign=next(value['kind']['source'] for value in pending['execution']['stack'] if value['entry']!=entry and value['kind']['source']!=pending['execution']['stack'][stack_index]['kind']['source'])
    exercise('RC.provisional-install','stack_source_relation',death,[(provisional+['kind','source'],foreign)],'actual provisional stack source is captured incarnation')
    cursor_index=next(i for i,row in enumerate(runs[token]['accepted']) if row['header']['execution']['resolution'] is not None);cursor=runs[token]['accepted'][cursor_index]['header']['execution']['resolution'];cursor_path=['accepted',cursor_index,'header','execution','resolution']
    exercise('RC.provisional-install','cursor_omitted',token,[(cursor_path,None),(['accepted',cursor_index,'header','instruction_index'],None)],'actual timestamp decision retains resolution cursor')
    exercise('RC.provisional-install','cursor_instruction',token,[(cursor_path+['instruction'],str(int(cursor['instruction'])+1)),(['accepted',cursor_index,'header','instruction_index'],int(cursor['instruction'])+1)],'actual timestamp creation/cursor instruction')
    other_entry=next(value['entry'] for row in actual['accepted'] for value in row['header']['execution']['stack'] if value['entry']!=cursor['entry'])
    exercise('RC.provisional-install','cursor_entry',token,[(cursor_path+['entry'],other_entry)],'actual timestamp creation/cursor entry')
    exercise('RC.provisional-install','header_instruction_index',token,[(['accepted',cursor_index,'header','instruction_index'],None)],('actual header instruction index','actual resolution instruction index'))
    order_index=next(i for i,row in enumerate(runs[token]['accepted']) if row['header']['pending'] is not None and row['header']['pending']['frame']['kind']=='triggers');waiting=runs[token]['accepted'][order_index]['header']['execution']['waiting'];order_path=['accepted',order_index,'header','execution','waiting']
    exercise('RC.provisional-install','order_waiting_omitted',token,[(order_path,[])],'actual complete waiting inventory equals discovered minus installed')
    exercise('RC.provisional-install','order_waiting_duplicated',token,[(order_path,waiting+[waiting[0]])],'actual waiting instance')
    frame_path=['accepted',pending_index,'header','pending'];wrong=41 if pending['pending']['chooser']==3 else 3
    exercise('RC.trigger-player-target','outer_chooser',death,[(frame_path+['chooser'],wrong),(frame_path+['frame','frame','chooser'],wrong)],'actual pending target captured controller')
    exercise('RC.trigger-player-target','inner_chooser',death,[(frame_path+['frame','frame','chooser'],wrong)],'actual inner frame chooser agrees with envelope')
    for field in ['chooser','event','boundary']:exercise('RC.trigger-player-target','inner_'+field+'_omitted',death,[(frame_path+['frame','frame',field],DELETE)],'exact actual trigger_target frame fields')
    domain=pending['pending']['frame']['frame']['domain'];exercise('RC.trigger-player-target','domain',death,[(frame_path+['frame','frame','domain'],domain[:-1])],('actual semantic player target membership','actual complete target membership','actual player target domain membership'))
    exercise('RC.trigger-player-target','provisional_target',death,[(provisional+['targets'],installed['targets'])],'actual provisional entry has no selected target')
    chosen=installed['targets'][0]['target']['player'];exercise('RC.trigger-player-target','installed_target',death,[(['accepted',installed_index,'header','execution','stack',installed_stack_index,'targets',0,'target','player'],41 if chosen==3 else 3)],'actual installed stack target equals recorded target occurrence')
    target_index=next(i for i,row in enumerate(actual['accepted']) if row['input']['command']['kind']=='reply' and row['input']['command']['choice']['kind']=='trigger_target');target=actual['accepted'][target_index]['input']['command']['choice']['target']['player']
    exercise('RC.trigger-player-target','selected_player',death,[(['accepted',target_index,'input','command','choice','target','player'],41 if target==3 else 3)],'actual target reply value')
    first_journal=next(i for i,row in enumerate(runs[one]['accepted']) if row['current_facts']['rules_journal'])
    for variant,tag in [('branch_forbidden','InstructionBranch'),('jump_forbidden','InstructionJump')]:
        path=['accepted',first_journal,'current_facts','rules_journal'];journal=runs[one]['accepted'][first_journal]['current_facts']['rules_journal'];exercise('RC.instruction-life-draw-order',variant,one,[(path,journal+[dict(tag=tag)])],'source19 forbids canonical rules journal record '+tag)
    birth_row,birth_index=next((i,j) for i,row in enumerate(runs[token]['accepted']) for j,event in enumerate(row['current_facts']['rules_journal']) if event['tag']=='Event' and any(occ['value']['tag']=='TokenCreated' for occ in event['occurrences']));birth=runs[token]['accepted'][birth_row]['current_facts']['rules_journal'][birth_index];created=next(occ['value'] for occ in birth['occurrences'] if occ['value']['tag']=='TokenCreated');choice=dict(id=dict(event=birth['event'],index=str(len(birth['occurrences']))),value=dict(tag='CreatureTypeChosen',entry=birth['cause']['entry'],instruction=str(birth['cause']['instruction']),slot='0',chooser=created['creator'],value=created['after']['creature_subtypes'][0]))
    exercise('RC.instruction-life-draw-order','creature_type_choice_forbidden',token,[(['accepted',birth_row,'current_facts','rules_journal',birth_index,'occurrences'],birth['occurrences']+[choice])],'source19 forbids canonical rules occurrence CreatureTypeChosen')
    last=len(runs[one]['accepted'])-1;exercise('RC.completion','bounded_phase',one,[(['accepted',last,'header','phase'],dict(kind='step_ended')),(['accepted',last,'header','priority_holder'],None)],'actual nonterminal stable input boundary is Ready')
    stack_index=next(i for i,row in enumerate(runs[token]['accepted']) if len(row['header']['execution']['stack'])>2 and all(value['bindings'] is not None for value in row['header']['execution']['stack']));stack=runs[token]['accepted'][stack_index]['header']['execution']['stack']
    exercise('RC.apnap-order','installed_stack_order',token,[(['accepted',stack_index,'header','execution','stack'],list(reversed(stack)))],'actual bottom-to-top stack order follows admission and completion')
    return rows


@evidence_boundary
def mandatory_native_controls(runs:dict[str,Any],run_dir:Path,authority:dict[str,Any])->list[dict[str,Any]]:
    """Exercise real Report3 branches independently; no capture, expected, or reference value is substituted."""
    manifest=read(Path(authority['authorities']['content']['path']));vocabulary=read(Path(authority['authorities']['vocabulary']['path']));cases={row['id']:row for row in authority['data']['cases']};primary={row['id']:row for row in authority['payloads']['full-games.json']['cases']+authority['payloads']['reduced-cases.json']['variants']};inputs=read(run_dir/'inputs.json');forge_cases={row['case_id']:row for row in inputs['cases']};references={};rows=[]
    def check(name:str,value:dict[str,Any])->dict[str,Any]:
        native=mandatory_native_projection(value,cases[name],manifest,vocabulary);facts=mandatory_native_primary_facts(native,value,cases[name],primary[name])
        if name not in references:
            raw=read(run_dir/'evidence'/f'{name}.json');references[name]=(raw,mandatory_forge_projection(raw,forge_cases[name],inputs))
        raw,reference=references[name];return dict(primary=facts,join=join_mandatory_common(native,reference,raw,cases[name]))
    def change(control:str,name:str,path:list[Any],value:Any,detail:str,variant:str|None=None,guard:str|tuple[str,...]|None=None)->None:
        row=exercise_mutation(control,'actual_native_Report3:'+name,runs[name],lambda d:replace(d,path,value),lambda d:check(name,d),detail)
        if variant is not None:
            require(guard is not None,'named native variant guard required');row=named_variant(row,variant,guard)
        rows.append(row)
    one='T01.opponent-active';token='T02.own-main';death='T06.artist-top';failed='T04.empty'
    change('RC.program-definition',one,['static_inputs','definitions',0,'program','names'],['Foreign source'],'alter whole actual compiled source program','whole_program','actual complete mandatory Program')
    change('RC.completion',failed,['completed'],False,'change actual completed native outcome into incomplete report','native_incomplete','actual native completed')
    change('RC.winner-loss-partition',failed,['final','outcome'],dict(kind='winner',seat=41),'replace actual failed-draw native winner','native_final','actual final outcome equals terminal native header')
    change('RC.physical-inventory',one,['static_inputs','aliases',0,'owner'],3 if runs[one]['static_inputs']['aliases'][0]['owner']==41 else 41,'replace actual physical owner','owner','actual physical owner')
    payment=next(i for i,r in enumerate(runs[token]['accepted']) if r['current_facts']['mana_payment'] is not None)
    change('RC.paid-source-cost',token,['accepted',payment,'current_facts','mana_payment','cost','generic'],'99','alter actual source-derived paid generic cost')
    pending=next(i for i,r in enumerate(runs[death]['accepted']) if r['header']['execution']['pending_trigger_placement'] is not None)
    change('RC.provisional-install',death,['accepted',pending,'header','execution','pending_trigger_placement'],None,'omit actual provisional/installed placement relation')
    change('RC.provisional-install',death,['accepted',pending,'header','execution','waiting'],[],'remove actual pending captured instance from waiting queue')
    journal=next((i,j) for i,r in enumerate(runs[death]['accepted']) for j,e in enumerate(r['current_facts']['rules_journal']) if e['tag']=='TriggerDiscovered')
    i,j=journal;change('RC.trigger-definition',death,['accepted',i,'current_facts','rules_journal',j,'value','definition'],999,'replace actual instance definition ordinal','definition','source first authored trigger definition')
    change('RC.trigger-complete-causes',death,['accepted',i,'current_facts','rules_journal',j,'value','causes','members'],[],'omit actual complete captured cause member')
    target=next(i for i,r in enumerate(runs[death]['accepted']) if r['input']['command']['kind']=='reply' and r['input']['command']['choice']['kind']=='trigger_target')
    change('RC.trigger-player-target',death,['accepted',target,'input','command','choice','target','player'],3 if runs[death]['accepted'][target]['input']['command']['choice']['target']['player']==41 else 41,'replace actual issued frame player reply')
    delta=next((i,j) for i,r in enumerate(runs[token]['accepted']) for j,d in enumerate(r['token_changes']) if d['kind']=='added');i,j=delta
    change('RC.token-endpoint-delta',token,['accepted',i,'token_changes',j],DELETE,'omit actual added-token endpoint delta','added_omitted',('actual complete token checkpoint vs deltas','actual complete checkpoint equals endpoint extraction/tokens'))
    change('RC.token-endpoint-delta',token,['accepted',i,'token_changes',j,'object'],'ObjectId(999999)','replace delta identity while retaining complete endpoint','identity','actual token delta same-identity endpoint')
    change('RC.token-template',token,['accepted',i,'token_changes',j,'after','creature','power'],'99','replace actual token endpoint power')
    probe_case='T09.fresh-next-turn';change('RC.decision-authority',probe_case,['rejection_probes',0,'no_candidate'],False,'adopt an actual rejected mana candidate','rejected_adopted','native rejected transition has no adopted candidate')
    change('RC.decision-authority',probe_case,['rejection_probes',0,'state_retained'],False,'discard actual rejected-input whole state retention','prefix','native rejection preserves whole state')
    change('RC.decision-authority',probe_case,['rejection_probes',0,'actual_error'],dict(path=['ForeignError'],payload=None),'replace actual independently checked typed rejection','error_type','actual typed error')
    change('RC.checkpoint-replay',token,['rerun','final_state_equal'],False,'remove actual complete final native rerun agreement','final_state_mismatch','actual saved-input rerun final_state_equal')
    checkpoint=next(k for k,v in runs[token]['checkpoints'].items() if v['tokens']);change('RC.checkpoint-replay',token,['checkpoints',checkpoint,'tokens'],{},'drop actual complete token checkpoint inventory','checkpoint_tokens',('actual complete token checkpoint vs deltas','actual complete checkpoint equals endpoint extraction/tokens'))
    failure=next((i,j,k) for i,r in enumerate(runs[failed]['accepted']) for j,e in enumerate(r['current_facts']['rules_journal']) if e['tag']=='Event' for k,o in enumerate(e['occurrences']) if o['value']['tag']=='FailedDraw');i,j,k=failure
    change('RC.failed-draw-count',failed,['accepted',i,'current_facts','rules_journal',j,'occurrences',k,'value','attempts'],'0','remove real failed attempts while preserving body completion and outcome')
    change('RC.failed-draw-count',failed,['accepted',i,'current_facts','rules_journal',j],DELETE,'omit real failed-draw instruction transaction','attempt_omitted','actual native exact successful and failed draw attempt denominator')
    for coherent in [False,True]:
        exercised=exercise_mutation('RC.instruction-life-draw-order','actual_native_Report3:'+failed,runs[failed],lambda value,c=coherent:insert_native_body_check(value,i,c),lambda value:check(failed,value),'insert extra empty SBA inside real body with '+('coherent chronological EventId remap' if coherent else 'forged nonmonotonic EventId'))
        require(('no actual native state-based check inside a mandatory body' if coherent else 'actual chronological native event identities') in exercised['reason'],'actual intermediate SBA control reaches intended relationship guard');rows.append(named_variant(exercised,'intermediate_sba','no actual native state-based check inside a mandatory body') if coherent else exercised)
    aliases=runs[one]['static_inputs']['aliases']
    for variant,value,guard in [('alias_duplicate',aliases+[aliases[0]],'duplicate native physical alias'),('alias_omitted',aliases[1:],'actual exact mandatory physical inventory'),('alias_foreign',[dict(aliases[0],alias='P0.foreign-alias.1')]+aliases[1:],'actual exact mandatory physical inventory')]:change('RC.physical-inventory',one,['static_inputs','aliases'],value,'alter actual physical '+variant,variant,guard)
    other_definition=next(row['definition_ref'] for row in aliases if row['definition_ref']!=aliases[0]['definition_ref']);change('RC.physical-inventory',one,['static_inputs','aliases',0,'definition_ref'],other_definition,'replace actual physical source by another real declared definition','source_foreign','actual physical source definition')
    definition_index=next(index for index,value in enumerate(runs[token]['static_inputs']['definitions']) if value['name']=='Raise the Alarm');creation=['static_inputs','definitions',definition_index,'program','kind','body','instructions',0,'creations',0,'definition']
    change('RC.program-definition',token,creation+['name','ordered_subtypes',0,'type','ordinal'],'25','change actual default token name subtype while preserving its typed vocabulary','default_definition','actual complete mandatory Program')
    change('RC.program-definition',token,creation+['program','card_types'],['Enchantment','Creature'],'add another actual card-type variant to the typed token Program','typed_definition','actual complete mandatory Program')
    mana=next(value['program']['mana'] for value in runs['T09.fresh-next-turn']['static_inputs']['definitions'] if value['name']=='Llanowar Visionary')
    change('RC.token-template',token,creation+['program','mana'],mana,'add an actual printed mana ability to the source-defined Soldier token','mana_abilities','actual complete mandatory Program')
    change('RC.token-template',token,creation+['program','continuous'],{},'replace required absent static program with an extra continuous clause','continuous','actual complete mandatory Program')
    last=len(runs[one]['accepted'])-1;holder=runs[one]['accepted'][last]['header']['priority_holder'];wrong_holder=3 if holder==41 else 41
    patches=[(['accepted',last,'header','phase','phase','holder'],wrong_holder),(['accepted',last,'header','priority_holder'],wrong_holder),(['final','holder'],wrong_holder)]
    rows.append(named_variant(exercise_mutation('RC.completion','actual_native_Report3:'+one,runs[one],lambda data:changed(data,patches),lambda data:check(one,data),'coherently change both current and summary holder at the frozen bounded stop'),'bounded_boundary','actual frozen bounded stop requires Ready phase'))
    terminal=len(runs[failed]['accepted'])-1
    change('RC.winner-loss-partition',failed,['accepted',terminal,'header','outcome'],dict(kind='winner',seat=41),'change only actual terminal native header winner','native_header','actual final outcome equals terminal native header')
    change('RC.winner-loss-partition',failed,['accepted',terminal,'header','losses'],[3],'change actual terminal loss partition while retaining complete loss journal','native_losses','actual final loser partition equals complete actual loss journal')
    change('RC.decision-authority',token,['rejection_probes',0,'actual_error'],DELETE,'omit actually retained typed rejection','error_omitted',"malformed evidence: 'actual_error'")
    change('RC.decision-authority',token,['rejection_probes',0,'continuation_ordinal'],DELETE,'omit actual successful immediate continuation relation','continuation_omitted',"malformed evidence: 'continuation_ordinal'")
    checkpoint='1';checkpoints=dict(runs[token]['checkpoints']);del checkpoints[checkpoint]
    rows.append(named_variant(exercise_mutation('RC.checkpoint-replay','actual_native_Report3:'+token,runs[token],lambda data:replace(data,['checkpoints'],checkpoints),lambda data:check(token,data),'omit one actual checkpoint while preserving the exact actual replay ordinal inventory'),'checkpoint_omitted','actual complete per-ordinal checkpoint inventory'))
    birth_row,birth_index=next((i,j) for i,row in enumerate(runs[token]['accepted']) for j,event in enumerate(row['current_facts']['rules_journal']) if event['tag']=='Event' and sum(occ['value']['tag']=='TokenCreated' for occ in event['occurrences'])==2)
    for control,variant in [('RC.simultaneous-cohort','batch_split'),('RC.token-birth-lineage','cohort')]:rows.append(named_variant(exercise_mutation(control,'actual_native_Report3:'+token,runs[token],lambda data:split_native_token_birth(data,birth_row,birth_index),lambda data:check(token,data),'split the actual two-member birth Event with coherent moved occurrence and later EventId references'),variant,'actual unique birth lineage'))
    creator=runs[token]['accepted'][birth_row]['current_facts']['rules_journal'][birth_index]['cause']['entry'];other_entry=next(value['entry'] for row in runs[token]['accepted'] for value in row['header']['execution']['stack'] if value['entry']!=creator)
    change('RC.token-birth-lineage',token,['accepted',birth_row,'current_facts','rules_journal',birth_index,'cause','entry'],other_entry,'bind actual token birth to another real admitted entry','entry','actual body and resolution execute the current live top')
    discovered=next(j for j,event in enumerate(runs[token]['accepted'][birth_row]['current_facts']['rules_journal']) if event['tag']=='TriggerDiscovered');change('RC.simultaneous-cohort',token,['accepted',birth_row,'current_facts','rules_journal',discovered],DELETE,'omit one actual watcher from the complete simultaneous birth cohort','instance_omitted','actual complete waiting inventory equals discovered minus installed')
    paid=next(i for i,row in enumerate(runs[token]['accepted']) if row['current_facts']['mana_payment'] is not None);patches=[(['accepted',paid,'input','command','payment','spend',0,'amount'],'99'),(['accepted',paid,'current_facts','mana_payment','spend',0,'amount'],'99')]
    rows.append(named_variant(exercise_mutation('RC.paid-source-cost','actual_native_Report3:'+token,runs[token],lambda data:changed(data,patches),lambda data:check(token,data),'coherently change submitted and returned spend while retaining actual printed cost'),'spend','actual exact finite spend'))
    failed_row,draw_event=next((i,j) for i,row in enumerate(runs[failed]['accepted']) for j,event in enumerate(row['current_facts']['rules_journal']) if event['tag']=='Event' and any(occ['value']['tag']=='FailedDraw' for occ in event['occurrences']));journal=runs[failed]['accepted'][failed_row]['current_facts']['rules_journal'];life_event=next(j for j,event in enumerate(journal) if event['tag']=='Event' and any(occ['value']['tag']=='LifeChanged' for occ in event['occurrences']))
    rows.append(named_variant(exercise_mutation('RC.instruction-life-draw-order','actual_native_Report3:'+failed,runs[failed],lambda data:exchange_native_events(data,failed_row,draw_event,life_event),lambda data:check(failed,data),'reverse actual draw/life effects with coherent chronological EventIds and references'),'draw_loss_order','actual native draw before life loss'))
    change('RC.instruction-life-draw-order',failed,['accepted',failed_row,'current_facts','rules_journal',draw_event],DELETE,'omit actual source-required draw transaction','draw_omitted','actual native exact successful and failed draw attempt denominator')
    change('RC.instruction-life-draw-order',failed,['accepted',failed_row,'current_facts','rules_journal',life_event],DELETE,'omit actual source-required life transaction','life_omitted','actual native complete ordered life body')
    artist_row,life_indices=next((i,[j for j,event in enumerate(row['current_facts']['rules_journal']) if event['tag']=='Event' and any(occ['value']['tag']=='LifeChanged' for occ in event['occurrences'])]) for i,row in enumerate(runs[death]['accepted']) if sum(event['tag']=='Event' and any(occ['value']['tag']=='LifeChanged' for occ in event.get('occurrences',[])) for event in row['current_facts']['rules_journal'])==2)
    rows.append(named_variant(exercise_mutation('RC.instruction-life-draw-order','actual_native_Report3:'+death,runs[death],lambda data:exchange_native_events(data,artist_row,*life_indices),lambda data:check(death,data),'reverse actual target-loss/controller-gain effects with coherent chronological EventIds and references'),'target_loss_gain_order','actual native complete ordered life body'))
    lost_row,lost_event,lost_occurrence=next((i,j,k) for i,row in enumerate(runs[failed]['accepted']) for j,event in enumerate(row['current_facts']['rules_journal']) if event['tag']=='Event' for k,occ in enumerate(event['occurrences']) if occ['value']['tag']=='PlayerLost')
    change('RC.failed-draw-count',failed,['accepted',lost_row,'current_facts','rules_journal',lost_event,'occurrences',lost_occurrence,'value','reasons'],['NonpositiveLife'],'replace actual failed-draw cause partition by life loss while life remains positive','failure_partition','/required_loss_causes')
    terminal=len(runs[failed]['accepted'])-1;wrong_winner=dict(kind='winner',seat=41);patches=[(['accepted',terminal,'header','outcome'],wrong_winner),(['accepted',terminal,'header','losses'],[3]),(['final','outcome'],wrong_winner),(['accepted',lost_row,'current_facts','rules_journal',lost_event,'occurrences',lost_occurrence,'value','player'],3)]
    rows.append(named_variant(exercise_mutation('RC.winner-loss-partition','actual_native_Report3:'+failed,runs[failed],lambda data:changed(data,patches),lambda data:check(failed,data),'coherently change header/final winner and actual terminal loss journal/partition'),'coherent_native_outcome','primary/native '+failed+'/'))
    fresh='T09.fresh-next-turn';printed_row=printed_event=printed_occurrence=None
    for i,row in enumerate(runs[fresh]['accepted']):
        production=row['current_facts']['mana_production']
        if production is not None and production['source']['alias']=='P0.llanowar-visionary.1':
            printed_row=i;printed_event,printed_occurrence=next((j,k) for j,event in enumerate(row['current_facts']['rules_journal']) if event['tag']=='Event' for k,occ in enumerate(event['occurrences']) if occ['value']['tag']=='ManaProduced');break
    require(printed_row is not None,'actual next-own-turn printed mana production')
    change('RC.creature-mana-age',fresh,['accepted',printed_row,'current_facts','rules_journal',printed_event],DELETE,'omit actual successful next-own-turn printed mana result','next_turn_omitted','/printed_ability')
    change('RC.creature-mana-age',fresh,['accepted',printed_row,'current_facts','rules_journal',printed_event,'occurrences',printed_occurrence,'value','production','produced','Green'],'2','change actual one-Green printed mana result to two','printed_one_green','/printed_ability')
    stale='T09.bounce-recast-stale';stale_probe=next(index for index,probe in enumerate(runs[stale]['rejection_probes']) if probe['expectation_ref']=='T09.StaleIncarnation');change('RC.creature-mana-age',stale,['rejection_probes',stale_probe,'no_candidate'],False,'admit a candidate for the actual rejected stale incarnation','stale','native rejected transition has no adopted candidate')
    rows.extend(mandatory_lifecycle_controls(runs,check))
    return rows


@evidence_boundary
def mandatory_observer_controls(document:dict[str,Any])->list[dict[str,Any]]:
    """Mutate actual OB01 nonzero stored fields and endpoint deltas through the shared reader."""
    rows=[];observations=document['observations'];identity=next(iter(observations[9]['after']['tokens']))
    def coherent_stored(value:Any,group:str,field:str,old:str,replacement:Any)->Any:
        # Update only actual occurrences of the same token's nonzero stored value
        # across endpoints, delta before/after, checkpoints and ancestry copies.
        # This reaches the stored-field/value guard instead of an earlier stale
        # checkpoint copy. Unchanged branches remain shared.
        if isinstance(value,dict):
            updates={key:coherent_stored(member,group,field,old,replacement) for key,member in value.items()};updates={key:member for key,member in updates.items() if member is not value[key]}
            if set(value)=={'object','program','characteristics','creature','counters'} and value['object']['object']==identity and isinstance(value[group],dict) and value[group].get(field)==old:
                changed_group=dict(value[group])
                if replacement is DELETE:del changed_group[field]
                else:changed_group[field]=replacement
                updates[group]=changed_group
            return dict(value,**updates) if updates else value
        if isinstance(value,list):
            result=[coherent_stored(member,group,field,old,replacement) for member in value];return result if any(left is not right for left,right in zip(result,value,strict=True)) else value
        return value
    for control,group,field,old,required_guard,value_guard in [('RC.token-stored-marks','creature','marked_damage','1','OB01 required stored marks independently of derived PT','OB01 actual derived PT and stored marks damage'),('RC.token-stored-counters','counters','plus_one','2','OB01 required complete stored counters','OB01 actual stored counter inventory counters')]:
        same(observations[9]['after']['tokens'][identity][group][field],old,'actual OB01 nonzero mutation prerequisite')
        for variant,value,guard in [('field_omitted',DELETE,required_guard),('nonzero_to_zero','0',value_guard)]:
            row=exercise_mutation(control,'actual_test_only_OB01',document,lambda data,g=group,f=field,o=old,v=value:coherent_stored(data,g,f,o,v),validate_token_observer,'coherently '+variant+' across actual token copies, preserving derived4/4');rows.append(named_variant(row,variant,guard))
    def exercise(control:str,path:list[Any],value:Any,detail:str,variant:str,guard:str)->None:rows.append(named_variant(exercise_mutation(control,'actual_test_only_OB01',document,lambda d:replace(d,path,value),validate_token_observer,detail),variant,guard))
    for kind in ['added','updated','removed']:
        i,j=next((i,j) for i,r in enumerate(observations) for j,d in enumerate(r['token_changes']) if d['kind']==kind);exercise('RC.token-endpoint-delta',['observations',i,'token_changes',j],DELETE,'omit actual OB01 '+kind+' endpoint delta',kind+'_omitted','actual OB01 complete endpoint delta inventory')
        delta=observations[i]['token_changes'][j]
        for side in (['after'] if kind=='added' else ['before','after'] if kind=='updated' else ['before']):
            owner=delta[side]['object']['owner'];guard='actual complete token delta preimage' if side=='before' else 'actual OB01 complete endpoint delta inventory';exercise('RC.token-endpoint-delta',['observations',i,'token_changes',j,side,'object','owner'],owner+1,'alter actual OB01 '+kind+' '+side+' complete endpoint',kind+'_'+side,guard)
        if kind=='added':exercise('RC.token-endpoint-delta',['observations',i,'token_changes',j,'object'],'ObjectId(999999)','substitute another delta identity while retaining actual endpoint','identity','actual token delta same-identity endpoint')
    exercise('RC.token-endpoint-delta',['observations',0,'token_changes'],DELETE,'remove required empty endpoint delta array','required_empty_omitted',"validate_token_observer malformed evidence: 'token_changes'")
    return rows


def mandatory_authority_controls(authority:dict[str,Any],document:dict[str,Any],binding:dict[str,Any],case_row:dict[str,Any])->list[dict[str,Any]]:
    """Exercise actual source receipts and byte-frozen supplied authority intake.

    Mutations use copied receipts or fresh isolated files. Original catalogue,
    metadata, vocabulary, D06 publication and frozen planning inputs stay intact.
    """
    rows=[];witness=document['type_context_witness'];sources=witness['source_inputs'];pins=compare_reader.NATIVE_INPUT_PINS
    for variant,kind in [('compressed_source','snapshot'),('metadata','metadata')]:
        indices=[index for index,row in enumerate(sources) if row['sha256']==pins[kind]];same(len(indices),1,'actual source authority control substrate '+variant);index=indices[0]
        row=exercise_mutation('RC.source-authority','actual_captured_source_receipts',document,lambda data,i=index:replace(data,['type_context_witness','source_inputs',i,'sha256'],'0'*64),lambda data:validate_captured_sources(data,binding,case_row),'change actual '+variant+' receipt digest');rows.append(named_variant(row,variant,'actual witnessed source bytes'))
    for variant,field in [('vocabulary','input'),('publication','publication')]:
        row=exercise_mutation('RC.source-authority','actual_captured_vocabulary_receipts',document,lambda data,f=field:replace(data,['type_context_witness','vocabulary',f,'sha256'],'0'*64),lambda data:validate_captured_sources(data,binding,case_row),'change actual '+variant+' receipt digest');rows.append(named_variant(row,variant,'actual witness '+variant+' file binding'))
    supplied=tuple(Path(authority['supplied'][key]['path']) for key in ['primary-index','compiled-input','requirements']);root=Path(authority['source_root'])
    @evidence_boundary
    def check(paths:tuple[Path,Path,Path])->dict[str,Any]:
        return mandatory_authorities(root,*paths)
    with tempfile.TemporaryDirectory(prefix='mandatory-authority-controls-') as directory:
        for variant,index,label in [('primary',0,'primary-index'),('compiled_input',1,'compiled-input')]:
            original=supplied[index];before=sha(original);copy=Path(directory)/(variant+'.json');shutil.copyfile(original,copy)
            same(sha(copy),before,'actual isolated authority copy equals original')
            with copy.open('ab') as output:output.write(b' ')
            changed_paths=tuple(copy if slot==index else path for slot,path in enumerate(supplied))
            row=exercise_mutation('RC.source-authority','actual_frozen_authority_bytes',supplied,lambda paths,v=changed_paths:v,check,'change only copied '+variant+' bytes while preserving otherwise valid JSON');rows.append(named_variant(row,variant,'Mandatory frozen authority mismatch: '+label));same(sha(original),before,'actual frozen authority original remains unchanged')
    return rows


def mandatory_api_controls(original:Path)->list[dict[str,Any]]:
    """Stream four real whole-API mutations while preserving the full data value."""
    rows=[];before=sha(original);prefix=b'{"status":"success","data":';suffix=b'}\n'
    with original.open('rb') as source:
        same(source.read(len(prefix)),prefix,'actual whole API mutation prefix')
        source.seek(-len(suffix),2);same(source.read(),suffix,'actual whole API mutation suffix')
    def payload(source:Any,output:Any)->None:
        same(source.read(len(prefix)),prefix,'actual API prefix while copying');remaining=original.stat().st_size-len(prefix)-len(suffix)
        require(remaining>0,'actual complete API data payload exists')
        while remaining:
            block=source.read(min(1024*1024,remaining));require(bool(block),'complete actual API data copied');output.write(block);remaining-=len(block)
        same(source.read(),suffix,'actual API suffix after complete data copy')
    with tempfile.TemporaryDirectory(prefix='mandatory-api-controls-') as directory:
        for variant,start,end in [('field_order',b'{"data":',b',"status":"success"}\n'),('field_name',b'{"Status":"success","data":',suffix),('field_omitted',b'{"data":',suffix),('field_content',b'{"status":"fail","data":',suffix)]:
            changed_file=Path(directory)/(variant+'.api.json')
            with original.open('rb') as source,changed_file.open('xb') as output:
                output.write(start);payload(source,output);output.write(end)
            row=exercise_mutation('RC.old-api-bytes','actual_whole_predecessor_API',original,lambda path,p=changed_file:p,lambda path:equal_api_bytes(original,path),'change '+variant+' only at the actual typed API boundary, preserving every data byte');rows.append(named_variant(row,variant,'whole predecessor API bytes including field order/name/omission'))
    same(sha(original),before,'actual whole API original unchanged after all variants')
    return rows


def mandatory_image_controls(binding:dict[str,Any],authority:dict[str,Any])->list[dict[str,Any]]:
    """Reach the maintained image/process/source guard with changed real index data."""
    rows=[]
    def check(value:dict[str,Any])->Any:return validate_image_binding(value,PROFILES[1],authority['requirements'])
    cases=binding['cases'];require(len(cases)==23,'actual complete image mutation substrate')
    for variant,value,guard in [('index_remove',cases[:-1],'actual exact capture case order'),('index_duplicate',cases+[cases[0]],'duplicate native image case'),('index_foreign',[dict(cases[0],caseId='foreign-case')]+cases[1:],'actual receipt case')]:
        row=exercise_mutation('RC.case-inventory','actual_final_image_index',binding,lambda data,v=value:replace(data,['cases'],v),check,'change actual indexed '+variant);rows.append(named_variant(row,variant,guard))
    row=exercise_mutation('RC.native-witness-bindings','actual_final_image_runtime',binding,lambda data:replace(data,['runtime','sha256'],'0'*64),check,'alter actual copied CLI runtime digest');rows.append(named_variant(row,'image','final native runtime actual bytes'))
    return rows


def mandatory_capture_reader(binding:dict[str,Any],case_row:dict[str,Any],authority:dict[str,Any])->Callable[[dict[str,Any]],dict[str,Any]]:
    """Reuse the maintained native half of the full pipeline for small capture mutations."""
    manifest=read(Path(authority['authorities']['content']['path']));vocabulary=read(Path(authority['authorities']['vocabulary']['path']));case=next(row for row in authority['data']['cases'] if row['id']==case_row['caseId']);primary=next(row for row in authority['payloads']['full-games.json']['cases']+authority['payloads']['reduced-cases.json']['variants'] if row['id']==case_row['caseId'])
    @evidence_boundary
    def check(value:dict[str,Any])->dict[str,Any]:
        report,provenance=unwrap_native(value,False,manifest,PROFILES[1]);validate_captured_sources(value,binding,case_row);actual=report['runs'][0];projected=mandatory_native_projection(actual,case,manifest,vocabulary);facts=mandatory_native_primary_facts(projected,actual,case,primary);return dict(case=actual['fixture_ref'],provenance=provenance,primary=facts)
    return check


def mandatory_capture_controls(document:dict[str,Any],binding:dict[str,Any],case_row:dict[str,Any],authority:dict[str,Any])->list[dict[str,Any]]:
    """Exercise complete real call/point/input/state/source relations on a small capture."""
    rows=[];check=mandatory_capture_reader(binding,case_row,authority);witness=document['type_context_witness'];calls=witness['calls'];points=witness['snapshots'];run=document['observations']['data']['runs'][0]
    def exercise(variant:str,patches:list[tuple[list[Any],Any]],guard:str|tuple[str,...],control:str='RC.native-witness-bindings')->None:
        row=exercise_mutation(control,'actual_complete_capture:'+run['fixture_ref'],document,lambda data:changed(data,patches),check,variant);rows.append(named_variant(row,variant,guard))
    successful=[(index,row) for index,row in enumerate(calls) if row['input']['tag']=='GameInput' and row['result']['tag']=='Ok'];require(len(successful)>1,'actual two successful calls for cross-state controls');call_index,call=successful[0];other=successful[1][1];require(bool(call['append']['value']),'actual nonempty returned journal for append mutation');point_index=next(index for index,row in enumerate(points) if row['point_id']==call['after_point']);other_index=next(index for index,row in enumerate(points) if row['point_id']==other['after_point']);base=['type_context_witness'];point=base+['snapshots',point_index];call_path=base+['calls',call_index]
    exercise('point_basis',[(point+['projection','value','basis','revision'],'999999')],'actual whole candidate state equals after Inline state')
    exercise('point_context',[(point+['native_context','value','profile'],'foreign-profile')],'actual point native context owns real state')
    exercise('point_vocabulary',[(point+['native_vocabulary','value'],'0'*64)],'actual point native vocabulary owns real state')
    exercise('point_source_owner',[(point+['owner_source'],'foreign-producer')],'actual after-point source method owner')
    own=points[point_index]['observation_pointers'][0];foreign=points[other_index]['observation_pointers'][0];joins=[]
    for join in witness['coverage']['observation_to_points']:
        if join['pointer']==own:continue
        joins.append(dict(join,points=join['points']+[points[point_index]['point_id']]) if join['pointer']==foreign else join)
    exercise('point_pointer',[(point+['observation_pointers'],[foreign]),(base+['coverage','observation_to_points'],joins)],'actual native Inline point owns its exact emitted slot')
    actor=call['input']['value']['actor'];wrong_actor=next(seat for seat in run['initialization']['header']['participants'] if seat!=actor)
    exercise('call_actor',[(call_path+['input','value','actor'],wrong_actor)],'actual saved complete typed input/report relation')
    exercise('call_input',[(call_path+['input','value','basis','revision'],'999999')],'actual saved complete typed input/report relation')
    exercise('call_result_basis',[(call_path+['result','candidate','state','basis','revision'],'999999')],'actual whole candidate state equals after Inline state')
    exercise('call_adoption',[(call_path+['adopted'],False)],'actual successful ordinary call adopted')
    exercise('call_append',[(call_path+['append','value'],[])],'actual returned complete journal equals call append')
    exercise('call_before',[(call_path+['before_point'],other['before_point'])],'actual complete before-state equals latest adopted source')
    exercise('call_after',[(call_path+['after_point'],other['after_point'])],'actual whole candidate state equals after Inline state')
    before_index=next(index for index,row in enumerate(points) if row['point_id']==call['before_point']);exercise('call_owner_method',[(call_path+['owner_method'],'ForeignState::advance'),(base+['snapshots',before_index,'owner_source'],'ForeignState::advance'),(point+['owner_source'],'ForeignState::advance')],'actual root advance owner method')
    error_index=next(index for index,row in enumerate(calls) if row['result']['tag']=='Err');exercise('call_error_leaf_domain',[(base+['calls',error_index,'result','error_leaf','domain'],'ForeignErrorDomain')],'actual typed native error leaf domain')
    exercise('source_digest',[(base+['source_inputs',0,'sha256'],'0'*64)],'actual witnessed source bytes')
    exercise('source_omitted',[(base+['source_inputs'],witness['source_inputs'][1:])],'actual required34 source inputs')
    exercise('source_empty',[(base+['source_inputs'],[])],'actual required34 source inputs')
    receipt=read(Path(case_row['executionReceipt']['path']));source=Path(receipt['cwd'])/'crates/engine/src/turn.rs';require(str(source) not in {row['path'] for row in witness['source_inputs']},'actual foreign source member control prerequisite');actual_foreign=dict(path=str(source),sha256=sha(source),bytes=source.stat().st_size)
    exercise('source_foreign',[(base+['source_inputs',0],actual_foreign)],'actual exact complete declared mandatory source inventory')
    exercise('record_pointers_empty',[(base+['coverage','record_pointers'],[])],'actual complete native record-pointer inventory')
    exercise('coverage_incomplete',[(base+['coverage','complete'],False)],'actual complete witness')
    exercise('vocabulary',[(base+['vocabulary','value','id'],'0'*64)],'actual complete published vocabulary value')
    exercise('registry',[(base+['registry','coverage','registryVersion'],6)],'actual mandatory registry7')
    exercise('profile',[(['profile'],'interaction')],'actual current Mandatory outer profile')
    exercise('current_schema',[(['schema_version'],8)],'actual current schema9')
    exercise('witness_schema',[(base+['schema'],'spellwake.current-type-context-witness.v6')],'actual witness7')
    runs=document['observations']['data']['runs']
    exercise('native_case_remove',[(['observations','data','runs'],[])],'exactly one actual run per mandatory capture','RC.case-inventory')
    exercise('native_case_duplicate',[(['observations','data','runs'],runs+[runs[0]])],'exactly one actual run per mandatory capture','RC.case-inventory')
    exercise('native_case_foreign',[(['observations','data','runs',0,'fixture_ref'],'foreign-case')],'actual run/static-input case identity','RC.case-inventory')
    setup=run['static_inputs']['setup'];card_index=next(index for index,row in enumerate(setup['cards']) if row['destination']=='battlefield');controller=setup['cards'][card_index]['controller'];wrong_controller=next(seat for seat in setup['participants'] if seat!=controller);report_path=['observations','data','runs',0]
    exercise('controller',[(report_path+['static_inputs','setup','cards',card_index,'controller'],wrong_controller)],'actual complete declared initializer seed','RC.physical-inventory')
    order=setup['initial_battlefield_order'];require(len(order)>1,'actual plural global battlefield order');exercise('global_order',[(report_path+['static_inputs','setup','initial_battlefield_order'],list(reversed(order)))],'actual complete declared initializer seed','RC.physical-inventory')
    ordinal=next(index for index,row in enumerate(run['accepted']) if row['current_facts']['rules_journal']);journal=run['accepted'][ordinal]['current_facts']['rules_journal']
    exercise('journal_mismatch',[(report_path+['accepted',ordinal,'current_facts','rules_journal'],journal[:-1])],'actual all13 complete returned candidate fact families','RC.checkpoint-replay')
    exercise('input_mismatch',[(report_path+['accepted',0,'input','actor'],wrong_actor)],'actual saved complete typed input/report relation','RC.checkpoint-replay')
    exercise('owner_counts',[(report_path+['accepted',0,'header','counts_by_owner',str(actor),'library'],999)],'actual complete Header owner counts derive from World objects','RC.physical-inventory')
    exercise('controller_counts',[(report_path+['accepted',0,'header','battlefield_counts_by_controller',str(actor)],999)],'actual complete Header controller counts derive from World objects','RC.physical-inventory')
    exercise('initialization_checkpoint',[(report_path+['initialization','checkpoint_ordinal'],999)],'actual initializer checkpoint ordinal zero','RC.checkpoint-replay')
    exercise('checkpoint_replay_duplicate',[(report_path+['rerun','checkpoint_ordinals_compared'],run['rerun']['checkpoint_ordinals_compared']+[0])],'every complete checkpoint reexecuted exactly once','RC.checkpoint-replay')
    replayed=run['rerun']['probe_ids_compared'];require(bool(replayed),'actual nonempty rejected-probe replay control substrate');exercise('probe_replay_duplicate',[(report_path+['rerun','probe_ids_compared'],replayed+[replayed[0]])],'every actual rejected probe reexecuted exactly once','RC.checkpoint-replay')
    return rows


def mandatory_auxiliary_capture_controls(binding:dict[str,Any],authority:dict[str,Any])->list[dict[str,Any]]:
    """Exercise actual ordered piles, departed draw, birth, death and cessation records.

    Only four named reduced captures are loaded one at a time. Every mutation
    reaches the same complete capture/source/native/primary pipeline; final image
    admission remains a separate prerequisite. Large opening packets are excluded.
    """
    indexed={row['caseId']:row for row in binding['cases']};rows=[]
    for name in ['T09.fresh-next-turn','T03.departed-recast','T07.death-birth-entry','T11.token-death-cessation']:
        case_row=indexed[name];path=Path(case_row['path']);require(path.stat().st_size<=32*1024*1024,'actual bounded small mutation substrate');before=sha(path);same(before,case_row['sha256'],'actual auxiliary indexed packet bytes');document=read(path);run=document['observations']['data']['runs'][0];check=mandatory_capture_reader(binding,case_row,authority);prefix=['observations','data','runs',0]
        def exercise(control:str,variant:str,mutate:Callable[[Any],Any],guard:str)->None:
            row=exercise_mutation(control,'actual_complete_capture:'+name,document,mutate,check,variant);rows.append(named_variant(row,variant,guard))
        if name=='T09.fresh-next-turn':
            pile=next(row for row in run['checkpoints']['0']['ordered_piles'] if row['pile']['kind']=='library' and len(row['objects_top_to_bottom'])>1)
            def reverse_pile(value:Any)->Any:
                if isinstance(value,dict):
                    if set(value)=={'pile','objects_top_to_bottom'} and value==pile:return dict(value,objects_top_to_bottom=list(reversed(value['objects_top_to_bottom'])))
                    updates={key:reverse_pile(member) for key,member in value.items()};updates={key:member for key,member in updates.items() if member is not value[key]};return dict(value,**updates) if updates else value
                if isinstance(value,list):
                    mapped=[reverse_pile(member) for member in value];return mapped if any(a is not b for a,b in zip(mapped,value,strict=True)) else value
                return value
            exercise('RC.physical-inventory','pile_order',lambda data:replace(data,prefix,reverse_pile(run)),'actual canonical candidate complete Header/World relation')
        elif name=='T03.departed-recast':
            i,j=next((i,j) for i,row in enumerate(run['accepted']) for j,event in enumerate(row['current_facts']['rules_journal']) if event['tag']=='Event' and event['cause']['tag']=='Draw' and any(occ['value']['tag']=='ZoneChange' and occ['value']['before']['object']['zone']['kind']=='library' and occ['value']['after']['object']['zone']['kind']=='hand' for occ in event['occurrences']))
            exercise('RC.departed-trigger','captured_draw_cancelled',lambda data:replace(data,prefix+['accepted',i,'current_facts','rules_journal',j],DELETE),'actual all13 complete returned candidate fact families')
        elif name=='T07.death-birth-entry':
            i,j=next((i,j) for i,row in enumerate(run['accepted']) for j,event in enumerate(row['current_facts']['rules_journal']) if event['tag']=='Event' and any(occ['value']['tag']=='TokenCreated' for occ in event['occurrences']))
            exercise('RC.death-birth-entry-chain','creation_omitted',lambda data:replace(data,prefix+['accepted',i,'current_facts','rules_journal',j],DELETE),'actual all13 complete returned candidate fact families')
        else:
            i,j,k=next((i,j,k) for i,row in enumerate(run['accepted']) for j,event in enumerate(row['current_facts']['rules_journal']) if event['tag']=='Event' for k,occ in enumerate(event['occurrences']) if occ['value']['tag']=='ZoneChange' and occ['value']['before']['object']['physical_card'] is None and occ['value']['before']['object']['zone']['kind']=='battlefield' and occ['value']['after']['object']['zone']['kind']=='graveyard');event=run['accepted'][i]['current_facts']['rules_journal'][j];remaining=[occ for index,occ in enumerate(event['occurrences']) if index!=k];remaining=[dict(occ,id=dict(event=event['event'],index=str(index))) for index,occ in enumerate(remaining)]
            exercise('RC.token-transient-event','death_omitted',lambda data:replace(data,prefix+['accepted',i,'current_facts','rules_journal',j,'occurrences'],remaining),'actual all13 complete returned candidate fact families')
            after=event['occurrences'][k]['value']['after'];exercise('RC.token-transient-event','before_incarnation',lambda data:replace(data,prefix+['accepted',i,'current_facts','rules_journal',j,'occurrences',k,'value','before'],after),'actual all13 complete returned candidate fact families')
            cease_i,cease_j,cease_k=next((i,j,k) for i,row in enumerate(run['accepted']) for j,event in enumerate(row['current_facts']['rules_journal']) if event['tag']=='Event' for k,occ in enumerate(event['occurrences']) if occ['value']['tag']=='TokenCeased');event=run['accepted'][cease_i]['current_facts']['rules_journal'][cease_j];remaining=[occ for index,occ in enumerate(event['occurrences']) if index!=cease_k];remaining=[dict(occ,id=dict(event=event['event'],index=str(index))) for index,occ in enumerate(remaining)]
            exercise('RC.token-transient-event','cessation_omitted',lambda data:replace(data,prefix+['accepted',cease_i,'current_facts','rules_journal',cease_j,'occurrences'],remaining),'actual all13 complete returned candidate fact families')
        same(sha(path),before,'actual auxiliary capture original restored and unchanged')
    return rows


@evidence_boundary
def mandatory_final_controls(run_dir:Path,native_paths:list[Path],binding:dict[str,Any],supplemental:dict[str,Any],primary_index:Path,compiled_input:Path,requirements:Path)->dict[str,Any]:
    """Require the full final baseline, then baseline/mutation/restoration for every finite mandatory class."""
    source=read(run_dir/'authority-binding.json');authority=mandatory_authorities(Path(source['source_root']),primary_index,compiled_input,requirements);baseline=compare_mandatory_native_files(run_dir,native_paths,binding,supplemental,authority);reference=mandatory_reference_controls(run_dir,primary_index,compiled_input,requirements,Path(supplemental['blueReferenceRun']['path']));rows=list(reference['exercises']);indexed={row['caseId']:row for row in binding['cases']};selected=['T01.opponent-active','T02.own-main','T06.artist-top','T04.empty','T09.fresh-next-turn','T09.bounce-recast-stale'];runs={};guards={}
    for name in selected:
        path=Path(indexed[name]['path']);guards[str(path)]=sha(path);document=read(path);report,_=unwrap_native(document,False,read(Path(authority['authorities']['content']['path'])),PROFILES[1]);runs[name]=report['runs'][0]
    rows.extend(mandatory_native_controls(runs,run_dir,authority));del runs
    observer_file=next(row for row in supplemental['files'] if row['name']=='observer');observer_path=bound_file(observer_file,'actual OB01 controls substrate');guards[str(observer_path)]=sha(observer_path);rows.extend(mandatory_observer_controls(read(observer_path)))
    rows.extend(mandatory_image_controls(binding,authority))
    name='T02.own-main';document=read(Path(indexed[name]['path']))
    rows.extend(mandatory_capture_controls(document,binding,indexed[name],authority))
    rows.extend(mandatory_authority_controls(authority,document,binding,indexed[name]))
    del document
    rows.extend(mandatory_auxiliary_capture_controls(binding,authority))
    original=bound_file(supplemental['predecessorApi']['files'][0]['before'],'actual predecessor API controls substrate');guards[str(original)]=sha(original)
    rows.extend(mandatory_api_controls(original))
    required=mandatory_variant_dispositions(authority['requirements'],rows,True)
    for path,before in guards.items():same(sha(Path(path)),before,'actual native/observer/API control source restored and immutable')
    return dict(schema_version=1,profile=PROFILES[1],status='ACTUAL_FINAL_MANDATORY_CONTROLS_PASSED',baseline=baseline,all_required_exercised=True,required_control_classes=required,exercises=rows,reader_sha256=sha(Path(compare_reader.__file__)),controls_sha256=sha(Path(__file__)),limits='All34 finite reader classes; separate OB01 stays test-only, no all-card/full-engine qualification',copy_scope='Only named mutation branches/ancestor containers; real whole files remain immutable; API mutation streamed to isolated temporary bytes')


def main()->None:
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--reference-run',type=Path);parser.add_argument('--native',type=Path,action='append',default=[]);parser.add_argument('--native-binding',type=Path);parser.add_argument('--supplemental-binding',type=Path);parser.add_argument('--allow-uncaptured',action='store_true');parser.add_argument('--allow-partial-controls',action='store_true');parser.add_argument('--out',type=Path,required=True)
    parser.add_argument('--profile',choices=PROFILES,default=PROFILES[0]);parser.add_argument('--primary-index',type=Path);parser.add_argument('--compiled-input',type=Path);parser.add_argument('--requirements',type=Path);parser.add_argument('--optional-admission',type=Path);args=parser.parse_args()
    if args.profile==compare_reader.OPTIONAL_PROFILE:
        if args.optional_admission is None:parser.error('optional controls require --optional-admission for actual current26 baseline and separately named native/API epochs')
        if any((args.reference_run,args.native,args.native_binding,args.supplemental_binding,args.allow_uncaptured,args.allow_partial_controls,args.primary_index,args.compiled_input,args.requirements)):parser.error('optional controls forbid partial/uncaptured or historical profile options')
        import interaction_reference_optional_controls as optional_controls
        output=optional_controls.final_controls(compare_reader,__import__(__name__),args.optional_admission,args.out.parent/(args.out.stem+'-components'))
        with args.out.open('x') as stream:json.dump(output,stream,indent=2);stream.write('\n')
        print(json.dumps(dict(status=output['status'],named=220,experiments=output['actual_experiments'])));return
    if args.optional_admission is not None:parser.error('optional envelope is outside historical control grammar')
    if args.reference_run is None:parser.error('historical controls require --reference-run')
    explicit=[args.primary_index,args.compiled_input,args.requirements]
    if args.profile==PROFILES[1]:
        if not all(explicit) or args.native_binding is None or args.supplemental_binding is None:parser.error('mandatory-trigger-v1 controls require explicit --primary-index, --compiled-input, --requirements, --native-binding and --supplemental-binding')
        if args.allow_uncaptured or args.allow_partial_controls:parser.error('mandatory-trigger-v1 controls require the full final image and every finite control; partial/uncaptured bypasses are forbidden')
        before={str(path):sha(path) for path in [args.native_binding,args.supplemental_binding]};output=mandatory_final_controls(args.reference_run,args.native,read(args.native_binding),read(args.supplemental_binding),*explicit)
        for path,digest_before in before.items():same(sha(Path(path)),digest_before,'actual control binding bytes unchanged')
        with args.out.open('x') as stream:json.dump(output,stream,indent=2);stream.write('\n')
        print(json.dumps(dict(profile=args.profile,all34=output['all_required_exercised'],actual_exercises=len(output['exercises']))));return
    if any(explicit) or args.supplemental_binding is not None:parser.error('default Blue controls reject mandatory-only authority options')
    baseline=compare_reference(args.reference_run);binding_sha=sha(args.native_binding) if args.native_binding else None;binding=read(args.native_binding) if args.native_binding else None
    if binding is not None:validate_image_binding(binding)
    c=Controls(args.reference_run,args.native,binding,args.allow_uncaptured);run_controls(c)
    same({r['id'] for r in c.rows},{r['id'] for r in c.plan['controls']},'exact frozen61 logical control inventory')
    postreview=run_postreview_controls(c)
    for path,before in c.file_before.items():same(sha(Path(path)),before,'original retained source file unchanged by controls')
    missing=[r for r in c.rows if r['status']=='NOT_EXERCISED'];output=dict(schema='interaction-reader-controls-v1',baseline=baseline,planned_classes=61,exercised_rejected=len(c.rows)-len(missing),not_exercised=len(missing),all61_exercised_and_rejected=not missing,final_image_binding_used=binding is not None,controls=c.rows,reader_sha256=sha(Path(compare_reader.__file__)),controls_sha256=sha(Path(__file__)),isolation='Persistent changed-branch ancestors only; unchanged witness snapshots/calls/definitions shared. Relevant actual baseline/restoration succeeds after every mutation; retained source file SHA verified unchanged.')
    postreview_missing=[r for r in postreview if r['status']=='NOT_EXERCISED'];output.update(postreview_inventory='Seven explicit additional controls: three outcome, four draw quantity/order; frozen61 authority unchanged',postreview_controls=postreview,postreview_exercised_rejected=len(postreview)-len(postreview_missing),all7_postreview_exercised_and_rejected=not postreview_missing)
    if args.native_binding:
        same(sha(args.native_binding),binding_sha,'actual native image index stable during controls');output['native_image_binding_file']=dict(path=str(args.native_binding.resolve()),sha256=binding_sha)
    with args.out.open('x') as stream:json.dump(output,stream,indent=2);stream.write('\n')
    print(json.dumps(dict(exercised=output['exercised_rejected'],not_exercised=len(missing),all61=not missing,postreview_exercised=output['postreview_exercised_rejected'],all7_postreview=not postreview_missing)))
    if (missing or postreview_missing) and not args.allow_partial_controls:raise SystemExit(1)


if __name__=='__main__':main()
