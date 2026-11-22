#!/usr/bin/env python3
"""Strict source/current-image authority boundary for the final optional26 CLI.

The input envelope is a reader-owned contract for root's future actual frozen
capture stage. No final acceptance envelope or successful final26 run is created
by this module. Historical development receipts do not satisfy its guard schema.
"""
from __future__ import annotations
from pathlib import Path
from typing import Any
import re
import importlib.util

# Root accepted the isolated v3 producer correction before final capture. Bind
# executable enumeration code and its original membership seed before import.
SOURCE_OWNER_SHA='8393fb0f1645ea6d2def0ce1eb35fd25b1674e9e0e85d2b559d447ab4cb43790'
SOURCE_SEED_SHA='40ff44dabce69775b7514738e267c9e66288ef8ee66ceeb46bf2d43b2ea266d4'

def source_inventory_owner(m:Any,root:Path)->Any:
    """Load only the approved actual producer's existing enumeration owner."""
    base=root/'work/engine-milestones/optional-payment-v1/ordinary-development-v1'
    path=base/'run_development.py';seed=base/'checks-attempt1/source-before.json'
    m.same(m.sha(path),SOURCE_OWNER_SHA,'approved source inventory owner before import')
    m.same(m.sha(seed),SOURCE_SEED_SHA,'approved source inventory seed before import')
    spec=importlib.util.spec_from_file_location('optional_final_source_inventory_owner',path)
    m.require(spec is not None and spec.loader is not None,'actual source inventory import owner')
    owner=importlib.util.module_from_spec(spec);exec(compile(path.read_bytes(),str(path),'exec'),owner.__dict__)
    m.same(owner.ROOT.resolve(),root.resolve(),'actual inventory owner ROOT')
    m.same(owner.BASE.resolve(),base.resolve(),'actual inventory owner BASE')
    m.same(Path(owner.__file__).resolve(),path.resolve(),'actual inventory owner file')
    m.same(m.sha(path),SOURCE_OWNER_SHA,'approved source inventory owner after import')
    m.same(m.sha(seed),SOURCE_SEED_SHA,'approved source inventory seed after import')
    return owner

def observe_source_inventory(m:Any,owner:Any,rows:list[dict[str,Any]])->list[dict[str,Any]]:
    """Require exact membership and bytes; additions are re-enumerated by producer."""
    m.same(m.sha(Path(owner.__file__)),SOURCE_OWNER_SHA,'approved source inventory owner during reader')
    m.same(m.sha(owner.BASE/'checks-attempt1/source-before.json'),SOURCE_SEED_SHA,'approved source inventory seed during reader')
    actual=owner.source_snapshot()
    m.same({row['path'] for row in rows},{row['path'] for row in actual},'complete approved producer selected source membership')
    m.same(rows,actual,'complete approved producer source bytes and deterministic order')
    return actual

PRIMARY_FIELDS=set('compiled recipes selected19 forge_selected19 metadata snapshot vocabulary publication registry_output registry_execution source_program_export primary_program_table'.split())
ACCEPTANCE_FIELDS=set('kind schema_version profile cwd source_before source_after build_receipt image check_receipts reader_helpers reference_files explicit_inputs capture_helper'.split())
ENVELOPE_FIELDS=set('schema_version profile source_acceptance primary native_cases reference_run supporting_epochs'.split())
CHECKS={
    'fmt':['cargo','fmt','--all','--check'],
    'check':['cargo','check','--locked','--workspace','--all-targets'],
    'clippy':['cargo','clippy','--locked','--workspace','--all-targets','--','-D','warnings'],
    'workspace-tests':['cargo','test','--locked','--workspace'],
    'optional-registry-tests':['cargo','test','--locked','-p','spellwake-card-programs','--test','optional_payment_registry','--','--include-ignored'],
}

def file(m:Any,value:dict[str,Any],label:str)->Path:
    """Use the maintained file authority observer, requiring a full byte receipt."""
    m.same(set(value),{'path','sha256','bytes'},label+' exact file receipt');m.require(type(value['bytes']) is int and value['bytes']>=0,label+' byte length');return m.bound_file(value,label)

def primary_authorities(m:Any,bundle:dict[str,Any])->dict[str,Any]:
    """Bind whole expected Programs to the independent primary table, not a game."""
    m.same(set(bundle),PRIMARY_FIELDS,'exact optional primary authority files');paths={name:file(m,value,'actual optional primary '+name) for name,value in bundle.items()}
    for name in ('metadata','snapshot','vocabulary','publication'):m.same(bundle[name]['sha256'],m.NATIVE_INPUT_PINS[name],'exact optional primary input '+name)
    for name,digest in [('compiled','d7858e932aff0de4960a7abe848a87061a350c69523a8917543dbb61ff35d1c2'),('recipes','ce084e897cd3e6b3af4412ab6d431e600642a7d6b1c0bf89496470f698fab46e'),('primary_program_table','1d0ce86b0b9d59af4b9e6352864f79da878f622c4a1da27f5a7b7e8b0c5b3142')]:m.same(bundle[name]['sha256'],digest,'exact reviewed primary '+name)
    data={name:m.read(path) for name,path in paths.items() if name not in ('snapshot','publication')};execution=data['registry_execution'];m.same(execution['exit_code'],0,'actual Registry8 source-owner process');m.same(execution['source_stable'],True,'actual Registry8 source-owner stable');m.same(execution['stdout'],bundle['registry_output'],'actual Registry8 output/process binding');m.same(data['registry_output']['status'],'success','actual Registry8 source-owner output')
    for image in execution['actual_executables']:file(m,image['retained'],'actual Registry8 retained image')
    compiled=data['compiled'];recipes=data['recipes'];vocabulary=data['vocabulary'];coverage=data['registry_output']['data'];table=data['primary_program_table'];m.optional_source_join(compiled,recipes);m.optional_raw_source_join(compiled,data['selected19'],data['forge_selected19'],metadata=data['metadata'],source_receipt=bundle['snapshot'],primary=table);programs=m.bind_optional_primary_programs(table,data['source_program_export'],compiled,coverage,vocabulary)
    return dict(compiled=compiled,recipes=recipes,native_registry_coverage=coverage,catalogue=data['selected19']['source'],vocabulary=dict(bundle['vocabulary'],id=vocabulary['id']),definition_sources=[dict(path='work/catalogue/oracle-cards-20261029210158.jsonl.gz',sha256=data['selected19']['source']['sha256'],uri=data['selected19']['metadata']['jsonl_download_uri'])],source_programs=programs,primary_program_authority=dict(status='ACCEPTED_FINITE_SOURCE19_WHOLE_PROGRAM_AND_TWO_COMPLETE_TEMPLATES',table=bundle['primary_program_table'],source_export=bundle['source_program_export'],global_fully_qualified_cards=0))

def source_rows(m:Any,acceptance:dict[str,Any])->list[dict[str,Any]]:
    before=m.read(file(m,acceptance['source_before'],'actual accepted source before'));after=m.read(file(m,acceptance['source_after'],'actual accepted source after'));m.same(after,before,'actual complete accepted source pre/post');m.require(isinstance(before,list) and bool(before),'actual accepted source row inventory');m.unique(before,'path','accepted source')
    for value in before:file(m,value,'actual current accepted source')
    return before

def current_epoch(m:Any,acceptance_path:Path)->dict[str,Any]:
    """Require actual current sources, checks, helpers and exactly one native image."""
    a=m.read(acceptance_path);m.same(set(a),ACCEPTANCE_FIELDS,'closed optional current-source acceptance');m.same(a['kind'],'RootAcceptedCurrentOptional26Epoch','explicit root current26 source acceptance');m.same(a['schema_version'],1,'optional source acceptance schema1');m.same(a['profile'],'optional-payment-v1','optional accepted source profile');cwd=Path(a['cwd']);m.require(cwd.is_absolute(),'actual source acceptance cwd');m.same(cwd.resolve(),Path(m.__file__).resolve().parents[1],'actual installed reader/current source root');rows=source_rows(m,a);owner=source_inventory_owner(m,cwd);observe_source_inventory(m,owner,rows)
    m.same(set(a['explicit_inputs']),set(m.NATIVE_INPUT_PINS),'exact four accepted explicit inputs')
    for name,value in a['explicit_inputs'].items():file(m,value,'actual accepted explicit input');m.same(value['sha256'],m.NATIVE_INPUT_PINS[name],'actual accepted pinned input '+name)
    reader_names={'interaction_reference_compare.py','interaction_reference_controls.py','interaction_reference_native_optional.py','interaction_reference_optional_admission.py','interaction_reference_optional_controls.py'};m.same({Path(r['path']).name for r in a['reader_helpers']},reader_names,'all five actual frozen reader owners');m.same(len(a['reader_helpers']),5,'unique current reader helpers')
    for value in a['reader_helpers']:
        m.same(Path(value['path']).resolve(),(cwd/'scripts'/Path(value['path']).name).resolve(),'actual canonical reader owner path');file(m,value,'actual current reader helper')
    reference_names={'SpellwakeBlueInteractionReferenceTest.java','interaction_forge_reference.py','interaction_reference_compare.py','interaction_reference_controls.py','teaching_reference_compare.py','teaching_forge_reference.py'};m.same({Path(r['path']).name for r in a['reference_files']},reference_names,'exact current reference source closure');m.same(len(a['reference_files']),6,'unique current reference owners')
    for value in a['reference_files']:
        expected=cwd/('tests/reference/forge/SpellwakeBlueInteractionReferenceTest.java' if Path(value['path']).name=='SpellwakeBlueInteractionReferenceTest.java' else 'scripts/'+Path(value['path']).name);m.same(Path(value['path']).resolve(),expected.resolve(),'actual canonical reference owner path');file(m,value,'actual current reference owner')
    file(m,a['capture_helper'],'actual frozen final native capture helper');image=file(m,a['image'],'actual accepted single native image');m.same(image.name,'optional-payment-game','actual optional CLI image');build_path=file(m,a['build_receipt'],'actual accepted CLI build receipt');m.same(build_path.parent.parent.resolve(),owner.BASE.resolve(),'actual build receipt belongs to approved producer BASE');build=m.read(build_path);m.same(build['runner'],next(row for row in rows if row['path']==str(Path(owner.__file__).resolve())),'actual build uses approved source inventory owner');m.same(build['exit_code'],0,'actual final CLI build success');build_command=[part for part in build['command'] if not part.startswith('--message-format=')];m.same(build_command,['cargo','build','--locked','-p','spellwake-reference-tools','--bin','optional-payment-game'],'actual locked single optional CLI build command');m.same(build['source_stable'],True,'actual final CLI build source stable');m.same(Path(build['cwd']).resolve(),cwd.resolve(),'actual final build cwd');m.same(m.read(build_path.parent/'source-before.json'),rows,'actual built source equals accepted source');m.same(m.read(build_path.parent/'source-after.json'),rows,'actual built source successor equals accepted source');matches=[v for v in build['actual_executables'] if v['target']['name']=='optional-payment-game'];m.same(len(matches),1,'actual unique optional built image');m.same(matches[0]['retained'],a['image'],'actual copied runtime/build image identity');file(m,matches[0]['original'],'actual original built image');m.same(matches[0]['original']['sha256'],a['image']['sha256'],'actual original/copied image bytes')
    m.same(set(a['check_receipts']),set(CHECKS),'exact required current native check inventory')
    for name,value in a['check_receipts'].items():
        path=file(m,value,'actual current check '+name);m.same(path.parent.parent.resolve(),owner.BASE.resolve(),'actual check receipt belongs to approved producer BASE');check=m.read(path);m.same(Path(check['cwd']).resolve(),cwd.resolve(),'actual named check ROOT');m.same(check['runner'],build['runner'],'actual check uses same approved source inventory owner');command=[part for part in check['command'] if not part.startswith('--message-format=')];m.same(command,CHECKS[name],'actual named locked check command '+name);m.same(check['exit_code'],0,'actual check exit '+name);m.same(check['source_stable'],True,'actual check source guard '+name);m.same(m.read(path.parent/'source-before.json'),rows,'actual check/source epoch '+name);m.same(m.read(path.parent/'source-after.json'),rows,'actual check/source successor '+name);log=m.bound_file(check['stdout'],'actual check stdout');m.bound_file(check['stderr'],'actual check stderr')
        if name.endswith('tests'):
            counts=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;',log.read_text());m.require(bool(counts) and sum(int(r[0]) for r in counts)>0 and all(int(r[1])==0 for r in counts),'actual nonzero successful named test execution '+name)
    return dict(acceptance=a,source_rows=rows,build=build,build_path=build_path,cwd=cwd,source_inventory_owner=owner)

def capture_sources(m:Any,epoch:dict[str,Any])->dict[str,Any]:
    """Derive the exact39 native owners from the real guarded writer declarations."""
    root=epoch['cwd'];retained=epoch['build_path'].parent/'source';indexed=m.unique(epoch['source_rows'],'path','accepted current source');paths=[]
    for relative,pattern,count in [('crates/reference_tools/src/bin/optional-payment-game.rs',r'let mut paths = \[(.*?)\]\s*\.map',24),('crates/reference_tools/src/current_capture.rs',r'for path in \[(.*?)\]\s*\{\s*let path = repo.join',14)]:
        source=retained/relative;m.same(m.sha(source),indexed[str(root/relative)]['sha256'],'actual retained writer source declaration');blocks=re.findall(pattern,source.read_text(),re.S);m.same(len(blocks),1,'actual unique capture source array');members=re.findall(r'"(crates/[^"\n]+)"',blocks[0]);m.same(len(members),count,'actual declared native source count');m.same(len(set(members)),count,'distinct native source members');m.same(re.sub(r'"crates/[^"\n]+"|[\s,]','',blocks[0]),'','only actual source path literals');paths.extend(str(root/member) for member in members)
    paths.extend(epoch['acceptance']['explicit_inputs'][key]['path'] for key in ('metadata','snapshot'));m.same(len(set(paths)),39,'actual exact optional39 source inventory');return {path:indexed[path] for path in set(paths)}

def compare_final26(m:Any,envelope_path:Path)->dict[str,Any]:
    """Dispatch all26 only after the single-current-source/image gate succeeds."""
    before=m.sha(envelope_path);e=m.read(envelope_path);m.same(set(e),ENVELOPE_FIELDS,'closed final optional admission envelope');m.same(e['schema_version'],1,'optional final admission schema1');m.same(e['profile'],'optional-payment-v1','explicit final optional profile');epoch=current_epoch(m,file(m,e['source_acceptance'],'named root source acceptance'));a=epoch['acceptance'];manifest=primary_authorities(m,e['primary']);manifest['capture_source_rows']=capture_sources(m,epoch);cases=m.unique(e['native_cases'],'case_id','final native26');expected=set(m.OPTIONAL_CASES);m.same(set(cases),expected,'exact final26 native case inventory')
    run=Path(e['reference_run']);commands=m.validate_reference_run(run);m.same(set(commands['selected']),expected,'actual all26 final Forge selected cases');m.validate_mandatory_process(run,commands,m.read(run/'results.json'),m.read(run/'source-stability.json'),m.read(run/'authority-binding.json'));reference={str(Path(r['path']).resolve()):r['sha256'] for r in a['reference_files']};m.same(m.read(run/'source-stability.json')['original_source_before'],reference,'actual final Forge/accepted helper source epoch');inputs=m.read(run/'inputs.json');source_cases=m.unique(inputs['cases'],'case_id','final source case');reports=[];class_epoch=None
    for case_id in m.OPTIONAL_CASES:
        entry=cases[case_id];m.same(set(entry),{'case_id','execution','before_guard','after_guard'},'closed actual final native case authority');receipt=m.read(file(m,entry['execution'],'actual final native process'));m.same(receipt['case'],case_id,'actual final receipt case');m.same(receipt['exit_code'],0,'actual final native process exit');m.same(receipt['source_stable'],True,'actual final native source stable');m.same(receipt['image_stable'],True,'actual final native image stable');m.same(receipt['image'],a['image'],'all26 actual same current native image');m.same(receipt['build_receipt'],a['build_receipt'],'all26 actual same current build');m.same(receipt['helper'],a['capture_helper'],'actual frozen final capture helper');m.same(receipt['publication'],a['explicit_inputs']['publication'],'actual final publication input');m.same(receipt['command'],[a['image']['path'],'--metadata',a['explicit_inputs']['metadata']['path'],'--snapshot',a['explicit_inputs']['snapshot']['path'],'--case',case_id,'--capture'],'actual final copied-image command')
        guards=[]
        for key in ('before_guard','after_guard'):
            guard=m.read(file(m,entry[key],'actual final native '+key));m.same(set(guard),{'source_rows','image','explicit_inputs','reader_helpers','reference_files','capture_helper'},'closed final native guard');m.same(guard,dict(source_rows=epoch['source_rows'],image=a['image'],explicit_inputs=a['explicit_inputs'],reader_helpers=a['reader_helpers'],reference_files=a['reference_files'],capture_helper=a['capture_helper']),'actual final complete frozen source/input/image/helper guard');guards.append(guard)
        m.same(guards[0],guards[1],'actual complete native before/after equality');native=m.bound_file(receipt['stdout'],'actual final native output');m.bound_file(receipt['stderr'],'actual final native stderr');capture=m.read_optional_capture(native,manifest,case_id);raw=m.read(run/'evidence'/(case_id+'.json'));m.same(raw['inputs_sha256'],m.sha(run/'inputs.json'),'actual final raw/input bytes');m.same(m.optional_wire_epoch(raw),2,'all26 final Forge explicit wire2');m.validate_runtime(raw,run,commands);classes={r['class']:r['sha256'] for r in raw['runtime']['loader_classes']};class_epoch=classes if class_epoch is None else class_epoch;m.same(classes,class_epoch,'all26 same actual loaded class-image epoch');common=m.optional_reduced_common(capture.report['runs'][0],raw,source_cases[case_id],inputs,manifest['compiled']);reports.append(dict(case=case_id,native_ingress=capture.ingress,native_provenance=capture.provenance,common=common))
    # Supporting finite native-only/API/control epochs remain separately named.
    m.same(set(e['supporting_epochs']),{'native_packets','predecessor_api','controls220'},'explicit separate supporting proof epochs')
    for key,value in e['supporting_epochs'].items():file(m,value,'actual separately named supporting epoch '+key)
    source_rows(m,a);observe_source_inventory(m,epoch['source_inventory_owner'],epoch['source_rows']);m.same(m.sha(envelope_path),before,'actual final admission envelope immutable during reader execution')
    return dict(status='ACTUAL_CURRENT_IMAGE_OPTIONAL26_COMMON_PASS',case_count=26,cases=reports,single_current_native_image=a['image'],source_acceptance=e['source_acceptance'],supporting_epochs=e['supporting_epochs'],supporting_epochs_current_source_inheritance=False,source_bindings_partial=True,globally_fully_qualified_cards=0,full_wave_qualification=False,final_image_adopted=False,limits='Current26 ordinary native/Forge common joins only. Native-only/API/220 and final independent source/flow acceptance remain explicit separate epochs and root gates.')
