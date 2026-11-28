"""Qualify actual local possession and private AI ante RPCs in a unique owned realm."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import urllib.request
import uuid
from auth_admission_fixture import qualify_admission
from disposable_supabase import DisposableSupabase, ROOT, private_write
from native_source_reader_acceptance import prepare_source_reader_reservation
from qualification_http import make_opener, request_json, object_json

REPO = ROOT.parent.parent
REV = REPO / 'work/reviews/application-architecture'
TEST = 'controller_ante::connected::owned_private_ai_ante_possession_originals_and_economics'

def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()

def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--attempt', type=int, default=1)
    attempt = parser.parse_args().attempt
    if not 1 <= attempt <= 99:
        parser.error('attempt must be 1..99')
    target = REV / f'p01-ai-ante-caller-connected-v{attempt}.json'
    if target.exists():
        raise RuntimeError('Refusing to overwrite retained result')
    previous = json.loads((REV / 'ai-ante241-foundation113-source-2026-11-13.json').read_text())['sourcePins']
    paths = {REPO / path for path in previous}
    paths.update((ROOT / 'src').rglob('*.rs'))
    paths.update((ROOT / 'tools').glob('*.py'))
    paths.add(Path(__file__).resolve())
    vocab = REPO / 'work/resolution-value-choices/current-s1-packets-v1/creature-vocabulary.input.v1.json'
    paths.add(vocab)
    pins = {str(path.relative_to(REPO)): sha(path) for path in sorted(paths)}
    trees = [ROOT/'src', ROOT/'supabase/migrations', ROOT/'tools', ROOT/'tests']
    def membership() -> dict[str,list[str]]:
        return {str(tree.relative_to(REPO)): sorted(str(p.relative_to(REPO)) for p in tree.rglob('*') if p.is_file() and p.suffix in ('.rs','.py','.sql','.json')) for tree in trees}
    members = membership()
    realm = DisposableSupabase()
    result: dict = {'scope':'owned local SecretStore/private PostgREST/SQL241 controlled admitted Collection metadata; no native card/format/provider deployment qualification','passed':False,'sourcePins':pins,'sourceMembership':members,'sourceStable':False,'environmentCleanupVerified':False,'runtimeStage':str(realm.stage.relative_to(REPO))}
    primary: BaseException | None = None
    try:
        realm.command(['cargo','test','--locked','--manifest-path',str(ROOT/'Cargo.toml'),'--lib','--no-run'], timeout=240,
                      extra_environment={'CARGO_BUILD_JOBS':'1','SPELLWAKE_CREATURE_VOCABULARY_INPUT':str(vocab)})
        realm.__enter__()
        base, secret, public = (realm.status[key] for key in ('API_URL','SECRET_KEY','PUBLISHABLE_KEY'))
        opener = make_opener()
        def auth(path: str, body: dict, admin: bool = True) -> dict:
            status, response = request_json(opener, urllib.request.Request(base+path,headers={'apikey':secret if admin else public,'Content-Type':'application/json'},data=json.dumps(body).encode()),timeout=30)
            if status != 200:
                raise RuntimeError('Owned credential fixture failed')
            return object_json(response)
        email = 'ai-ante-owner@example.invalid'
        generated = auth('/auth/v1/admin/generate_link',{'type':'magiclink','email':email})
        properties = object_json(generated.get('properties',generated))
        signed = auth('/auth/v1/verify',{'email':email,'token':properties['email_otp'],'type':properties['verification_type']},False)
        bearer = signed['access_token']; parts = bearer.split('.')
        claims = object_json(json.loads(base64.urlsafe_b64decode(parts[1]+'='*(-len(parts[1])%4))))
        admission,_ = qualify_admission(claims,realm.sql)
        owner = str(uuid.UUID(realm.sql("SELECT id FROM app.account WHERE auth_subject='"+str(uuid.UUID(claims['sub']))+"'")))
        # Every admitted room has its own controlled card source family. Basic land
        # metadata makes Collection admission inventory-independent; no native claim.
        reservation = prepare_source_reader_reservation(realm,owner,owner,mixed_ai=True,mode='collection')
        foreign = prepare_source_reader_reservation(realm,owner,owner,mixed_ai=True,mode='collection')
        def actor(matched: str) -> dict:
            return object_json(json.loads(realm.sql("SELECT jsonb_build_object('matched',m.id,'seat',seat.id,'grant',g.id,'binding',b.id,'epoch',b.binding_epoch::text,'reference',g.grant_secret_ref,'revision',slot.deck_revision_id,'configuration',room.configuration_id) FROM app.match m JOIN app.match_seat seat ON seat.match_id=m.id AND seat.controller_kind='ai' JOIN app.ai_runner_binding b ON b.match_seat_id=seat.id JOIN app.controller_grant g ON g.binding_id=b.id AND g.binding_epoch=b.binding_epoch AND g.state='active' JOIN app.room_slot slot ON slot.id=b.room_slot_id JOIN app.room room ON room.id=slot.room_id WHERE m.id='"+str(uuid.UUID(matched))+"'")))
        fixture = actor(reservation['matchId']); other = actor(foreign['matchId'])
        for key in ('matched','grant','binding','epoch','reference'):
            fixture['foreign'+key[0].upper()+key[1:]] = other[key]
        before = object_json(json.loads(realm.sql("SELECT jsonb_build_object('mode',mode,'state',state,'game',(SELECT state FROM app.game WHERE id=reserved_game_id),'coins',(SELECT coalesce(sum(quantity),0)::text FROM app.coin_balance WHERE owner_id=(SELECT id FROM app.asset_owner WHERE account_id='"+owner+"'))) FROM app.match WHERE id='"+reservation['matchId']+"'")))
        if before['mode']!='collection' or before['game']!='reserved':
            raise RuntimeError('Actual Collection initial ante reservation required')
        fixture_path=realm.stage/'private-ai-ante-fixture.json'; output=realm.stage/'ai-ante-result.json'
        private_write(fixture_path,json.dumps(fixture).encode())
        realm.command(['cargo','test','--locked','--manifest-path',str(ROOT/'Cargo.toml'),'--lib',TEST,'--','--ignored','--exact','--nocapture'],timeout=180,
            extra_environment={'CARGO_BUILD_JOBS':'1','SPELLWAKE_CREATURE_VOCABULARY_INPUT':str(vocab),'SPELLWAKE_SUPABASE_URL':base,'SPELLWAKE_SUPABASE_SECRET_KEY':secret,'SPELLWAKE_AI_ANTE_FIXTURE':str(fixture_path),'SPELLWAKE_AI_ANTE_RESULT':str(output)})
        inner=object_json(json.loads(output.read_text()))
        if not inner or not all(value is True for value in inner.values()):
            raise RuntimeError('Owned private consumer controls incomplete')
        matched=reservation['matchId']
        counts=object_json(json.loads(realm.sql("SELECT jsonb_build_object('originals',(SELECT count(*) FROM app.command_receipt WHERE scope='ante_offer_v1' AND target_ref=m.id::text),'accepted',(SELECT count(*) FROM app.command_receipt WHERE scope='ante_offer_v1' AND target_ref=m.id::text AND state='accepted'),'rejected',(SELECT count(*) FROM app.command_receipt WHERE scope='ante_offer_v1' AND target_ref=m.id::text AND state='rejected'),'exposures',(SELECT count(*) FROM app.ai_loss_exposure WHERE match_id=m.id),'reserved',(SELECT reserved_coins::text FROM app.ai_loss_exposure WHERE match_id=m.id),'terms',(SELECT terms_revision::text FROM app.ante_commitment WHERE match_id=m.id),'nativeRevision',(SELECT accepted_revision::text FROM app.game WHERE id=m.reserved_game_id),'coins',(SELECT coalesce(sum(quantity),0)::text FROM app.coin_balance WHERE owner_id=(SELECT id FROM app.asset_owner WHERE account_id='"+owner+"')),'game',(SELECT state FROM app.game WHERE id=m.reserved_game_id)) FROM app.match m WHERE m.id='"+matched+"'")))
        expected={'originals':5,'accepted':4,'rejected':1,'exposures':1,'reserved':'300','terms':'4','nativeRevision':'0','coins':before['coins'],'game':'reserved'}
        if counts != expected:
            raise RuntimeError('Canonical ante economics/original/native footprint differs')
        result.update(passed=True,actualConsumer=inner,canonicalCounts=counts,ordinaryAccountAdmitted=admission['replaySameAccount'],sourceFixture='Canonical admitted initial Collection reservation; normalized basic-land/deck/provider/delivery/Ready observations, actual local minted credential and real private PostgREST SQL')
    except BaseException as error:
        primary=error
        result['failure']={'class':type(error).__name__,'reason':getattr(error,'reason','private diagnostics retained')}
    finally:
        try:
            realm.__exit__()
            result['environmentCleanupVerified']=realm.cleanup_verified
        except BaseException as cleanup:
            result['cleanupFailure']={'class':type(cleanup).__name__}
            if primary is None: primary=cleanup
        result['sourceStable']=all(sha(REPO/path)==digest for path,digest in pins.items()) and membership()==members
        result['runtime']=realm.runtime
        result['migrationManifest']=realm.manifest
        private_write(target,json.dumps(result,indent=2).encode())
    print(json.dumps({'passed':result['passed'],'sourceStable':result['sourceStable'],'environmentCleanupVerified':result['environmentCleanupVerified'],'receipt':str(target.relative_to(REPO))}))
    if primary is not None or not all(result[key] for key in ('passed','sourceStable','environmentCleanupVerified')):
        raise RuntimeError('Owned private AI ante qualification failed; see retained private diagnostics') from None

if __name__=='__main__':
    main()
