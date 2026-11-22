"""Native-only owner/branch joins over the maintained canonical reader hooks.

No ordinary Registry, source-card, global-game or rules-interpreter credit lives
here. Each real constructor has its own universe; immutable state forks remain
inside that constructor rather than being stitched by equal GameBasis values.
"""
from __future__ import annotations
import math
import re
from pathlib import Path
from typing import Any

CASES=('OP-N03','OP-N04','OP-N05','OP-N06','OP-N07','OP-N08','OP-N10','OP-N11','OP-N15','OP-N16','OP-N18','OP-N19','OP-N20','OP-N21','OP-N22','OP-N23','OP-N24')
ROOT_FIELDS={'helper','initial_journal','initial_state','initial_state_point','label','ordinal','role_context','tag'}
STATE_ROOT_FIELDS={'basis','decision','eliminations','entropy','opening','outcome','turn'}
PROGRAM_FIELDS={'activated','card_types','color_indicator','continuous','creature_subtypes','effect_defined_colors','kind','land_subtypes','mana','names','supertypes','triggered'}


def natural(m:Any,value:Any,label:str)->int:
    # Finite wire-contract.v2 intentionally uses the JSON mathematical integer
    # domain. Canonical native body values keep their existing strict schemas.
    m.require(type(value) in (int,float) and 0<=value<=9007199254740991 and math.isfinite(value) and value==int(value),label)
    return int(value)


def native_journal(m:Any,records:list[dict[str,Any]])->None:
    m.validate_optional_journal(records,additional_occurrence_fields={'CreatureTypeChosen':'chooser entry instruction slot value'})


def effect_digest(m:Any,state:dict[str,Any],universe:Any)->str:
    m.same(set(state),(m._NATIVE_STATE_FIELDS-STATE_ROOT_FIELDS)|{'pending_work','resolving'},'exact complete native EffectWindow projection fields')
    m.native_world_digest(state);m.validate_optional_owned_work(state,None,require_frame=False);m.validate_optional_copy_objects(state);m.optional_role_join(state);universe._programs(state['programs'])
    m.same(state['creature_vocabulary'],universe.vocabulary_id,'actual supplied/native EffectWindow vocabulary')
    for row in state['objects']:universe._remember_object(row['object'])
    m.validate_optional_stack_payloads(state,universe.object_cards,universe.entry_payloads)
    return m.canonical_digest(state)


def saved_replay_join(m:Any,observations:dict[str,Any],calls:list[dict[str,Any]])->dict[str,Any]:
    """Compare original saved inputs and every actual rerun collection by root."""
    replay=observations['replay'];m.require(replay is not None,'actual N16 full saved-input replay required');m.same(set(replay),{'schema','branches'},'exact native saved replay fields');m.same(replay['schema'],'spellwake.native-saved-input-replay.v2','actual native saved-input replay schema2');m.same(len(replay['branches']),3,'actual Take/Decline/unpaid branch denominator')
    buckets=[]
    for call in calls:
        call_protocol(m,call)
        if call['input']['tag'] in ('GameInitialize','GameInitializeOpening','GameInitializeScheduled'):buckets.append([])
        m.require(bool(buckets),'actual saved replay call has constructor');buckets[-1].append(call)
    m.same(len(buckets),len(observations['root_segments']),'actual saved replay constructor buckets')
    for index,branch in enumerate(replay['branches']):
        m.same(set(branch),{'final_state','optional','probes','replayed_journals','replayed_states','root_anchor','saved_inputs','saved_journals','saved_states','start'},'exact complete native replay branch fields');m.same(branch['start'],'RetainedNativeStateAfterActualInitializer','actual replay starts from retained native state');m.same(set(branch['root_anchor']),{'ordinal','initial_state_point'},'actual replay root anchor fields');ordinal=natural(m,branch['root_anchor']['ordinal'],'actual saved replay root ordinal');m.same(ordinal,index,'actual saved branch selects its own constructor root');root=observations['root_segments'][ordinal];m.same(branch['root_anchor']['initial_state_point'],root['initial_state_point'],'actual replay anchored to exact same-constructor source point')
        actual=buckets[ordinal];constructor=actual[0];m.same(constructor['result']['candidate']['state'],root['initial_state'],'actual replay root/full actual constructor state');m.same(constructor['append']['value'],root['initial_journal'],'actual replay root/full constructor initial journal')
        m.require('saved_inputs' in branch,'actual saved replay requires original input collection');n=len(branch['saved_inputs']);m.same(len(branch['saved_journals']),n+1,'actual saved replay includes initial journal and every accepted append');m.same(len(branch['saved_states']),n+1,'actual saved replay includes initial and every accepted full state');m.same(len(branch['replayed_journals']),n,'actual rerun includes every accepted append');m.same(len(branch['replayed_states']),n+1,'actual rerun includes every complete state');m.same(branch['saved_states'][0],root['initial_state'],'actual saved replay uses same constructor initial state');m.same(branch['saved_journals'][0],root['initial_journal'],'actual saved replay retains complete initializer journal')
        m.same([probe['kind'] for probe in branch['probes']],['LegalChangedColor','WrongActor'],'actual saved replay complete two-probe inventory')
        for probe in branch['probes']:
            matches=[call for call in actual if call['input'].get('tag')=='GameInput' and call['input']['value']==probe['input']];m.same(len(matches),1,'actual saved replay probe has unique same-root typed call');call=matches[0];m.same(call['adopted'],False,'actual saved replay changed/denied probe stays outside adoption boundary')
            if probe['kind']=='LegalChangedColor':
                m.same(probe['expected_relation'],'DifferentJournalAndWholeState','actual legal changed-color probe relation');m.same(call['result']['tag'],'Ok','actual changed-color probe is legal native Ok');m.same(call['result']['candidate'],probe['result'],'actual legal changed-color complete returned candidate')
                original=[i for i,value in enumerate(branch['saved_inputs']) if value['basis']==probe['input']['basis'] and value['command'].get('choice',{}).get('tag')=='ManaColor'];m.same(len(original),1,'actual changed-color probe matches original saved color input');at=original[0];saved=branch['saved_inputs'][at];changed=probe['input'];m.require(changed['command']['choice']['color']!=saved['command']['choice']['color'],'actual legal color probe changed an actual color');m.same({**changed,'command':{**changed['command'],'choice':saved['command']['choice']}},saved,'actual changed-color probe changes only color');m.require(probe['result']['state']!=branch['saved_states'][at+1] and probe['result']['append']!=branch['saved_journals'][at+1],'actual changed-color candidate changes complete state and journal')
            else:
                m.same(probe['expected_relation'],'AtomicTypedErr','actual wrong-actor probe relation');m.same(call['result'],probe['error'],'actual denied probe complete typed Result');m.same(call['result']['error_path'],['WrongChooser'],'actual wrong actor denied by native chooser owner');m.same(call['after_point'],None,'actual denied replay probe has no candidate point')
        accepted=[call for call in actual[1:] if call['result']['tag']=='Ok' and call['adopted']]
        m.same(len(accepted),n,'actual saved branch complete adopted input collection')
        for at,(call,value) in enumerate(zip(accepted,branch['saved_inputs'],strict=True)):
            m.same(call['input'],dict(tag='GameInput',value=value),'actual replay retains exact original typed input bytes/value');m.same(call['result']['candidate']['state'],branch['saved_states'][at+1],'actual saved trace complete candidate/checkpoint state');m.same(call['result']['candidate']['append'],branch['saved_journals'][at+1],'actual saved trace complete accepted journal');m.same(call['input']['value']['basis'],branch['saved_states'][at]['basis'],'actual saved trace original input uses exact preceding whole state basis')
        m.same(branch['replayed_journals'],branch['saved_journals'][1:],'actual entire native reexecution journal collection');m.same(branch['replayed_states'],branch['saved_states'],'actual entire native reexecution checkpoint collection');m.same(branch['final_state'],branch['saved_states'][-1],'actual whole final native state equality')
    m.same([branch['optional'] for branch in replay['branches']],['Take','Decline',None],'actual saved replay branch dispositions')
    return dict(branches=3,complete_original_inputs=True,complete_journals=True,complete_states=True,complete_probes=True,same_constructor_anchors=True)


def trigger_source_join(m:Any,state:dict[str,Any],captures:dict[str,Any],events:dict[str,Any])->None:
    """Join complete captured bindings to actual discovery and cause publications."""
    for entry in [*state['stack'],*([state['resolving']] if state['resolving'] is not None and not any(row['entry']==state['resolving']['entry'] for row in state['stack']) else [])]:
        if entry['kind']['tag']!='TriggeredAbility' or entry['body'] is None:continue
        binding=entry['bindings']['trigger'];m.require(binding['instance'] in captures,'actual native trigger payload has discovery source');captured=captures[binding['instance']]
        m.same(entry['controller'],captured['controller'],'actual native captured ability controller retains discovery owner');m.same(binding,{key:captured[key] for key in ('instance','definition','causes','source')},'actual native complete captured cause/source binding');m.same(entry['body'],captured['body'],'actual native captured complete body')
        causes=binding['causes'];m.require(causes['event'] in events,'actual native complete cause set joins published event');event=events[causes['event']];members=causes['members'];m.require(bool(members),'actual native cause set is nonempty');m.same(len({(r['occurrence']['event'],r['occurrence']['index']) for r in members}),len(members),'actual native complete cause members unique')
        for member in members:
            actual=[row for row in event['occurrences'] if row['id']==member['occurrence']];m.same(len(actual),1,'actual native cause member exact occurrence');value=actual[0]['value'];related=member['related'];tag=related['tag']
            if tag=='SpellCast':m.same(value,dict(tag='CastCompleted',cast=related['cast']),'actual native related complete cast publication');m.same(member['related_player'],related['cast']['controller'],'actual native related caster follows published cast controller')
            elif tag=='StatusChange':
                m.same(value['tag'],tag,'actual native tap cause occurrence family');m.same({key:value[key] for key in ('before','after')},{key:related[key] for key in ('before','after')},'actual native full tap cause before/after appearances');m.same(member['related_player'],related['after']['controller'],'actual native tap cause related controller')
            elif tag=='TokenCreated':
                m.same(value['tag'],tag,'actual native token cause occurrence family');m.same((value['after'],value['creator']),(related['after'],related['creator']),'actual native complete token creation cause');m.same(member['related_player'],related['creator'],'actual native token cause related creator')
            else:raise m.EvidenceError('unsupported finite native related cause family '+tag)
        key=causes['key']
        if key['tag']=='Occurrence':m.same([member['occurrence'] for member in members],[key['occurrence']],'actual native complete EachOccurrence denominator')
        elif key['tag']=='Player':m.same({member['related_player'] for member in members},{key['player']},'actual native complete by-player cause partition')


def tap_waiting_join(m:Any,before:dict[str,Any],call:dict[str,Any])->None:
    """Inspect the real tap watcher separately from activation and production."""
    candidate=call['result']['candidate'];after=candidate['state'];prior={row['instance'] for row in before['waiting']};new=[row for row in after['waiting'] if row['instance'] not in prior];discovered=[row['value'] for row in candidate['append'] if row['tag']=='TriggerDiscovered'];m.same(len(new),1,'actual tap watcher has one waiting capture');m.same(discovered,new,'actual tap waiting capture equals complete once-only discovery')
    costs=[row for row in candidate['append'] if row['tag']=='Event' and row['cause']['tag']=='ManaAbilityCost'];m.same(len(costs),1,'actual tap watcher cause has real tap-cost publication');waiting=new[0];m.same(waiting['causes']['event'],costs[0]['event'],'actual BecomesTapped waiting cause is tap cost only');m.same(len(waiting['causes']['members']),1,'actual SelfObject tap watcher one cause');member=waiting['causes']['members'][0];m.same(member['related']['tag'],'StatusChange','actual tap watcher source uses status change, not activation/production predicate');m.same(member['occurrence'],costs[0]['occurrences'][0]['id'],'actual tap watcher exact cost occurrence');m.same(member['related']['after']['object'],call['input']['value']['command']['source'],'actual tap watcher observes actual tapped source')


def zero_count_refresh_join(m:Any,calls:list[dict[str,Any]],point_values:dict[str,Any])->None:
    zero=[call for call in calls if call['result']['tag']=='Ok' and call['input'].get('value',{}).get('command',{}).get('tag')=='ActivateMana' and call['result']['candidate']['mana_production'] is not None and call['result']['candidate']['mana_production']['produced']=={}];m.same(len(zero),1,'actual zero-count native prefix exactly once');prefix=zero[0];before=point_values[prefix['before_point']];after=prefix['result']['candidate']['state'];m.optional_mana_prefix_join(before,prefix);m.same(after['decision']['frame']['revision'],after['effect_revision'],'actual zero-count fresh payment frame exists before root reply dispatch');m.require(after['decision']['id']!=before['decision']['id'],'actual zero-count reissued parent has fresh root decision')
    replies=[call for call in calls if call['result']['tag']=='Ok' and call['owner_method']=='GameState::advance' and call['input'].get('value',{}).get('command',{}).get('tag')=='Reply'];m.same(len(replies),1,'actual zero-count successful current parent reply exactly once');dispatch=point_values[replies[0]['before_point']];m.validate_optional_pending(dispatch);m.same(dispatch['decision'],after['decision'],'actual zero-count refreshed frame is bound before ordinary root reply dispatch')
    direct=[call for call in calls if call['owner_method']=='EffectWindow::resume_rule'];m.same(len(direct),1,'actual zero-count old direct frame probe retained');m.same(direct[0]['input']['operands']['frame'],before['decision']['frame'],'actual old direct frame is retained pre-prefix frame');m.same(direct[0]['result']['error_path'],['WrongResolutionPaymentFrame'],'actual old direct frame rejected after zero-count prefix');m.same(direct[0]['adopted'],False,'actual old direct frame remains unadopted')
    stale=[call for call in calls if call['result']['tag']=='Err' and call['owner_method']=='GameState::advance'];m.same([call['result']['error_path'] for call in stale],[['StaleRevision'],['WrongDecision']],'actual zero-count retains both old-basis and old-decision typed rejections')
    for call in stale:m.same(call['input']['value']['command']['decision'],before['decision']['id'],'actual zero-count rejected old root decision identity');m.same(call['adopted'],False,'actual zero-count old root reply remains unadopted')


def optional_child_join(m:Any,before:dict[str,Any],call:dict[str,Any])->str|None:
    command=call['input'].get('value',{}).get('command',{});decision=before.get('decision')
    if decision is None or decision['frame']['tag']!='OptionalEffect' or command.get('choice')!=dict(tag='OptionalEffect',choice='Take'):return None
    after=call['result']['candidate']['state'];pending=after['decision']
    if pending is None:return None
    cursor=before['resolution_state'];index=int(cursor['instruction'])+1;instruction=before['resolving']['body']['instructions'][index]
    kinds={'CopySpell':'CopyTargets','ChooseCreatureType':'CreatureType','CreateTokens':'TimestampOrder','Damage':'DamageOrder'}
    if instruction['op'] not in kinds:return None
    tag=kinds[instruction['op']];m.same(pending['frame']['tag'],tag,'actual optional primitive preserves its real child kind');m.same(after['resolving'],before['resolving'],'actual optional child retains complete original body and targets');m.same(after['resolution_state'],dict(cursor,instruction=str(index)),'actual optional child retains original cursor entry and locals');m.require(pending['id']!=decision['id'],'actual optional child gets fresh root decision');m.same(pending['basis'],after['basis'],'actual optional child current root basis')
    controls=m.optional_control_join(before,call,journal_validator=lambda journal:native_journal(m,journal));m.same(len(controls),1,'actual optional child consumes one original outcome')
    if tag=='CopyTargets':m.same(pending['frame']['original'],before['resolving']['targets'][0]['target']['entry'],'actual optional copy child retains exact original target');m.same(str(pending['frame']['instruction']),str(index),'actual optional copy child instruction')
    if tag=='CreatureType':m.same(pending['frame']['slot'],instruction['into'],'actual optional type child retains destination slot')
    return tag


def creature_assignment_join(m:Any,before:dict[str,Any],call:dict[str,Any])->None:
    frame=before['decision']['frame'];m.same(frame['tag'],'CreatureType','actual creature-type child frame');choice=call['input']['value']['command']['choice'];m.same(choice['tag'],'CreatureType','actual creature-type child reply');journal=call['result']['candidate']['append'];assignments=[item['value'] for event in journal if event['tag']=='Event' for item in event['occurrences'] if item['value']['tag']=='CreatureTypeChosen'];m.same(len(assignments),1,'actual optional type child assigns exactly once');m.same(assignments[0],dict(tag='CreatureTypeChosen',chooser=frame['chooser'],entry=frame['entry'],instruction=frame['instruction'],slot=frame['slot'],value=choice['value']),'actual type assignment owned entry slot value and chooser');m.require(not any(row['tag']=='OptionalEffectOutcome' for row in journal),'actual child reply does not repeat optional outcome')


def declined_local_join(m:Any,before:dict[str,Any],call:dict[str,Any])->None:
    m.same(call['input']['value']['command']['choice'],dict(tag='OptionalEffect',choice='Decline'),'actual skipped-local branch is Decline');cursor=before['resolution_state'];instruction=before['resolving']['body']['instructions'][int(cursor['instruction'])+1];m.same(instruction['op'],'ChooseCreatureType','actual skipped primitive owns a local assignment');m.require(not any(row['slot']==instruction['into'] for row in cursor['locals']),'actual skipped local was not already assigned');journal=call['result']['candidate']['append'];m.require(not any(item['value']['tag']=='CreatureTypeChosen' for row in journal if row['tag']=='Event' for item in row['occurrences']),'actual optional skip leaves local unassigned without assignment publication');m.require(call['result']['candidate']['state']['decision'] is None or call['result']['candidate']['state']['decision']['frame']['tag']!='CreatureType','actual optional skip creates no creature-type child')


def bounced_optional_join(m:Any,calls:list[dict[str,Any]],points:dict[str,Any])->None:
    denied=[(i,call) for i,call in enumerate(calls) if call['result']['tag']=='Err' and call['input'].get('value',{}).get('command',{}).get('choice')==dict(tag='OptionalEffect',choice='Take')];m.same(len(denied),1,'actual unavailable Take native rejection retained');at,call=denied[0];state=points[call['before_point']];m.same(state['decision']['frame']['options'],['Decline'],'actual unavailable Take omitted from issued domain');m.same(state['pending_work']['work']['options'],['Decline'],'actual unavailable Take omitted from owned domain');target=state['resolving']['targets'][0]['target']['object'];m.require(target not in {row['object']['object'] for row in state['objects']},'actual unavailable Take binds departed original target');prior=next(previous for previous in reversed(calls[:at]) if previous['result']['tag']=='Ok');m.same(state,prior['result']['candidate']['state'],'actual rejected unavailable Take retains latest whole bounced-target state');call_protocol(m,call);m.same(call['result']['error_path'],['Effect','IllegalOptionalEffect'],'actual unavailable Take rejected by native optional owner')


def moved_donor_join(m:Any,call:dict[str,Any])->None:
    m.same(call['owner_method'],'Program::validate','actual moved-donor dependency uses native definition owner');program=call['input']['input']['program'];m.same([row['op'] for row in program['activated'][0]['body']['instructions']],['OptionalEffect','ReturnToOwnersHand','CopyTargetToSource'],'actual merged optional moved-donor dependency input');m.same(call['result']['tag'],'Err','actual merged optional moved-donor Program is rejected');m.same(call['result']['error_path'],['InvalidInstruction'],'actual moved-donor copy dependency typed rejection')


def receiver_join(m:Any,state:dict[str,Any],frame:dict[str,Any],activation_source:dict[str,Any])->None:
    entry=state['resolving'];m.same(entry['kind']['tag'],'ActivatedAbility','actual optional receiver source is captured activation');m.same(entry['kind']['source'],activation_source,'actual optional receiver remains exact captured activation source');objects={row['object']['object']:row['object'] for row in state['objects']};m.require(activation_source['object'] not in objects,'actual captured optional receiver incarnation absent');donor=entry['targets'][0]['target']['object'];m.require(donor in objects and objects[donor]['zone']['kind']=='battlefield','actual independent donor remains present');m.same(state['pending_work']['work']['options'],['Decline'],'actual absent receiver has Decline-only owned domain');m.same(frame['options'],['Decline'],'actual absent receiver has Decline-only issued domain')


def call_protocol(m:Any,call:dict[str,Any])->None:
    m.same(set(call),{'adopted','after_point','append','before_point','call_id','input','owner_method','replay','result'},'exact complete native-only captured call fields')
    m.require(type(call['adopted']) is bool,'actual native-only call requires typed adoption boundary')
    replay=call['replay'];m.same(set(replay),{'source_eq','full_result_eq','full_record_eq','input_storage','source_snapshot_before_invocation'},'exact complete native-only replay evidence')
    m.same(replay['full_result_eq'],True,'actual native-only full Result reexecution');m.same(replay['full_record_eq'],True,'actual native-only complete projected result reexecution');m.same(replay['input_storage'],dict(tag='SavedTypedCall',call_id=call['call_id']),'actual native-only saved typed input identity')
    initializer=call['input']['tag'] in ('GameInitialize','GameInitializeOpening','GameInitializeScheduled')
    m.same(replay['source_eq'],None if initializer else True,'actual native-only retained complete source')
    m.same(replay['source_snapshot_before_invocation'],'InputOnlyForInitializer' if initializer else True,'actual native-only before-invocation source capture')
    result=call['result']
    if result['tag']=='Err':
        m.same(set(result),{'tag','error','error_domain','error_leaf','error_path'},'exact native-only typed error projection')
        m.same(call['adopted'],False,'actual typed Err never adopted');m.same(call['after_point'],None,'actual typed Err has no successor');m.same(call['append'],dict(tag='NoReturnedTransition',reason='NativeError'),'actual typed Err returns no journal')
        domain='DefinitionError' if call['owner_method']=='Program::validate' else 'EffectError' if call['owner_method'].startswith('EffectWindow::') else 'GameError'
        m.same(result['error_domain'],domain,'actual native-only error top owner');m.same(set(result['error_leaf']),{'data','domain','variant'},'actual complete native-only error leaf');m.same(result['error_leaf']['variant'],result['error_path'][-1],'actual typed native-only error leaf/path relation');m.same(result['error_leaf']['domain'],m.native_error_leaf_domain(result['error_path'],domain),'actual typed native-only nested error owner')
    else:m.same(result['tag'],'Ok','closed native-only Result discriminator')


class NativeOptionalIndex:
    CURRENT_VERSION=10
    OUTER_PROFILE='optional-payment-native'
    def __init__(self,m:Any)->None:
        self.m=m;self.observations=None;self.calls=[];self.points=[];self.call_ids=set();self.point_ids=set();self.universes=[];self.root_calls=[];self.current=-1;self.max_calls=1024;self.max_points=8192;self.replay_calls=[];self.raw_calls=[];self.point_values={};self.captures=[];self.events=[]

    def observe_report(self,value:dict[str,Any])->None:
        m=self.m;m.same(set(value),{'case_id','provenance','replay','roles','root_segments','schema','scope','supplied_owners'},'exact native-only observations v2 fields');m.same(value['schema'],'spellwake.optional-payment-native-packets.v2','actual explicit native-only observation schema2');m.same(value['scope'],'NativeOnly','actual explicit native-only scope');m.require(value['case_id'] in CASES,'actual native-only finite case inventory');m.same(value['provenance'],dict(tag='SyntheticNativeOnly',ordinary_credit=False,source_card_credit=False,full_game_credit=False),'actual native-only qualification boundary')
        for ordinal,root in enumerate(value['root_segments']):
            m.same(set(root),ROOT_FIELDS,'exact actual initializer root fields');m.same(root['tag'],'ActualNativeInitializerRoot','actual initializer root tag');m.same(natural(m,root['ordinal'],'native root ordinal'),ordinal,'actual ordered native root ordinal');m.same(set(root['role_context']),{'participants','active'},'actual root role context fields');m.same([natural(m,v,'native root participant') for v in root['role_context']['participants']],root['initial_state']['participants'],'actual root participants own initializer');m.same(natural(m,root['role_context']['active'],'native root active'),root['initial_state']['active'],'actual root active owns initializer');m.require(isinstance(root['initial_state_point'],str) and bool(root['initial_state_point']),'actual root source point identity')
        roles=value['roles'];case=value['case_id']
        if case in ('OP-N11','OP-N21'):m.same(roles,None,'pure native Program-only case has no fabricated roles');m.same(value['root_segments'],[],'pure native Program-only case has no fabricated roots')
        else:
            fields={'active','captured_controller','caster_and_payer'} if case in ('OP-N03','OP-N04') else {'active','captured_controller','creator_and_payer'} if case=='OP-N10' else {'participants','active','source_owner','source_controller'} if case=='OP-N08' else {'participants','active'}
            m.same(set(roles),fields,'exact finite native-only role fields')
            normalized={key:[natural(m,participant,'native participant role') for participant in member] if key=='participants' else natural(m,member,'native scalar role') for key,member in roles.items()}
            if 'participants' in normalized:m.same(len(normalized['participants']),2,'native duel participant count');m.same(len(set(normalized['participants'])),2,'native duel participants distinct')
            else:m.same(len(set(normalized.values())),3,'native active/captured-controller/caster roles genuinely distinct')
            for root in value['root_segments']:m.same(root['initial_state']['active'],normalized['active'],'native role header actual root active');m.same(set(root['initial_state']['participants']),set(normalized['participants']) if 'participants' in normalized else set(normalized.values()),'native role header actual root participants')
        self.observations=value

    def _new_universe(self,call:dict[str,Any])->Any:
        m=self.m;self.current+=1;m.require(self.current<len(self.observations['root_segments']),'actual constructor requires its own declared native root');seats=call['input']['seed']['seats']
        universe=type('NativeOnlyRootUniverse',(m.OptionalWitnessIndex,),{'REGISTERED_SEATS':set(seats),'ALLOW_UNADOPTED_SUCCESS':True,'ALIAS_OBSERVATIONS_REQUIRED':False,'JOURNAL':staticmethod(lambda records:native_journal(m,records)),'CONTROL_JOIN':staticmethod(lambda before,call:m.optional_control_join(before,call,journal_validator=lambda records:native_journal(m,records)))})()
        self.universes.append(universe);self.root_calls.append(call['call_id']);self.captures.append({});self.events.append({});return universe

    def add_call(self,call:dict[str,Any])->None:
        m=self.m;call_protocol(m,call);m.require(call['call_id'] not in self.call_ids,'actual unique native-only call identity');self.call_ids.add(call['call_id']);method=call['owner_method'];initializer=call['input']['tag'] in ('GameInitialize','GameInitializeOpening','GameInitializeScheduled')
        if self.observations['case_id']=='OP-N16':self.replay_calls.append(call)
        self.raw_calls.append(call)
        if initializer:universe=self._new_universe(call)
        else:universe=None if self.current<0 else self.universes[self.current]
        result=call['result'];saved=dict(call_id=call['call_id'],root=self.current,owner_method=method,input=call['input'],adopted=call['adopted'],before_point=call['before_point'],after_point=call['after_point'],result_tag=result['tag'],replay=call['replay'],append=call['append'])
        if method.startswith('GameState::'):
            m.require(universe is not None,'actual native GameState call has constructor root');universe.add_call(call);saved.update(universe.calls[-1])
        elif method=='EffectWindow::resume_rule':
            m.require(universe is not None,'actual direct EffectWindow call has explicit preceding root');m.same(call['input']['tag'],'EffectCall','actual direct EffectWindow typed input');m.same(call['input']['method'],method,'actual direct EffectWindow source method')
            if result['tag']=='Ok':
                m.same(set(result),{'tag','progress'},'actual EffectWindow returns progress without root candidate');progress=result['progress'];m.same(set(progress),{'state','result','append'},'actual complete native RuleProgress fields');native_journal(m,progress['append']);m.same(call['append'],dict(tag='RulesJournal',value=progress['append']),'actual direct native progress/journal relation');saved['state_digest']=effect_digest(m,progress['state'],universe);saved['progress_result']=progress['result'];saved['progress_append']=progress['append']
                outcome=progress['result'];fields={'Complete':{'checks','draws','completion','damage','declarations','assignments','permanent_entry'},'Pending':{'checks','draws','frame','damage','declarations','assignments'}};m.require(outcome['tag'] in fields,'closed actual RuleProgress result');m.same(set(outcome),{'tag'}|fields[outcome['tag']],'complete actual RuleProgress result fields')
                if outcome['tag']=='Pending':m.validate_optional_owned_work(progress['state'],outcome['frame'])
        elif method=='Program::validate':
            m.same(set(call['input']),{'tag','method','input'},'exact pure ProgramValidate input wrapper');m.same(call['input']['tag'],'PureCall','actual pure ProgramValidate input tag');m.same(call['input']['method'],method,'actual pure source method');m.same(set(call['input']['input']),{'program','creature_vocabulary'},'exact complete pure ProgramValidate operands');m.same(set(call['input']['input']['program']),PROGRAM_FIELDS,'actual complete ProgramValidate Program fields')
            if result['tag']=='Ok':m.same(set(result),{'tag'},'actual pure ProgramValidate Ok has no fabricated candidate');m.same(call['append'],dict(tag='NotApplicable',reason='OwnerHasNoRulesAppend'),'actual pure ProgramValidate Ok has no invented journal')
        else:raise m.EvidenceError('unsupported actual native-only source method '+method)
        if result['tag']=='Err':saved['error']=result
        if self.current>=0:
            for record in call['append'].get('value',[]):
                if record['tag']=='Event':self.events[self.current][record['event']]=record
                elif record['tag']=='TriggerDiscovered':self.captures[self.current][record['value']['instance']]=record['value']
        self.calls.append(saved)

    def add_point(self,point:dict[str,Any])->None:
        m=self.m;m.same(set(point),{'damage_parent','native_context','native_vocabulary','observation_pointers','owner','owner_source','phase','point_id','policy_vocabulary','program_source','projection','resolution'},'exact native-only snapshot fields');m.require(point['point_id'] not in self.point_ids,'actual unique native-only point');self.point_ids.add(point['point_id']);m.same(point['projection']['tag'],'Inline','actual native-only complete Inline snapshot');m.same(set(point['projection']),{'tag','value'},'exact actual native-only Inline fields');m.same(len(point['observation_pointers']),1,'actual one pointer for each native-only point')
        value=point['projection']['value'];owner=point['owner'];call=next((call for call in self.calls if point['point_id'] in (call['before_point'],call['after_point'])),None)
        if self.observations['case_id'] in ('OP-N08','OP-N18','OP-N19','OP-N20','OP-N22','OP-N23','OP-N24'):self.point_values[point['point_id']]=value
        if call is not None:root=call['root']
        else:
            path=point['observation_pointers'][0];parts=path.split('/');root=None
            if len(parts)>3 and parts[1:3]==['observations','root_segments']:root=int(parts[3])
            elif len(parts)>4 and parts[1:4]==['observations','replay','branches']:root=natural(m,self.observations['replay']['branches'][int(parts[4])]['root_anchor']['ordinal'],'actual replay root anchor ordinal')
            elif len(parts)>3 and parts[1:3]==['observations','supplied_owners']:root=natural(m,self.observations['supplied_owners'][int(parts[3])]['preceding_root_ordinal'],'actual supplied EffectWindow preceding root ordinal')
            m.require(root is not None,'actual external native observation has explicit root or supplied owner')
        saved=dict(point_id=point['point_id'],root=root,phase=point['phase'],owner=owner,owner_source=point['owner_source'],pointers=point['observation_pointers'],policy_vocabulary=point['policy_vocabulary'])
        if owner in ('GameState','GameSeedInput'):
            universe=self.universes[root];universe.add_point(point);saved.update(universe.points[-1])
            if owner=='GameState':
                trigger_source_join(m,value,self.captures[root],self.events[root])
                m.same(value['active'],natural(m,self.observations['root_segments'][root]['role_context']['active'],'actual native root active'),'actual native root active player remains independent of payer/controller')
                saved['basis']=value['basis'];saved['decision']=value['decision']
                projected={key:member for key,member in value.items() if key not in STATE_ROOT_FIELDS};projected['resources']={key:member for key,member in value['resources'].items() if key!='next_decision'};saved['effect_digest']=m.canonical_digest(projected)
        elif owner=='EffectWindow':
            universe=self.universes[root];saved['state_digest']=effect_digest(m,value,universe);m.same(point['native_context'],dict(tag='NotOwned',reason='OwnerHasNoContext'),'actual EffectWindow has no invented root context');m.same(point['native_vocabulary'],dict(tag='NativeOwned',value=value['creature_vocabulary']),'actual EffectWindow vocabulary owner');m.same(point['program_source'],dict(tag='NativeOwned',value=[dict(card=str(row['card_id']),program=row['definition']) for row in value['programs']]),'actual EffectWindow complete Program owners');m.same(point['resolution'],dict(tag='Applicable',value=value['resolution_state']),'actual EffectWindow cursor owner');m.same(point['damage_parent'],dict(tag='Applicable',value=None if value['pending_damage'] is None else value['pending_damage']['parent']),'actual EffectWindow real damage parent')
        elif owner=='PureValue':
            m.same(point['phase'],'PureInput','actual pure Program input phase');m.same(set(value),PROGRAM_FIELDS,'actual complete pure Program projection');m.same(value,call['input']['input']['program'],'actual pure source Program/call relation');m.same(point['native_context'],dict(tag='NotOwned',reason='OwnerHasNoContext'),'actual pure input has no root context');m.same(point['native_vocabulary'],dict(tag='NotOwned',reason='OwnerHasNoVocabulary'),'actual pure input has no invented owned vocabulary');m.same(point['program_source']['value'],value,'actual complete supplied pure Program');saved['state_digest']=m.canonical_digest(value)
        else:raise m.EvidenceError('unsupported actual native-only snapshot owner '+owner)
        self.points.append(saved)

    def finish(self,outer:dict[str,Any],witness:dict[str,Any],manifest:dict[str,Any],expected_case:str|None=None)->tuple[dict[str,Any],dict[str,Any]]:
        m=self.m;o=self.observations;m.same(set(outer),{'observations','profile','schema_version','type_context_witness'},'exact native-only envelope');m.same(outer['profile'],'optional-payment-native','explicit native-only profile');m.same(outer['schema_version'],10,'actual native-only current10');m.same(witness['schema'],'spellwake.current-type-context-witness.v8','actual native-only witness8');m.same(witness['registry'],dict(tag='NotApplicable',reason='NoRegistryConstruction'),'actual native-only NoRegistry boundary');m.same(o['case_id'],expected_case,'actual selected native-only case');m.same(len(self.root_calls),len(o['root_segments']),'actual constructor/root one-to-one inventory')
        m.same(witness['vocabulary']['value'],manifest['vocabulary'],'actual complete native-only published vocabulary');m.same({row['path']:row for row in witness['source_inputs']},manifest['capture_source_rows'],'actual native-only source/image epoch closure')
        points={p['point_id']:p for p in self.points};calls={c['call_id']:c for c in self.calls};coverage=witness['coverage'];m.same(coverage['complete'],True,'actual native-only complete witness coverage');m.same(coverage['uncovered'],[],'actual native-only no uncovered observations');m.same(coverage['required_calls'],list(calls),'actual ordered native-only call inventory');m.same(coverage['required_points'],list(points),'actual ordered native-only point inventory')
        reachable=[set() for _ in self.universes];effect_reachable=[set() for _ in self.universes];native_paths=[];expected_joins={};external=set();root_constructors=[]
        for supplied in o['supplied_owners']:
            m.same(supplied['root_continuity'],'NotClaimed','actual supplied EffectWindow has no invented root continuity');root=natural(m,supplied['preceding_root_ordinal'],'actual supplied root ordinal');m.require(root<len(reachable),'actual supplied owner references preceding root');m.same(points[supplied['point']]['root'],root,'actual supplied owner point/root relation');effect_reachable[root].add(points[supplied['point']]['state_digest'])
        for call in self.calls:
            before=points[call['before_point']];m.same(before['root'],call['root'],'actual same-constructor source call/point root');m.same(before['owner_source'],call['owner_method'],'actual native source before-point method')
            if call['input']['tag'] in ('GameInitialize','GameInitializeOpening','GameInitializeScheduled'):
                root=call['root'];root_constructors.append(call);m.same(before['owner'],'GameSeedInput','actual native constructor supplied owner');m.same(before['phase'],'InitializeInput','actual native constructor source phase');expected=dict(context=call['input']['context'],seed=call['input']['seed']);m.same(before['state_digest'],m.canonical_digest(expected),'actual complete native constructor input/point relation')
            elif call['owner_method']=='Program::validate':m.same(before['owner'],'PureValue','actual ProgramValidate pure owner')
            else:
                m.same(before['phase'],'BeforeCall','actual native before invocation phase')
                if before['owner']=='GameState':
                    m.require(before['state_digest'] in reachable[call['root']],'actual native branch before-state belongs to same constructor graph')
                    if call['result_tag']=='Ok':m.same(call['input']['value']['basis'],before['basis'],'actual successful saved native input/full before-basis relation')
                else:
                    m.same(before['owner'],'EffectWindow','actual direct native method owner');m.require(before['state_digest'] in effect_reachable[call['root']],'actual direct EffectWindow source is returned same-root execution or explicit supplied owner')
            if call['result_tag']=='Ok' and call['after_point'] is not None:
                after=points[call['after_point']];m.same(after['root'],call['root'],'actual same constructor successor point');m.same(after['state_digest'],call['state_digest'],'actual whole native returned state/after-point relation');m.same(after['phase'],'AfterCall','actual native returned-state phase');m.same(after['owner_source'],call['owner_method'],'actual returned-state source method')
                if after['owner']=='GameState':reachable[call['root']].add(after['state_digest']);effect_reachable[call['root']].add(after['effect_digest'])
                else:effect_reachable[call['root']].add(after['state_digest'])
        for ordinal,root in enumerate(o['root_segments']):
            point=points[root['initial_state_point']];constructor=root_constructors[ordinal];after=points[constructor['after_point']];position=self.points.index(point);m.require(position>0 and self.points[position-1]['point_id']==after['point_id'],'actual root marker immediately follows its real constructor successor point');m.same(point['owner_source'],'NativeOnly.ActualInitializerRoot','actual root marker source owner');m.same(point['state_digest'],after['state_digest'],'actual root marker/same-constructor complete state');m.same(root['initial_journal'],constructor['append']['value'],'actual root initial journal/same constructor result')
        for index,point in enumerate(self.points):
            m.same(point['policy_vocabulary'],manifest['vocabulary']['id'],'actual native-only policy/native vocabulary identity')
            path=point['pointers'][0];m.require(path not in expected_joins,'actual native-only unique observation pointer');expected_joins[path]=[point['point_id']]
            own='/type_context_witness/snapshots/'+str(index)+'/projection/value'
            if path==own:native_paths.append(path)
            else:
                m.require(path.startswith('/observations/'),'actual native-only source-bound external pointer');value=m.pointer(outer,path);m.same(m.canonical_digest(value),point['state_digest'],'actual whole native observation/point relation');external.add(path)
        m.same(coverage['record_pointers'],native_paths,'actual complete native-only record-pointer inventory');m.same({r['pointer']:r['points'] for r in coverage['observation_to_points']},expected_joins,'actual complete native-only reciprocal pointer inventory')
        required_external={'/observations/root_segments/'+str(i)+'/initial_state' for i in range(len(o['root_segments']))}|{'/observations/supplied_owners/'+str(i)+'/state' for i in range(len(o['supplied_owners']))}
        if o['replay'] is not None:
            for i,branch in enumerate(o['replay']['branches']):
                required_external.add('/observations/replay/branches/'+str(i)+'/final_state')
                for field in ('saved_states','replayed_states'):required_external.update('/observations/replay/branches/'+str(i)+'/'+field+'/'+str(j) for j in range(len(branch[field])))
        m.same(external,required_external,'actual independently derived complete native-only observation denominator')
        replay=None
        if o['case_id']=='OP-N16':replay=saved_replay_join(m,o,self.replay_calls)
        if o['case_id']=='OP-N23':zero_count_refresh_join(m,self.raw_calls,self.point_values)
        if o['case_id']=='OP-N08':
            for call in self.raw_calls:
                if call['result']['tag']=='Ok' and call['owner_method']=='GameState::advance' and any(row['tag']=='TriggerDiscovered' for row in call['append'].get('value',[])):tap_waiting_join(m,self.point_values[call['before_point']],call)
        if o['case_id'] in ('OP-N18','OP-N19','OP-N22'):
            for call in self.raw_calls:
                if call['result']['tag']!='Ok' or call['owner_method']!='GameState::advance':continue
                before=self.point_values[call['before_point']];optional_child_join(m,before,call);choice=call['input']['value']['command'].get('choice',{})
                if choice.get('tag')=='CreatureType':creature_assignment_join(m,before,call)
                if o['case_id']=='OP-N19' and choice==dict(tag='OptionalEffect',choice='Decline'):declined_local_join(m,before,call)
        if o['case_id']=='OP-N20':bounced_optional_join(m,self.raw_calls,self.point_values)
        if o['case_id']=='OP-N21':
            for call in self.raw_calls:moved_donor_join(m,call)
        if o['case_id']=='OP-N24':
            roots=[]
            for call in self.raw_calls:
                if call['input']['tag'].startswith('GameInitialize'):roots.append([])
                roots[-1].append(call)
            for ordinal,bucket in enumerate(roots):
                activations=[item['value'] for call in bucket for record in call['append'].get('value',[]) if record['tag']=='Event' for item in record['occurrences'] if item['value']['tag']=='ActivatedAbility'];m.same(len(activations),1,'actual receiver native root has one captured activation source');source=activations[0]['source']
                for call in bucket:
                    if call['owner_method']=='GameState::advance':
                        state=self.point_values[call['before_point']]
                        if state['decision'] is not None and state['decision']['frame']['tag']=='OptionalEffect':receiver_join(m,state,state['decision']['frame'],source)
                    elif call['owner_method']=='EffectWindow::resume_rule':receiver_join(m,self.point_values[call['before_point']],call['input']['operands']['frame'],source)
        return o,dict(status='ACTUAL_NATIVE_ONLY_OWNER_GRAPH_STRUCTURAL_PASS',roots=len(self.universes),calls=len(calls),points=len(points),same_constructor_graph=True,global_graph_stitching=False,saved_input_replay=replay,synthetic_native_only=True,registry_credit=False,ordinary_credit=False,source_card_credit=False,full_game_credit=False,qualification_credit=0)


def read_capture(m:Any,path:Path,manifest:dict[str,Any],case:str)->Any:
    # One maintained bounded JsonStream reader and call/point framework. The
    # native-only index supplies only owner/segment-specific admission hooks.
    index=NativeOptionalIndex(m)
    return m.read_mandatory_capture(path,manifest,case,index_factory=lambda:index)


def load_actual_authority(m:Any,producer:Path,root:Path)->dict[str,Any]:
    """Bind the real test-image/index/source-declaration packet epoch."""
    process=m.read(producer/'receipt.json');m.same(process['exit_code'],0,'actual native-only exporter process success');m.same(process['source_stable'],True,'actual native-only exporter source stable');index_path=m.bound_file(process['native_packet_index'],'actual native-only packet index');index=m.read(index_path);m.same(set(index),{'artifacts','profile','schema','scope'},'closed actual native-only packet index');m.same((index['profile'],index['schema'],index['scope']),('optional-payment-native','spellwake.optional-payment-native-packet-index.v2','NativeOnly'),'exact actual native-only index namespace');m.same(len(index['artifacts']),17,'actual native-only index17 rows');m.unique(index['artifacts'],'path','actual unique native-only artifact path');m.same(index['artifacts'],process['native_packet_artifacts'],'actual native-only as-produced index/artifact receipt relation');artifacts={Path(r['path']).stem:r for r in index['artifacts']};m.same(set(artifacts),set(CASES),'actual complete native-only export17 inventory')
    command=[part for part in process['command'] if not part.startswith('--message-format=')];selector='effects::resolution_choices::tests::native_mechanisms::native_packets::export_optional_payment_native_packets';m.same(command,['cargo','test','--locked','-p','spellwake-engine','--lib','--','--ignored','--exact',selector,'--nocapture'],'actual exact ignored native packet exporter command');log=m.bound_file(process['stdout'],'actual native packet exporter stdout');m.bound_file(process['stderr'],'actual native packet exporter stderr');text=log.read_text();m.same(re.findall(r'^test ([^\s]+) \.\.\. ok\s*$',text,re.M),[selector],'actual named native exporter executed once');m.same(re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;',text),[('1','0','0')],'actual exporter one pass zero failed zero ignored')
    for image in process['actual_executables']:m.bound_file(image['retained'],'actual native-only test image')
    before=m.read(producer/'source-before.json');m.same(m.read(producer/'source-after.json'),before,'actual native-only complete retained source pre/post');sources=m.unique(before,'path','actual native-only retained source');retained=producer/'source';expected=[]
    for path,pattern in [(retained/'crates/engine/src/effects/resolution_choices/native_packets.rs',r'let sources = \[(.*?)\]\s*\.map'),(retained/'crates/reference_tools/src/current_capture.rs',r'for path in \[(.*?)\]\s*\{\s*let path = repo.join')]:
        blocks=re.findall(pattern,path.read_text(),re.S);m.same(len(blocks),1,'actual source-declared native packet closure');members=re.findall(r'"([^"\n]+)"',blocks[0]);m.require(bool(members) and not re.sub(r'"[^"\n]+"|[\s,]','',blocks[0]),'actual native-only finite source declaration');expected.extend(str(root/member) for member in members)
    source_rows={path:sources[path] for path in set(expected)}
    for path,value in source_rows.items():
        copy=retained/Path(path).relative_to(root);m.same((m.sha(copy),copy.stat().st_size),(value['sha256'],value['bytes']),'actual retained source-declared native-only bytes')
    vocabulary=Path(process['explicit_input_paths']['SPELLWAKE_CREATURE_VOCABULARY_INPUT']);publication=Path(process['explicit_input_paths']['SPELLWAKE_CREATURE_VOCABULARY_PUBLICATION']);manifest=dict(vocabulary=m.read(vocabulary),capture_source_rows=source_rows)
    return dict(process=process,index_path=index_path,artifacts=artifacts,manifest=manifest,vocabulary=vocabulary,publication=publication)
