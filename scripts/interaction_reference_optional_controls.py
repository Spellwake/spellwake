"""One owner for actual optional220 component mutation selectors.

Raw parsing, native owner relations and mutation execution stay in their existing
maintained owners. Resource paths are explicit so final controls cannot silently
borrow historical native/Forge baselines.
"""
from __future__ import annotations
import hashlib
import json
from pathlib import Path
from typing import Any


def load(path:Path)->Any:return json.loads(path.read_text())
def row(path:Path)->dict[str,Any]:
    data=path.read_bytes();return dict(path=str(path.resolve()),bytes=len(data),sha256=hashlib.sha256(data).hexdigest())
receipt=row

def ptr(path:list[Any])->str:return '/'+('/'.join(str(k).replace('~','~0').replace('/','~1') for k in path))
pointer=ptr


def ordinary(directory:Path,resources:dict[str,Any],compare:Any,controls:Any)->dict[str,Any]:
    ROOT=Path(resources['root']);BASE=ROOT/'work/engine-milestones/optional-payment-v1';PLAN=ROOT/'.quzzar-system-design/architecture-proposed/work-planning-20261103/planning/optional-payment-v1/qualification/preparation/planned-reader-controls.json';attempt=directory.name
    obligations={row['id']:row for row in load(PLAN)['controls']}
    rows=[];authority_rows=[];inputs={};documents={};contexts={}

    def read(path):
        path=path.resolve();inputs[str(path)]=receipt(path)
        if path not in documents:documents[path]=load(path)
        return documents[path]

    def case(name):
        if name not in contexts:
            packet=Path(resources['native_files'][name])
            document=read(packet);compare.optional_report_ingress(document,name)
            points=document['type_context_witness']['snapshots'];by_id={p['point_id']:(i,p) for i,p in enumerate(points)}
            contexts[name]=dict(path=packet,document=document,points=by_id,calls=document['type_context_witness']['calls'])
        return contexts[name]

    def select(name,tag):
        context=case(name)
        for call_i,call in enumerate(context['calls']):
            if call['result']['tag']!='Ok' or call['owner_method']!='GameState::advance':continue
            point_i,point=context['points'][call['before_point']];state=point['projection']['value']
            if state['decision'] is not None and state['decision']['frame']['tag']==tag and call['input']['value']['command']['tag']=='Reply':
                return dict(before=state,call=call),context,point_i,call_i
        raise compare.EvidenceError('actual successful control reply missing '+name+'/'+tag)

    def state_check(value):
        state=value['before'];signature=compare.optional_native_state_signature(state);compare.optional_role_join(state)
        objects={row['object']['object']:row['object']['physical_card'] for row in state['objects']}
        # Capture the original body before this mutation's cut; current copies
        # are joined independently to already owned originals by the full reader.
        if not any(row['kind']['tag']=='SpellCopy' for row in state['stack']):compare.validate_optional_stack_payloads(state,objects,{})
        return signature

    def pair_check(value):
        state_check(value)
        return compare.optional_control_join(value['before'],value['call'])

    def exercise(number,name,substrate,patches,reader,guards,anchors):
        identity=f'OP-C{number:03}'
        guards=[guard for guard in guards if guard!='malformed evidence']
        omitted=[path[-1] for path,value in patches if value is controls.DELETE and path]
        guards.extend(reader.__name__+" malformed evidence: "+repr(key) for key in omitted)
        anchors=[dict(file=receipt(path),json_pointer=ptr(path_keys),**extra) for path,path_keys,extra in anchors]
        row=controls.exercise_optional_atomic(obligations[identity],name,substrate,patches,reader,tuple(guards),anchors)
        rows.append(row)
        (directory/'rows.json').write_text(json.dumps(rows,indent=2)+'\n')

    def pair_anchor(context,point_i,call_i,path):
        root=['type_context_witness','snapshots',point_i,'projection','value'] if path[0]=='before' else ['type_context_witness','calls',call_i]
        return context['path'],root+path[1:],dict(component='canonical state' if path[0]=='before' else 'actual saved native call')

    # SOURCE_AND_EPOCH: independently retained authorities, no game-derived Program.
    selected_path=Path(resources['primary']['selected19']);selected=read(selected_path)
    metadata_path=Path(resources['primary']['metadata']);metadata=read(metadata_path)
    snapshot=Path(resources['primary']['snapshot']);inputs[str(snapshot)]=receipt(snapshot)
    reference_path=Path(resources['primary']['forge_selected19']);reference=read(reference_path)
    compiled_path=Path(resources['primary']['compiled']);compiled=read(compiled_path)
    table_path=Path(resources['primary']['primary_program_table']);table=read(table_path)
    compare.same(receipt(table_path)['sha256'],'1d0ce86b0b9d59af4b9e6352864f79da878f622c4a1da27f5a7b7e8b0c5b3142','actual independently accepted source Program table bytes')
    def source_check(value):
        compare.optional_raw_source_join(compiled,value,reference,metadata=metadata,source_receipt=receipt(snapshot),primary=table)
        return value
    source_fields=[(['source','sha256'],'0'*64),(['metadata','compressed_size'],metadata['compressed_size']+1),(['metadata','updated_at'],'2026-10-30T21:01:58.033+00:00'),
        (['rows',0,'printing','id'],selected['rows'][1]['printing']['id']),(['rows',0,'printing','set'],'roe'),(['rows',0,'printing','collector_number'],'210'),(['rows',0,'printing','lang'],'fr'),
        (['rows',0,'oracle_id'],selected['rows'][1]['oracle_id']),(['rows',0,'materialized_faces_for_review_not_native_signature',0,'name'],'Foreign Name'),(['rows',0,'printing','layout'],'transform'),
        (['rows',0,'materialized_faces_for_review_not_native_signature',0,'type_line'],'Creature — Human'),(['rows',0,'materialized_faces_for_review_not_native_signature',0,'mana_cost'],'{2}{B}'),
        (['rows',0,'materialized_faces_for_review_not_native_signature',0,'power'],'1'),(['rows',0,'materialized_faces_for_review_not_native_signature',0,'toughness'],'2'),
        (['rows',0,'materialized_faces_for_review_not_native_signature',0,'oracle_text'],'You gain 1 life.'),(['rows',0,'full_oracle_paragraphs',0,'text'],'You gain 1 life.'),(['rows',0,'complete_saved_legalities','legacy'],'not_legal')]
    for index,(path,foreign) in enumerate(source_fields):
        for offset,value in enumerate((foreign,controls.DELETE)):
            exercise(1+index*2+offset,'OP-R11',selected,[(path,value)],source_check,['actual exact compressed source receipt','actual complete pinned source metadata','optional selected raw source19','actual complete optional compiled/raw identity join','actual exact optional printing join','actual full Forge/saved printing identity','actual complete saved source faces','actual complete saved clauses and paragraph order','actual dated saved Legacy status','malformed evidence'],[(selected_path,path,{})])
    native=case('OP-R11');coverage=native['document']['type_context_witness']['registry']['coverage']
    for start,path,foreign in [(35,['cards',0,'binding','semanticSha256'],'0'*64),(37,['cards',0,'binding','program'],'foreign-program')]:
        for offset,value in enumerate((foreign,controls.DELETE)):
            exercise(start+offset,'OP-R11',coverage,[(path,value)],lambda value:compare.validate_optional_registry(value,dict(native_registry_coverage=coverage,compiled=compiled),compare.OPTIONAL_PROFILE),['actual complete retained C03 Registry8 coverage'],[(native['path'],['type_context_witness','registry','coverage']+path,{})])
    for number,value in [(39,9),(40,controls.DELETE)]:
        exercise(number,'OP-R11',native['document'],[(['schema_version'],value)],compare.optional_report_ingress,['explicit optional current10','exact optional current10 envelope'],[(native['path'],['schema_version'],{})])

    # Issued frame and actual supplied input relationships.
    for name,tag,start,fields in [('OP-R02','OptionalEffect',41,['revision','entry','instruction','chooser','options.Take','options.Decline']),('OP-R11','ResolutionPayment',53,['revision','entry','instruction','payer','cost'])]:
        pair,context,point_i,call_i=select(name,tag);frame=pair['before']['decision']['frame']
        for index,field in enumerate(fields):
            path=['before','decision','frame',field];original=frame.get(field)
            if field.startswith('options.'):
                position=frame['options'].index(field.split('.')[1]);path=['before','decision','frame','options',position];foreign='Foreign'
            elif field in ('revision','instruction'):foreign=str(int(original)+1)
            elif field=='entry':foreign='StackEntryId(999999)'
            elif field in ('chooser','payer'):foreign=3 if original==41 else 41
            else:foreign=dict(original,generic=str(int(original['generic'])+1))
            for offset,value in enumerate((foreign,controls.DELETE)):
                exercise(start+index*2+offset,name,pair,[(path,value)],state_check,['actual fresh frame/executor revision','actual issued frame/cursor origin','actual issued frame/owned','malformed evidence'],[pair_anchor(context,point_i,call_i,path)])
    pair,context,point_i,call_i=select('OP-R11','ResolutionPayment')
    input_fields=[(['command','decision'],'999999'),(['basis','game'],'999999'),(['command','choice','tag'],'OptionalEffect'),(['actor'],3),(['command','choice','reply','tag'],'Decline'),(['command','choice','reply','payment','spend',0,'amount'],'2')]
    for index,(relative,foreign) in enumerate(input_fields):
        path=['call','input','value',*relative]
        # The normalized allocation is an owned list, so omission removes that
        # required allocation rather than inventing a zero-valued replacement.
        if index==5:path=['call','input','value','command','choice','reply','payment','spend']
        for offset,value in enumerate((([dict(type='Blue',amount='2')] if index==5 else foreign),controls.DELETE)):
            exercise(63+index*2+offset,'OP-R11',pair,[(path,value)],pair_check,['actual control reply/current DecisionId','actual control reply/current complete GameBasis','actual payment reply kind','actual control reply/root chooser','actual Paid reply relation','actual normalized Paid record/saved Pay allocation','malformed evidence'],[pair_anchor(context,point_i,call_i,path)])

    pair,context,point_i,call_i=select('OP-R14','ManaColor')
    work=['before','pending_work','parent','work'];cursor=pair['before']['resolution_state'];resolving=pair['before']['resolving']
    pending_fields=[(work+['cursor','instruction'],str(int(cursor['instruction'])+1)),(['before','resolving','targets',0,'target','entry'],'StackEntryId(999999)'),
        (['before','resolving','kind','object'],'ObjectId(999999)'),(work+['payer'],3),(work+['cost','generic'],'4'),(work+['paid_destination'],'999999'),(work+['declined_destination'],'999999'),
        (['before','pending_work','parent','tag'],'Priority'),(work+['cursor','entry'],'StackEntryId(999999)'),(work+['cost','generic'],'5'),(['before','decision','frame','offered'],['White'])]
    for index,(path,foreign) in enumerate(pending_fields):
        for offset,value in enumerate((foreign,controls.DELETE)):
            exercise(75+index*2+offset,'OP-R14',pair,[(path,value)],state_check,['complete owned cursor fields','complete owned optional/payment continuation fields','actual owned cursor/current resolution relation','actual queued/resolving payload is the same owned entry','actual exact bound current target-spell controller','actual owned payment quote/successor/source','exact priority color parent','closed color parent','actual color decision/owned frame relation','actual complete current target stack inventory','actual target spell still present','malformed evidence'],[pair_anchor(context,point_i,call_i,path)])
    def color_check(value):
        state_check(value)
        return compare.optional_color_reply_join(value['before'],value['call'])
    for number,value in [(97,pair['before']['decision']['id']),(98,controls.DELETE)]:
        path=['call','result','candidate','state','decision','id']
        exercise(number,'OP-R14',pair,[(path,value)],color_check,['actual restored parent has fresh decision identity','malformed evidence'],[pair_anchor(context,point_i,call_i,path)])
    color_context=case('OP-R14');owners={};restorations=[];color_calls=color_context['calls']
    for call in color_calls:
        if call['result']['tag']!='Ok':continue
        point=color_context['points'][call['before_point']][1]
        if point['owner']!='GameState':continue
        before=point['projection']['value'];pending=before['pending_work']
        if pending is not None and pending['tag']=='ManaColor' and pending['parent']['tag']=='Payment':
            owner=before['basis']['game']+':'+before['decision']['id'];owners[owner]=True
    def color_inventory(value):
        returned=[]
        for call in value:
            if call['result']['tag']!='Ok':continue
            point=color_context['points'][call['before_point']][1]
            if point['owner']=='GameState':
                restored=compare.optional_color_reply_join(point['projection']['value'],call)
                if restored is not None:returned.append(restored)
        compare.optional_color_restoration_inventory(set(owners),returned)
        return returned
    for number,mutated in [(99,[*color_calls,color_calls[call_i]]),(100,color_calls[:call_i]+color_calls[call_i+1:])]:
        exercise(number,'OP-R14',color_calls,[([],mutated)],color_inventory,['actual exactly once color-parent restoration cut','actual complete color-parent restoration owner inventory'],[(color_context['path'],['type_context_witness','calls',call_i],dict(relationship='accepted return of this actual issued payment color child'))])
    restored=pair['call']['result']['candidate']['state']
    for number,value in [(211,str(int(restored['decision']['frame']['revision'])-1)),(212,controls.DELETE)]:
        path=['call','result','candidate','state','decision','frame','revision']
        exercise(number,'OP-R14',pair,[(path,value)],color_check,['actual fresh frame/executor revision','malformed evidence'],[pair_anchor(context,point_i,call_i,path)])

    # Actual emitted outcomes: mutate the real origin, actor, outcome and edge.
    for name,tag,outcome,start in [('OP-R02','OptionalEffect','Take',101),('OP-R03','OptionalEffect','Decline',105),('OP-R11','ResolutionPayment','Paid',109),('OP-R18','ResolutionPayment','Declined',117)]:
        pair,context,point_i,call_i=select(name,tag)
        journal=pair['call']['result']['candidate']['append'];j=next(i for i,row in enumerate(journal) if row['tag']==('OptionalEffectOutcome' if tag=='OptionalEffect' else 'ResolutionPaymentOutcome'))
        actor='chooser' if tag=='OptionalEffect' else 'payer';base=['call','result','candidate','append',j]
        fields=[(['entry'],'StackEntryId(999999)'),([actor],3 if journal[j][actor]==41 else 41),(['outcome'] if tag=='OptionalEffect' else ['outcome','tag'],('Decline' if outcome=='Take' else 'Take') if tag=='OptionalEffect' else ('Declined' if outcome=='Paid' else 'Paid')),(['next'],'999999')]
        for offset,(relative,value) in enumerate(fields):
            path=base+relative
            exercise(start+offset,name,pair,[(path,value)],pair_check,['actual control/issued offer origin','actual optional control actor','actual optional reply/outcome','actual optional control structural successor','actual payment control actor','actual Paid record/offer','exact payment outcome payload','actual Declined owned successor','actual Paid owned successor','malformed evidence'],[pair_anchor(context,point_i,call_i,path)])

    pair,context,point_i,call_i=select('OP-R11','ResolutionPayment');journal=pair['call']['result']['candidate']['append'];j=next(i for i,row in enumerate(journal) if row['tag']=='ResolutionPaymentOutcome');debit_i=next(i for i,row in enumerate(journal) if row['tag']=='Event' and row['cause']['tag']=='ManaDebited')
    payment_base=['call','result','candidate','append',j,'outcome','payment'];debit_base=['call','result','candidate','append',debit_i,'occurrences']
    changes={121:(payment_base+['cost'],controls.DELETE),122:(payment_base+['cost','generic'],'4'),123:(payment_base+['spend'],controls.DELETE),124:(payment_base+['spend',0,'amount'],'2'),125:(debit_base,[]),126:(debit_base+[0,'value','after'],'1')}
    for number,(path,value) in changes.items():
        exercise(number,'OP-R11',pair,[(path,value)],pair_check,['actual Paid record/offer','same existing normalized native payment record','actual normalized Paid record/saved Pay allocation','actual exact ordered debit/normalized spend relation','actual native debit pool delta','malformed evidence'],[pair_anchor(context,point_i,call_i,path)])
    decline,decline_context,decline_point,decline_call=select('OP-R12','ResolutionPayment')
    actual_debit=journal[debit_i]
    for number,path,value in [(127,['call','result','candidate','state','mana'],controls.DELETE),(128,['call','result','candidate','append'],[actual_debit,*decline['call']['result']['candidate']['append']])]:
        exercise(number,'OP-R12',decline,[(path,value)],pair_check,['actual Declined requires complete returned mana pools','Declined emits no debit'],[pair_anchor(decline_context,decline_point,decline_call,path)])
    for number,path,value in [(131,['call','result','candidate','append',j,'tag'],controls.DELETE),(132,['call','result','candidate','append',debit_i,'occurrences',0,'value','tag'],'ResolutionPaymentOutcome')]:
        exercise(number,'OP-R11',pair,[(path,value)],pair_check,['actual control/journal record requires explicit kind outside triggerable occurrences','source19 forbids canonical rules occurrence ResolutionPaymentOutcome'],[pair_anchor(context,point_i,call_i,path)])
    forge_dir=Path(resources['reference_runs']['OP-R16']);raw_path=forge_dir/'evidence/OP-R16.json';forge_raw=read(raw_path);forge_inputs=read(forge_dir/'inputs.json');forge_case=next(row for row in forge_inputs['cases'] if row['case_id']=='OP-R16')
    run=case('OP-R16')['document']['observations']['data']['runs'][0];native_index=compare.optional_native_common_index(run,forge_case)
    declined_raw=read(forge_dir/'evidence/OP-R12.json')
    for number,value in [(133,controls.DELETE),(134,declined_raw['resolution_payments'])]:
        exercise(number,'OP-R16',forge_raw,[(['resolution_payments'],value)],lambda value:compare.optional_no_offer_join(native_index,value),['actual no-offer proof requires complete payment receipt collection','actual all-targets-illegal path has no invented payment callback/outcome'],[(raw_path,['resolution_payments'],dict(native_settlement_file=receipt(case('OP-R16')['path'])))])

    # Existing ManaPaymentRecord owner remains the sole normalized payment fact.
    pair,context,point_i,call_i=select('OP-R14','ResolutionPayment');journal=pair['call']['result']['candidate']['append'];j=next(i for i,row in enumerate(journal) if row['tag']=='ResolutionPaymentOutcome')
    payment=journal[j]['outcome']['payment'];base=['call','result','candidate','append',j,'outcome','payment']
    changes={135:(base+['entry'],'StackEntryId(999999)'),136:(base+['payer'],3),137:(base+['cost','generic'],'4'),140:(base+['spend',0,'type'],'Green'),141:(base+['spend',0,'amount'],'2'),142:(base+['spend'],list(reversed(payment['spend']))),143:(['call','result','candidate','mana_payment'],payment)}
    compare.require(len(payment['spend'])>1,'C142 real eligible multirow R14 allocation')
    for number,(path,value) in changes.items():
        exercise(number,'OP-R14',pair,[(path,value)],pair_check,['actual Paid record/offer','actual normalized Paid record/saved Pay allocation','resolution payment never fabricates cast/activation fact'],[pair_anchor(context,point_i,call_i,path)])

    pair,context,point_i,call_i=select('OP-R20','ResolutionPayment');state=pair['before'];target=state['resolving']['targets'][0]['target']['entry'];target_i=next(i for i,row in enumerate(state['stack']) if row['entry']==target);copy_i=next(i for i,row in enumerate(state['copies']) if row['entry']==target)
    compare.same(state['stack'][target_i]['kind']['tag'],'SpellCopy','C150/C151 actual current target-copy prerequisite')
    foreign=3 if state['pending_work']['work']['payer']==41 else 41
    patches=[(['before','stack',target_i,'controller'],foreign),(['before','copies',copy_i,'controller'],foreign)]
    exercise(150,'OP-R20',pair,patches,pair_check,['actual exact bound current target-spell controller'],[pair_anchor(context,point_i,call_i,path) for path,value in patches])
    patches=[(['before','decision','chooser'],foreign),(['before','decision','frame','payer'],foreign),(['before','pending_work','work','payer'],foreign),(['call','input','value','actor'],foreign)]
    exercise(151,'OP-R20',pair,patches,pair_check,['actual exact bound current target-spell controller'],[pair_anchor(context,point_i,call_i,path) for path,value in patches])
    pair,context,point_i,call_i=select('OP-R21','ResolutionPayment');original=pair['before']['resolving']['kind']['copied_from'];compare.same(pair['before']['resolving']['kind']['tag'],'SpellCopy','C156/C157 real copied Mana Leak offer')
    patches=[(['before','decision','frame','entry'],original)]
    exercise(156,'OP-R21',pair,patches,pair_check,['actual issued frame/cursor origin'],[pair_anchor(context,point_i,call_i,path) for path,value in patches])
    foreign=41 if pair['before']['decision']['chooser']==3 else 3
    patches+=[(['before','decision','frame','payer'],foreign),(['before','decision','chooser'],foreign),(['call','input','value','actor'],foreign)]
    exercise(157,'OP-R21',pair,patches,pair_check,['actual issued frame/cursor origin'],[pair_anchor(context,point_i,call_i,path) for path,value in patches])

    prefix_context=case('OP-R14')
    prefix_call_i=next(i for i,call in enumerate(prefix_context['calls']) if call['result']['tag']=='Ok' and call['input'].get('value',{}).get('command',{}).get('tag')=='ActivateMana' and prefix_context['points'][call['before_point']][1]['projection']['value']['pending_work'] is not None and prefix_context['points'][call['before_point']][1]['projection']['value']['pending_work']['tag']=='ResolutionPayment' and call['result']['candidate']['mana_production'] is not None)
    prefix_call=prefix_context['calls'][prefix_call_i];prefix_point_i,prefix_point=prefix_context['points'][prefix_call['before_point']];prefix_pair=dict(before=prefix_point['projection']['value'],call=prefix_call)
    def prefix_check(value):
        state_check(value)
        return compare.optional_mana_prefix_join(value['before'],value['call'])
    append=prefix_call['result']['candidate']['append'];cost_i=next(i for i,row in enumerate(append) if row['tag']=='Event' and row['cause']['tag']=='ManaAbilityCost');activation_i=next(i for i,row in enumerate(append) if row['tag']=='Event' and row['cause']['tag']=='ManaAbilityActivated');production_i=next(i for i,row in enumerate(append) if row['tag']=='Event' and row['cause']['tag']=='ManaProduced')
    base=['call','result','candidate'];cost_base=base+['append',cost_i,'occurrences',0,'value'];prior_priority=next(point['projection']['value'] for i,point in prefix_context['points'].values() if point['owner']=='GameState' and point['projection']['value']['window_phase'].startswith('Priority('))
    swapped=list(append);swapped[cost_i],swapped[activation_i]=swapped[activation_i],swapped[cost_i]
    variants={158:[(base+['append'],append[:cost_i]+append[cost_i+1:])],159:[(base+['append'],[append[cost_i],*append])],160:[(base+['append'],append[:activation_i]+append[activation_i+1:])],161:[(base+['append'],swapped)],
        162:[(base+['mana_production'],controls.DELETE)],163:[(base+['append'],[*append,append[production_i]])],164:[(cost_base+['before'],controls.DELETE)],165:[(cost_base+['before'],append[cost_i]['occurrences'][0]['value']['after']),(cost_base+['after'],append[cost_i]['occurrences'][0]['value']['before'])],
        168:[(base+['checks'],controls.DELETE)],169:[(base+['state'],prior_priority)]}
    prefix_guards=['actual accepted mana tap-cost publication exactly once','actual accepted mana activation publication exactly once','actual tap-cost cut precedes activation publication','actual complete mana prefix requires production fact','actual complete mana prefix positive production once and no zero production occurrence','actual mana tap-cost before world cut','actual payment mana prefix requires complete no-check proof','no actual ordinary priority cut during payment mana prefix','malformed evidence']
    for number,patches in variants.items():
        exercise(number,'OP-R14',prefix_pair,patches,prefix_check,prefix_guards,[pair_anchor(prefix_context,prefix_point_i,prefix_call_i,path) for path,value in patches])
    pair,context,point_i,call_i=select('OP-R14','ManaColor');color_append=pair['call']['result']['candidate']['append'];produced_i=next(i for i,row in enumerate(color_append) if row['tag']=='Event' and row['cause']['tag']=='ManaProduced')
    variants={170:[(base+['checks'],controls.DELETE)],171:[(base+['state'],prior_priority)],174:[(base+['append'],color_append[:produced_i]+color_append[produced_i+1:])],175:[(base+['append'],[*color_append,color_append[produced_i]])]}
    for number,patches in variants.items():
        exercise(number,'OP-R14',pair,patches,color_check,['actual color child requires complete no-check proof','no actual ordinary priority cut during color child','actual color child publishes positive production exactly once and no zero production occurrence'],[pair_anchor(context,point_i,call_i,path) for path,value in patches])

    # Separate accepted source authority controls carry no OP-C count. They change
    # complete real exporter Programs independently of every game result.
    export_path=Path(resources['primary']['source_program_export']);export=read(export_path)
    vocabulary_path=Path(resources['primary']['vocabulary'])
    vocabulary=read(vocabulary_path)
    def primary_check(value):return compare.bind_optional_primary_programs(table,value,compiled,coverage,vocabulary)
    for index,source in enumerate(export['rows']):
        other=export['rows'][(index+1)%len(export['rows'])]['program'];path=['rows',index,'program']
        row=controls.exercise_mutation('OP-PROGRAM-'+source['source_definition']['oracleId'],'independent whole source Program',export,lambda value,path=path,other=other:controls.changed(value,[(path,other)]),compare.evidence_boundary(primary_check),'replace entire actual source Program with another complete actual source Program')
        compare.require('separate export agrees with independent complete Program' in row['reason'],'actual full source Program mutation reaches primary value authority')
        authority_rows.append(dict(row,actual_field_anchors=[dict(file=receipt(export_path),json_pointer=ptr(path))],game_packet_used_as_expectation=False,op_c_credit=0))
    for index,template in enumerate(table['nested_token_templates']):
        source_i=next(i for i,row in enumerate(export['rows']) if row['source_definition']['oracleId']==template['source_oracle_id']);coordinate=template['creation_coordinate'];program=export['rows'][source_i]['program'];body=['kind','body'] if coordinate['triggered_ability_id'] is None else ['triggered',next(i for i,row in enumerate(program['triggered']) if row['id']==coordinate['triggered_ability_id']),'body']
        path=['rows',source_i,'program',*body,'instructions',coordinate['instruction'],'creations',coordinate['creation'],'definition'];foreign=table['nested_token_templates'][1-index]['complete_expected_definition']
        row=controls.exercise_mutation('OP-TEMPLATE-'+template['source_oracle_id'],'independent complete nested template',export,lambda value,path=path,foreign=foreign:controls.changed(value,[(path,foreign)]),compare.evidence_boundary(primary_check),'replace entire actual nested token template with the other complete actual template')
        compare.require('separate export agrees with independent complete Program' in row['reason'],'actual complete template mutation reaches independent primary Program authority')
        authority_rows.append(dict(row,actual_field_anchors=[dict(file=receipt(export_path),json_pointer=ptr(path))],game_packet_used_as_expectation=False,op_c_credit=0))

    dispositions=[dict(id=identity,status='EXERCISED_REJECTED' if any(row['id']==identity for row in rows) else 'NOT_EXERCISED') for identity in obligations]
    for path,original in inputs.items():compare.same(receipt(Path(path)),original,'actual immutable mutation input after execution')
    output=dict(kind='ACTUAL_PRIVATE_OPTIONAL_ATOMIC_COMPONENT_CONTROLS',attempt=attempt,inputs=list(inputs.values()),rows=rows,independent_source_program_mutations=authority_rows,dispositions=dispositions,executed=len(rows),required=len(obligations),full220_complete=False,qualification_credit=0,forge_producer_executed_by_reader=False,native_producer_executed_by_reader=False)
    (directory/'result.json').write_text(json.dumps(output,indent=2)+'\n')
    return output


def native(directory:Path,resources:dict[str,Any],m:Any,n:Any,c:Any)->dict[str,Any]:
    ROOT=Path(resources['root']);BASE=ROOT/'work/engine-milestones/optional-payment-v1';AMENDMENT=ROOT/'.quzzar-system-design/architecture-proposed/work-planning-20261103/planning/optional-payment-v1/qualification/control-eligibility-amendment-v2';attempt=directory.name;authority=n.load_actual_authority(m,Path(resources['native_producer']),ROOT)
    m.same(row(AMENDMENT/'acceptance.json')['sha256'],'bcf8db433a37b2e711a3305bd23418ba037e33dc71855a046422911779c12507','actual accepted narrow eligibility amendment');m.same(row(AMENDMENT/'planned-reader-controls.v2.json')['sha256'],'b94529426f7c54fbe9358182edfa6ab6695fa94167d067b71fc35ecb5e689898','actual accepted220 eligibility epoch2');plan={r['id']:r for r in load(AMENDMENT/'planned-reader-controls.v2.json')['controls']};rows=[];inputs={};contexts={};full_baselines={}
    def read(path):inputs[str(path)]=row(path);return load(path)
    def case(name):
        if name not in contexts:
            path=m.bound_file(authority['artifacts'][name],'actual native-only control substrate');document=read(path);capture=n.read_capture(m,path,authority['manifest'],name);full_baselines[name]=capture.provenance
            points={point['point_id']:(i,point) for i,point in enumerate(document['type_context_witness']['snapshots'])};calls=document['type_context_witness']['calls'];roots=[];root=-1;root_by_call={};captures=[];events=[]
            for call in calls:
                if call['input']['tag'].startswith('GameInitialize'):root+=1;captures.append({});events.append({})
                root_by_call[call['call_id']]=root
                for record in call['append'].get('value',[]):
                    if record['tag']=='Event':events[root][record['event']]=record
                    elif record['tag']=='TriggerDiscovered':captures[root][record['value']['instance']]=record['value']
            contexts[name]=dict(path=path,document=document,points=points,calls=calls,root_by_call=root_by_call,captures=captures,events=events)
        return contexts[name]
    def select(name,tag,predicate=lambda before,call:True,command='Reply'):
        context=case(name)
        for call_i,call in enumerate(context['calls']):
            if call['result']['tag']!='Ok' or call['owner_method']!='GameState::advance':continue
            point_i,point=context['points'][call['before_point']];before=point['projection']['value'];decision=before['decision']
            if decision is not None and decision['frame']['tag']==tag and call['input']['value']['command']['tag']==command and predicate(before,call):return dict(before=before,call=call),context,point_i,call_i
        raise m.EvidenceError('actual eligible native-only selector missing '+name+'/'+tag)
    def check_pair(value):
        before=value['before'];m.optional_native_state_signature(before);m.optional_role_join(before)
        m.validate_optional_stack_payloads(before,{row['object']['object']:row['object']['physical_card'] for row in before['objects']},{})
        return m.optional_control_join(before,value['call'],journal_validator=lambda journal:n.native_journal(m,journal))
    def check_prefix(value):
        m.optional_native_state_signature(value['before']);m.optional_role_join(value['before']);return m.optional_mana_prefix_join(value['before'],value['call'])
    def anchor(context,point_i,call_i,path):return (context['path'],(['type_context_witness','snapshots',point_i,'projection','value'] if path[0]=='before' else ['type_context_witness','calls',call_i])+path[1:])
    def exercise(number,name,substrate,patches,reader,guards,anchors):
        guards=[guard for guard in guards if guard!='malformed evidence'];omitted=[path[-1] for path,value in patches if value is c.DELETE and path];guards.extend(reader.__name__+' malformed evidence: '+repr(key) for key in omitted)
        actual=[dict(file=row(path),json_pointer=pointer(keys),source_scope='Actual native-only artifact; no ordinary/Registry/source-card credit') for path,keys in anchors]
        result=c.exercise_optional_atomic(plan[f'OP-C{number:03}'],name,substrate,patches,reader,tuple(guards),actual);rows.append(result);(directory/'rows.json').write_text(json.dumps(rows,indent=2)+'\n')

    pair,context,point_i,call_i=select('OP-N05','ResolutionPayment');append=pair['call']['result']['candidate']['append'];at=next(i for i,r in enumerate(append) if r['tag']=='ResolutionPaymentOutcome');base=['call','result','candidate','append',at]
    m.same(append[at]['outcome']['payment']['spend'],[],'actual N05 explicit zero Paid normalized spend');m.same(append[at]['cost'],dict(generic='0',required={}),'actual N05 zero quote')
    for number,path,value in [(113,base+['entry'],'StackEntryId(999999)'),(114,base+['payer'],1),(115,base+['outcome','tag'],'Declined'),(116,base+['next'],'999999'),(129,['call','result','candidate','append'],append[:at]+append[at+1:]),(130,base+['outcome'],dict(tag='Declined'))]:
        exercise(number,'OP-N05',pair,[(path,value)],check_pair,['actual Paid record/offer','exact payment outcome payload','actual Paid owned successor','one outcome per actual optional/payment reply','actual Declined reply relation'],[anchor(context,point_i,call_i,path)])

    pair,context,point_i,call_i=select('OP-N15','ResolutionPayment',lambda before,call:bool(before['decision']['frame']['cost']['required']))
    append=pair['call']['result']['candidate']['append'];at=next(i for i,r in enumerate(append) if r['tag']=='ResolutionPaymentOutcome');payment=append[at]['outcome']['payment'];m.same(payment['cost']['required'],{'Colourless':'1'},'actual amended N15 required Colourless1 Paid root');m.same(append[at]['outcome']['tag'],'Paid','actual amended N15 is Paid')
    for number,path,value in [(138,['call','result','candidate','append',at,'outcome','payment','cost','required'],{'Blue':'1'}),(139,['call','result','candidate','append',at,'outcome','payment','cost','required','Colourless'],'2')]:exercise(number,'OP-N15',pair,[(path,value)],check_pair,['actual Paid record/offer cost'],[anchor(context,point_i,call_i,path)])

    def role_checker(context,root):
        def check_role(value):
            check_pair(value);n.trigger_source_join(m,value['before'],context['captures'][root],context['events'][root]);m.same(value['before']['active'],context['document']['observations']['root_segments'][root]['initial_state']['active'],'actual native root active player remains independent of payer/controller');return value['before']['pending_work']
        return check_role
    def coupled(pair,actor):
        field='chooser' if pair['before']['decision']['frame']['tag']=='OptionalEffect' else 'payer'
        return [(['before','decision','chooser'],actor),(['before','decision','frame',field],actor),(['before','pending_work','work',field],actor),(['call','input','value','actor'],actor)]
    pair,context,point_i,call_i=select('OP-N03','OptionalEffect');root=context['root_by_call'][pair['call']['call_id']];reader=role_checker(context,root)
    for number,patches in [(144,[(['before','resolving','controller'],1)]),(145,coupled(pair,1))]:exercise(number,'OP-N03',pair,patches,reader,['actual captured resolving entry controller','actual native captured ability controller retains discovery owner'],[anchor(context,point_i,call_i,path) for path,value in patches])
    pair,context,point_i,call_i=select('OP-N03','ResolutionPayment');root=context['root_by_call'][pair['call']['call_id']];reader=role_checker(context,root);members=pair['before']['resolving']['bindings']['trigger']['causes']['members'];foreign=pair['before']['active'];member_path=['before','resolving','bindings','trigger','causes','members']
    variants={146:[(member_path+[0,'related_player'],foreign)],147:coupled(pair,foreign),148:[(member_path,[*members,dict(members[0],related_player=foreign)])],149:coupled(pair,foreign)}
    for number,patches in variants.items():exercise(number,'OP-N03',pair,patches,reader,['actual unique related payer/chooser','actual complete cause set has unique related player'],[anchor(context,point_i,call_i,path) for path,value in patches])
    pair,context,point_i,call_i=select('OP-N04','ResolutionPayment');root=context['root_by_call'][pair['call']['call_id']];reader=role_checker(context,root);active=pair['before']['active'];payer=pair['before']['pending_work']['work']['payer'];controller=pair['before']['resolving']['controller'];m.same(len({active,payer,controller}),3,'actual N04 active payer captured controller all distinct')
    for number,patches in [(152,[(['before','active'],payer)]),(153,coupled(pair,active))]:exercise(number,'OP-N04',pair,patches,reader,['actual native root active player remains independent of payer/controller','actual unique related payer/chooser'],[anchor(context,point_i,call_i,path) for path,value in patches])
    def changed_source(before,call):
        source=before['resolving']['bindings']['trigger']['source']['object'];current=next((row['object'] for row in before['objects'] if row['object']['object']==source['object']),None)
        return current is not None and current['controller'] is not None and current['controller']!=source['controller']
    pair,context,point_i,call_i=select('OP-N03','OptionalEffect',changed_source);root=context['root_by_call'][pair['call']['call_id']];reader=role_checker(context,root);source=pair['before']['resolving']['bindings']['trigger']['source']['object'];current=next(row['object'] for row in pair['before']['objects'] if row['object']['object']==source['object'])
    stack_i=next(i for i,row in enumerate(pair['before']['stack']) if row['entry']==pair['before']['resolving']['entry'])
    for number,patches in [(154,[(['before','resolving','bindings','trigger','source','object','controller'],current['controller']),(['before','stack',stack_i,'bindings','trigger','source','object','controller'],current['controller'])]),(155,coupled(pair,current['controller']))]:exercise(number,'OP-N03',pair,patches,reader,['actual native complete captured cause/source binding','actual captured resolving entry controller'],[anchor(context,point_i,call_i,path) for path,value in patches])

    pair,context,point_i,call_i=select('OP-N08','ResolutionPayment',lambda before,call:call['result']['candidate']['state']['pending_work']['tag']=='ManaColor',command='ActivateMana');append=pair['call']['result']['candidate']['append'];at=next(i for i,r in enumerate(append) if r['tag']=='TriggerDiscovered')
    def check_waiting(value):check_prefix(value);n.tap_waiting_join(m,value['before'],value['call']);return value['call']['result']['candidate']['state']['waiting']
    for number,value in [(166,append[:at]+append[at+1:]),(167,[*append,append[at]])]:
        path=['call','result','candidate','append'];exercise(number,'OP-N08',pair,[(path,value)],check_waiting,['actual tap waiting capture equals complete once-only discovery'],[anchor(context,point_i,call_i,path)])
    declined,context,point_i,call_i=select('OP-N08','ResolutionPayment');after=declined['call']['result']['candidate']['state'];m.require(any(int(v)>0 for r in after['mana'] for v in r['pool'].values()),'actual N08 decline retains a nonzero accepted production prefix')
    old_pool=context['document']['observations']['root_segments'][0]['initial_state']['mana']
    for number,path,value in [(172,['call','result','candidate','state','mana'],c.DELETE),(173,['call','result','candidate','state','mana'],old_pool)]:exercise(number,'OP-N08',declined,[(path,value)],check_pair,['actual Declined requires complete returned mana pools','actual Declined preserves complete accepted mana prefix without debit'],[anchor(context,point_i,call_i,path)])

    context=case('OP-N16');doc=context['document'];replay=doc['observations']['replay'];branch=replay['branches'][0];saved=branch['saved_states'];prefix=next(i for i,state in enumerate(saved) if state['pending_work'] is not None and state['pending_work']['tag']=='ManaColor' and state['pending_work']['parent']['tag']=='Payment');base=['observations','replay','branches',0]
    def check_replay(value):return n.saved_replay_join(m,value['observations'],value['type_context_witness']['calls'])
    color_probe=branch['probes'][0];probe_call=next(i for i,call in enumerate(context['calls']) if call['input'].get('value')==color_probe['input']);nonempty=next(j for j in branch['saved_journals'] if j)
    variants={176:[(base+['saved_journals',0],c.DELETE)],177:[(base+['saved_journals',0],nonempty)],178:[(base+['saved_inputs',0],c.DELETE)],179:[(base+['saved_inputs',0,'actor'],1)],180:[(base+['saved_journals',1],c.DELETE)],181:[(base+['saved_journals',1],[])],
        182:[(base+['probes',1],c.DELETE)],183:[(base+['probes',1,'error','error_path'],['WrongDecision'])],184:[(base+['saved_states',1,'basis'],c.DELETE)],185:[(base+['saved_states',1,'basis','revision'],'999')],186:[(base+['final_state','basis'],c.DELETE)],187:[(base+['final_state','basis','revision'],'999')],
        188:[(base+['saved_states',prefix,'pending_work','parent'],c.DELETE)],189:[(base+['saved_states',prefix,'pending_work','parent','work','payer'],1)],190:[(['type_context_witness','calls',probe_call,'adopted'],c.DELETE)],191:[(['type_context_witness','calls',probe_call,'adopted'],True)],192:[(base+['saved_inputs',0],c.DELETE)],193:[(base+['saved_inputs',0,'basis','game'],'999')],194:[(base+['replayed_states',-1],c.DELETE)],195:[(base+['replayed_states',-1,'basis','revision'],'999')]}
    first_input_i=next(i for i,call in enumerate(context['calls']) if call['input'].get('value')==branch['saved_inputs'][0])
    variants[192]=[(['type_context_witness','calls',first_input_i,'replay','input_storage'],c.DELETE)]
    variants[193]=[(['type_context_witness','calls',first_input_i,'input','value','basis','game'],'999')]
    guards=['actual saved replay includes initial journal and every accepted append','actual saved replay includes initial and every accepted full state','actual saved replay retains complete initializer journal','actual saved replay complete two-probe inventory','actual denied probe complete typed Result','actual saved trace complete candidate/checkpoint state','actual saved trace complete accepted journal','actual saved branch complete adopted input collection','actual replay retains exact original typed input bytes/value','actual whole final native state equality','actual saved replay changed/denied probe stays outside adoption boundary','actual rerun includes every complete state','actual entire native reexecution checkpoint collection','actual native-only call requires typed adoption boundary','exact complete native-only captured call fields','exact complete native-only replay evidence','malformed evidence']
    for number,patches in variants.items():exercise(number,'OP-N16',doc,patches,check_replay,guards,[(context['path'],path) for path,value in patches])

    context=case('OP-N23');doc=context['document'];values={p['point_id']:p['projection']['value'] for p in doc['type_context_witness']['snapshots']};calls=context['calls'];zero_i=next(i for i,call in enumerate(calls) if call['result']['tag']=='Ok' and call['input'].get('value',{}).get('command',{}).get('tag')=='ActivateMana' and call['result']['candidate']['mana_production']['produced']=={});zero=calls[zero_i];before=values[zero['before_point']];state=zero['result']['candidate']['state'];base=['type_context_witness','calls',zero_i,'result','candidate'];decision_before=context['points'][zero['before_point']][0];root_after=next(i for i,call in enumerate(calls) if call['result']['tag']=='Ok' and call['input'].get('value',{}).get('command',{}).get('tag')=='Reply');root_point=context['points'][calls[root_after]['before_point']][0]
    def check_zero(value):return n.zero_count_refresh_join(m,value['type_context_witness']['calls'],{p['point_id']:p['projection']['value'] for p in value['type_context_witness']['snapshots']})
    variants={213:[(['type_context_witness','snapshots',root_point,'projection','value','decision','frame','revision'],before['decision']['frame']['revision'])],214:[(['type_context_witness','snapshots',root_point,'projection','value','decision','frame','revision'],c.DELETE)],215:[(base+['state','pending_work'],dict(tag='Resolution',cursor=state['resolution_state']))],216:[(base+['state','pending_work'],c.DELETE)],217:[(base+['mana_production','produced'],{'Blue':'1'})],218:[(base+['mana_production','produced'],c.DELETE)]}
    direct_i=next(i for i,call in enumerate(calls) if call['owner_method']=='EffectWindow::resume_rule');variants[219]=[(['type_context_witness','calls',direct_i,'result','error_path'],['WrongRevision'])];variants[220]=[(['type_context_witness','calls',direct_i],c.DELETE)]
    guards=['actual zero-count fresh payment frame exists before root reply dispatch','actual fresh frame/executor revision','actual complete mana prefix restores same payment owner','actual mana prefix returns its payment owner or child','actual complete mana prefix positive production once and no zero production occurrence','actual zero-count native prefix exactly once','actual zero-count old direct frame probe retained','actual old direct frame rejected after zero-count prefix','malformed evidence']
    for number,patches in variants.items():exercise(number,'OP-N23',doc,patches,check_zero,guards,[(context['path'],path) for path,value in patches])

    pair,context,point_i,call_i=select('OP-N18','OptionalEffect',lambda before,call:call['input']['value']['command']['choice']['choice']=='Take')
    def check_child(value):return n.optional_child_join(m,value['before'],value['call'])
    path=['call','result','candidate','state','resolution_state','instruction'];exercise(200,'OP-N18',pair,[(path,'0')],check_child,['actual optional child retains original cursor entry and locals'],[anchor(context,point_i,call_i,path)])
    pair,context,point_i,call_i=select('OP-N19','CreatureType');append=pair['call']['result']['candidate']['append'];chosen=next(row for row in append if row['tag']=='Event' and any(item['value']['tag']=='CreatureTypeChosen' for item in row['occurrences']))
    def check_assignment(value):return n.creature_assignment_join(m,value['before'],value['call'])
    path=['call','result','candidate','append'];exercise(201,'OP-N19',pair,[(path,[*append,chosen])],check_assignment,['actual optional type child assigns exactly once'],[anchor(context,point_i,call_i,path)])
    context=case('OP-N20');doc=context['document'];denied_i=next(i for i,call in enumerate(context['calls']) if call['result']['tag']=='Err' and call['input'].get('value',{}).get('command',{}).get('choice')==dict(tag='OptionalEffect',choice='Take'));point_i,point=context['points'][context['calls'][denied_i]['before_point']];source=point['projection']['value'];base=['type_context_witness','snapshots',point_i,'projection','value']
    def check_bounced(value):return n.bounced_optional_join(m,value['type_context_witness']['calls'],{point['point_id']:point['projection']['value'] for point in value['type_context_witness']['snapshots']})
    patches=[(base+['decision','frame','options'],['Take','Decline']),(base+['pending_work','work','options'],['Take','Decline'])];exercise(202,'OP-N20',doc,patches,check_bounced,['actual unavailable Take omitted from issued domain','actual unavailable Take omitted from owned domain'],[(context['path'],path) for path,value in patches])
    fresh_i=next(i for i,row in enumerate(source['objects']) if row['object']['zone']['kind']=='hand');path=base+['objects',fresh_i,'object','zone'];exercise(203,'OP-N20',doc,[(path,dict(kind='battlefield'))],check_bounced,['actual rejected unavailable Take retains latest whole bounced-target state'],[(context['path'],path)])
    pair,context,point_i,call_i=select('OP-N19','OptionalEffect',lambda before,call:call['input']['value']['command']['choice']['choice']=='Decline')
    def check_skip(value):return n.declined_local_join(m,value['before'],value['call'])
    path=['call','result','candidate','append'];exercise(204,'OP-N19',pair,[(path,[*pair['call']['result']['candidate']['append'],chosen])],check_skip,['actual optional skip leaves local unassigned without assignment publication'],[anchor(context,point_i,call_i,path)])
    context=case('OP-N21');call=context['calls'][0]
    def check_donor(value):return n.moved_donor_join(m,value)
    exercise(205,'OP-N21',call,[(['result'],dict(tag='Ok'))],check_donor,['actual merged optional moved-donor Program is rejected'],[(context['path'],['type_context_witness','calls',0,'result'])])
    for primitive in ('CreateTokens','Damage'):
        pair,context,point_i,call_i=select('OP-N22','OptionalEffect',lambda before,call:call['input']['value']['command']['choice']['choice']=='Take' and before['resolving']['body']['instructions'][int(before['resolution_state']['instruction'])+1]['op']==primitive)
        path=['call','result','candidate','state','decision','frame','tag'];exercise(206,'OP-N22',pair,[(path,'CopyTargets')],check_child,['actual optional primitive preserves its real child kind'],[anchor(context,point_i,call_i,path)]);rows[-1]['member_id']=primitive
    pair,context,point_i,call_i=select('OP-N08','ResolutionPayment',lambda before,call:call['result']['candidate']['state']['pending_work']['tag']=='ManaColor',command='ActivateMana');append=pair['call']['result']['candidate']['append'];discovered_i=next(i for i,row in enumerate(append) if row['tag']=='TriggerDiscovered');activated_i=next(i for i,row in enumerate(append) if row['tag']=='Event' and row['cause']['tag']=='ManaAbilityActivated');event=append[activated_i]['event']
    def check_tap(value):return n.tap_waiting_join(m,value['before'],value['call'])
    patches=[(['call','result','candidate','state','waiting',0,'causes','event'],event),(['call','result','candidate','append',discovered_i,'value','causes','event'],event)];exercise(207,'OP-N08',pair,patches,check_tap,['actual BecomesTapped waiting cause is tap cost only'],[anchor(context,point_i,call_i,path) for path,value in patches])
    path=['call','result','candidate','append'];exercise(208,'OP-N08',pair,[(path,append[:activated_i]+append[activated_i+1:])],check_prefix,['actual accepted mana activation publication exactly once'],[anchor(context,point_i,call_i,path)])
    context=case('OP-N24');doc=context['document'];root=-1;activation_sources={}
    for call in context['calls']:
        if call['input']['tag'].startswith('GameInitialize'):root+=1
        for record in call['append'].get('value',[]):
            if record['tag']=='Event':
                for item in record['occurrences']:
                    if item['value']['tag']=='ActivatedAbility':activation_sources[root]=item['value']['source']
    pair,context,point_i,call_i=select('OP-N24','OptionalEffect');source=activation_sources[context['root_by_call'][pair['call']['call_id']]]
    def check_receiver(value):return n.receiver_join(m,value['before'],value['before']['decision']['frame'],source)
    patches=[(['before','decision','frame','options'],['Take','Decline']),(['before','pending_work','work','options'],['Take','Decline'])];exercise(209,'OP-N24',pair,patches,check_receiver,['actual absent receiver has Decline-only owned domain','actual absent receiver has Decline-only issued domain'],[anchor(context,point_i,call_i,path) for path,value in patches])
    supplied=doc['observations']['supplied_owners'][0];source=activation_sources[int(supplied['preceding_root_ordinal'])];state=supplied['state'];fresh=next(row['object'] for row in state['objects'] if row['object']['physical_card']==source['physical_card'])
    actual_receiver_frame=next(call['input']['operands']['frame'] for call in context['calls'] if call['owner_method']=='EffectWindow::resume_rule')
    def check_returned(value):return n.receiver_join(m,value,actual_receiver_frame,source)
    path=['resolving','kind','source'];exercise(210,'OP-N24',state,[(path,fresh)],check_returned,['actual optional receiver remains exact captured activation source'],[(context['path'],['observations','supplied_owners',0,'state',*path])])

    # Genuine external predecessor adjuncts keep their own report/profile names.
    # They are never relabeled as native-only trajectories or Registry evidence.
    case('OP-N16');groups=resources['api_groups']
    (directory/'actual-byte-mutations').mkdir()
    for label,original,current,expected in groups:
        m.same(row(original)['sha256'],expected,'actual immutable predecessor API source bytes');m.same(row(current)['sha256'],expected,'actual source-stable current typed API bytes');inputs[str(original)]=row(original);inputs[str(current)]=row(current);data=current.read_bytes();m.require(data.startswith(b'{"status":"success","data":'),'actual typed API status field source position')
        for number,value in [(196,data.replace(b'"status":"success",',b'',1)),(197,data.replace(b'"status":"success"',b'"status":"fail"',1))]:
            changed_file=directory/'actual-byte-mutations'/(label+'-'+str(number)+'.api.json');changed_file.write_bytes(value)
            def check_api(path,original=original):return m.equal_api_bytes(original,path)
            exercise(number,'OP-N16',current,[([],changed_file)],check_api,['whole predecessor API bytes including field order/name/omission'],[(current,['status'])]);rows[-1].update(member_id=label,actual_mutated_bytes=row(changed_file),scope='Explicit external genuine predecessor API byte adjunct; original report/profile retained')
    legacy_path=Path(resources['historical_native']);legacy=read(legacy_path);manifest_path=Path(resources['historical_manifest']);legacy_manifest=read(manifest_path)
    def check_namespace(value):report,provenance=m.unwrap_native(value,False,legacy_manifest,m.PROFILES[1]);return dict(report=report,provenance=provenance)
    m.same(legacy['schema_version'],9,'actual retained historical current9 namespace');m.same(legacy['type_context_witness']['schema'],'spellwake.current-type-context-witness.v7','actual retained historical witness7 namespace')
    for number,path,value in [(198,['schema_version'],c.DELETE),(199,['type_context_witness','schema'],'spellwake.current-type-context-witness.v8')]:exercise(number,'OP-N16',legacy,[(path,value)],check_namespace,['exact current mandatory outer fields','actual witness7'],[(legacy_path,path)]);rows[-1]['scope']='Explicit external historical9/7 predecessor namespace adjunct; not a NativeOnly packet'

    for path,before in inputs.items():m.same(row(Path(path)),before,'actual native atomic immutable source input after execution')
    result=dict(kind='ACTUAL_PRIVATE_NATIVE_ONLY_ATOMIC_COMPONENT_CONTROLS',attempt=attempt,inputs=list(inputs.values()),rows=rows,executed=len(rows),unique_controls=len({r['id'] for r in rows}),active_eligibility_epoch=row(AMENDMENT/'planned-reader-controls.v2.json'),eligibility_acceptance=row(AMENDMENT/'acceptance.json'),complete_native_baselines=full_baselines,full220_complete=False,qualification_credit=0,ordinary_or_registry_credit=False)
    (directory/'result.json').write_text(json.dumps(result,indent=2)+'\n')
    return result


def development_resources(root:Path,producer:Path|None=None)->dict[str,Any]:
    """Explicit historical development substrates, granting no final-image credit."""
    base=root/'work/engine-milestones/optional-payment-v1';native_files={}
    for n in range(1,25):
        name=f'OP-R{n:02}';epoch=2 if n==2 else 5 if n in (3,12,16) else 4 if n in (9,11,13,14,18,20,21,23) else 6;native_files[name]=base/f'ordinary-development-v1/reduced-attempt{epoch}'/name/'native.json'
    primary=dict(selected19=base/'source-preparation/selected19.full-inventory.json',metadata=root/'work/catalogue/oracle-metadata.json',snapshot=root/'work/catalogue/oracle-cards-20261029210158.jsonl.gz',forge_selected19=root/'.quzzar-system-design/architecture-proposed/work-planning-20261103/planning/optional-payment-v1/qualification/reference-preparation/selected19-forge-sources.json',compiled=root/'crates/reference_tools/src/ordinary_game/optional-inputs-v1.json',primary_program_table=base/'source-program-authority-review-v1/comparison-table.json',source_program_export=base/'ordinary-development-v1/export-attempt1/source-programs.json',vocabulary=root/'work/resolution-value-choices/current-s1-packets-v1/creature-vocabulary.input.v1.json')
    typed=base/'ordinary-development-v1/predecessor-api-attempt2/api';old=root/'work/engine-milestones/mandatory-trigger-v1/implementation-v1/baseline/attempt1/api-before';probe=base/'report3-api-preservation-v1/isolated-typed-probe-v1';groups=[('historical-report1',old/'historical-all.api.json',typed/'historical-all.api.json','baa07079e96a7efabd58c81877f767b955521b3b349f4b1a53fea3e76c7507c9'),('teaching-report1',old/'teaching-all.api.json',typed/'teaching-all.api.json','561d8ad9a6a5cd44b84d89197a09a5da9c7312e7ddd8ee0280eb053bd244ce16'),('interaction-report2',old/'interaction-all.api.json',typed/'interaction-all.api.json','22bd625f8a3a40cc590fa27d913db96d9ad9a2f789186646ab157701d5cfc889'),('mandatory-report3',probe/'runs/all23/old/stdout.api.json',probe/'runs/all23/current/stdout.api.json','226425ff56e60100b039bc55431fbe9f857ebe605b930bb115982c9bdf71f638')]
    return dict(root=root,primary=primary,native_files=native_files,reference_runs={'OP-R16':base/'reference-execution-v1/attempt5-r03-r12-r16/capture'},native_producer=producer or base/'ordinary-development-v1/native-packets-attempt1',api_groups=groups,historical_native=root/'work/engine-milestones/mandatory-trigger-v1/native-capture-attempt2/cases/T04.one-life/native.json',historical_manifest=root/'.quzzar-system-design/architecture-proposed/work-planning-20261103/planning/mandatory-trigger-v1/content/preparation/content-manifest.json')


def final_controls(m:Any,c:Any,envelope_path:Path,directory:Path)->dict[str,Any]:
    """Run the real all26 baseline and all220 component selectors on explicit resources."""
    import interaction_reference_optional_admission as admission
    import interaction_reference_native_optional as n
    envelope=m.read(envelope_path);baseline=admission.compare_final26(m,envelope_path);accepted=m.read(Path(envelope['source_acceptance']['path']));root=Path(accepted['cwd']);resources=development_resources(root)
    # Every final ordinary/Forge substrate replaces its development resource.
    resources['primary']={name:Path(value['path']) for name,value in envelope['primary'].items()};resources['native_files']={entry['case_id']:m.bound_file(m.read(Path(entry['execution']['path']))['stdout'],'actual final control native substrate') for entry in envelope['native_cases']};resources['reference_runs']={name:Path(envelope['reference_run']) for name in resources['native_files']};native_process=admission.file(m,envelope['supporting_epochs']['native_packets'],'actual separately named native-only control epoch');m.same(native_process.name,'receipt.json','actual maintained native-only process receipt');resources['native_producer']=native_process.parent
    api_index=m.read(admission.file(m,envelope['supporting_epochs']['predecessor_api'],'actual separately named typed API epoch'));m.same(set(api_index),{'kind','schema_version','groups','process_receipts'},'closed typed API control adjunct index');m.same(api_index['kind'],'ActualTypedPredecessorControlAdjuncts','explicit typed predecessor adjunct');m.same(api_index['schema_version'],1,'typed predecessor adjunct schema1');groups=m.unique(api_index['groups'],'id','actual typed API group');expected={name:digest for name,_,_,digest in resources['api_groups']};m.same(set(groups),set(expected),'exact four genuine typed API groups');resources['api_groups']=[]
    for name,entry in groups.items():
        m.same(set(entry),{'id','before','current'},'closed actual typed API group');before=admission.file(m,entry['before'],'actual immutable typed predecessor bytes');current=admission.file(m,entry['current'],'actual current typed predecessor bytes');m.same(entry['before']['sha256'],expected[name],'exact genuine old typed API byte identity');m.equal_api_bytes(before,current);resources['api_groups'].append((name,before,current,expected[name]))
    m.require(bool(api_index['process_receipts']),'actual typed API adjunct producer receipts named')
    for value in api_index['process_receipts']:admission.file(m,value,'actual typed API producer receipt (separate epoch; no source inheritance)')
    directory.mkdir(parents=True,exist_ok=False);ordinary_dir=directory/'ordinary';native_dir=directory/'native';ordinary_dir.mkdir();native_dir.mkdir();first=ordinary(ordinary_dir,resources,m,c);second=native(native_dir,resources,m,n,c);rows=first['rows']+second['rows'];ids={r['id'] for r in rows};m.same(ids,{f'OP-C{i:03}' for i in range(1,221)},'actual all220 optional component obligations');m.same(len(rows),227,'actual distinct enum/API member experiment count');m.require(all(r['status']=='EXERCISED_REJECTED' and r['baseline_passed'] and r['restoration_passed'] for r in rows),'actual every baseline/rejection/restoration required')
    return dict(status='ACTUAL_OPTIONAL220_COMPONENT_CONTROLS_PASS',baseline=baseline,named_obligations=220,actual_experiments=len(rows),all220_exercised_and_rejected=True,ordinary_result=row(ordinary_dir/'result.json'),native_result=row(native_dir/'result.json'),independent_source_authority_mutations=first['independent_source_program_mutations'],native_and_API_epochs_separate=True,source_card_qualification=False,full_wave_qualification=False,qualification_credit=0)
