"""Observe the real current daily job waiting until13UTC, without another fetch.

Requires an already-established valid local slot; owns only its price worker child.
No synthetic source/market and no hosted activation are introduced.
"""
import base64
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import subprocess
from typing import Callable
from test_local_extraction import sql, literal
from test_local_storage import BASE, ROOT


def qualify_live_day(root: Path, stage: Path, namespace: str, cli: Callable[..., dict], stop: Callable) -> None:
    """Keep actual Temporal timer delivery separate from source acquisition proof."""
    if root != ROOT or 'project_id = "spellwake-application-local"' not in (root / 'supabase/config.toml').read_text():
        raise RuntimeError('Unexpected owned local project')
    now=datetime.now(timezone.utc)
    if now.hour!=12:
        raise RuntimeError('Live timer qualification must start within12UTC')
    slot=now.date().isoformat()
    record=json.loads(sql('SELECT row_to_json(source) FROM (SELECT app.read_established_price_slot('+literal(slot)+'::date) AS value) source'))['value']
    if not isinstance(record,dict) or record.get('manifest',{}).get('state')!='valid':
        raise RuntimeError('An existing valid live slot is required; no fetch is authorized by this harness')
    if sql('SELECT (SELECT count(*) FROM app.account)+(SELECT count(*) FROM app.prediction_market)+(SELECT count(*) FROM app.price_observation)')!='0':
        raise RuntimeError('Live source-day qualification requires no application accounts/markets/observations')
    state=json.loads((root/'target/local-supabase-status.json').read_text())
    if state.get('API_URL')!=BASE or not state.get('SECRET_KEY','').startswith('sb_secret_'):
        raise RuntimeError('Unexpected local backend')
    environment=os.environ.copy()
    environment.update(SPELLWAKE_SUPABASE_URL=BASE,SPELLWAKE_SUPABASE_SECRET_KEY=state['SECRET_KEY'],
                       SPELLWAKE_PRICE_BUCKET='spellwake-evidence',SPELLWAKE_PRICE_STAGE=str(root/'target/local-price-stage'),
                       SPELLWAKE_TEMPORAL_ADDRESS='http://127.0.0.1:57233',SPELLWAKE_TEMPORAL_NAMESPACE=namespace)
    environment.pop('SPELLWAKE_TEMPORAL_API_KEY',None)
    path=stage/'live-day-worker.log';path.touch(mode=0o600)
    worker=None
    try:
        with path.open('ab') as log:
            worker=subprocess.Popen([str(root/'target/debug/price_worker')],env=environment,stdout=log,stderr=subprocess.STDOUT)
            result=subprocess.run([str(root/'target/debug/price_workflow'),'run-day',slot],env=environment,capture_output=True,timeout=4000,check=True)
            if json.loads(result.stdout)!= {'status':'success','data':{'manifestId':record['id']}}:
                raise RuntimeError('Actual day delivery returned a different manifest')
            history=cli('workflow','show','--workflow-id','spellwake-price-day-v1:'+slot)
            events=history['events']
            scheduled=[event['activityTaskScheduledEventAttributes']['activityType']['name'] for event in events if 'activityTaskScheduledEventAttributes' in event]
            if scheduled!=['spellwake-price-collect-v1','spellwake-price-extract-v1'] or not any('timerFiredEventAttributes' in event for event in events):
                raise RuntimeError('Actual day timer/activity sequence differs')
            extracted=[event for event in events if event.get('activityTaskScheduledEventAttributes',{}).get('activityType',{}).get('name')=='spellwake-price-extract-v1']
            if len(extracted)!=1 or datetime.fromisoformat(extracted[0]['eventTime'].replace('Z','+00:00')).hour<13:
                raise RuntimeError('Extraction was scheduled before its acceptance window closed')
            encoded=json.dumps(history).encode();secret=state['SECRET_KEY'].encode()
            if secret in encoded or base64.b64encode(secret) in encoded:
                raise RuntimeError('Backend key entered actual day history')
            (stage/'live-day-history.json').write_bytes(encoded)
            print('Live current-day job passed: existing valid manifest, actual13UTC timer, real source activity and zero-market extraction. No new fetch/publication/settlement claim.')
    finally:
        stop(worker)
