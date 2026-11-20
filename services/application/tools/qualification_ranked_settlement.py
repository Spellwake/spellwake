"""Financial assertions for a native multiplayer match in the owned qualifier realm."""
import json
from uuid import UUID, uuid4

from disposable_supabase import DisposableSupabase


def accepted(realm: DisposableSupabase, query: str) -> dict:
    reply = json.loads(realm.sql(query))
    if reply['status'] != 'success':
        raise RuntimeError(f"Terminal fixture operation failed: {reply}")
    return reply['data']


def offer_stakes(realm: DisposableSupabase, reservation: dict) -> dict:
    match = UUID(reservation['matchId'])
    seats = [UUID(s) for s in reservation['seatIds']]
    realm.sql("""
        INSERT INTO app.asset_definition(kind,currency_key) VALUES('coin','coin') ON CONFLICT(currency_key) DO NOTHING;
        INSERT INTO app.season(year,quarter,start_at,end_at,policy_ref,state)
        SELECT extract(year FROM q)::int,extract(quarter FROM q)::int,q AT TIME ZONE 'UTC',
               (q+interval '3 months') AT TIME ZONE 'UTC','fixture:ranked-collection','active'
        FROM (SELECT date_trunc('quarter',current_timestamp AT TIME ZONE 'UTC') q) t
        ON CONFLICT DO NOTHING
    """)
    coin = UUID(realm.sql("SELECT id FROM app.asset_definition WHERE currency_key='coin'"))
    balances = []
    for index, seat in enumerate(seats):
        account = UUID(realm.sql(f"SELECT account_id FROM app.match_seat WHERE id='{seat}' AND match_id='{match}'"))
        operation = uuid4()
        realm.sql(f"""BEGIN;
            INSERT INTO app.ledger_operation(id,kind,source_ref,source_scope,actor_account_id,created_at,state)
              VALUES('{operation}','fixture','{operation}','fixture','{account}',clock_timestamp(),'pending');
            INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref)
              SELECT '{operation}',app.ensure_inventory(id,'{coin}'),'credit',1000,'fixture','{operation}'
              FROM app.asset_owner WHERE account_id='{account}' AND state='active';
            UPDATE app.ledger_operation SET state='committed' WHERE id='{operation}'; COMMIT""")
        balances.append(int(realm.sql(f"""SELECT i.owned_quantity FROM app.inventory_asset i
            JOIN app.asset_owner o ON o.id=i.owner_id WHERE o.account_id='{account}' AND i.asset_definition_id='{coin}'""")))
        lines = json.dumps([{'assetId': str(coin), 'quantity': '100'}])
        offered = accepted(realm, f"SELECT app.set_ante_stake('{match}','{seat}',{index},'{lines}',NULL,'{uuid4()}')")
    commitment = UUID(offered['resourceId'])
    terms_hash = offered['termsHash']
    if len(terms_hash) != 64 or any(c not in '0123456789abcdef' for c in terms_hash):
        raise ValueError('Invalid retained ante terms hash')
    for seat in seats:
        ack = accepted(realm, f"SELECT app.accept_ante_terms('{commitment}','{seat}',{len(seats)},'{terms_hash}','{uuid4()}')")
    if ack['state'] != 'committed':
        raise RuntimeError('Expected all original seats to accept the same stake terms')
    return {'id':str(commitment),'openingBalances':balances}


def check_settlement(realm: DisposableSupabase, reservation: dict, commitment: dict) -> dict:
    match = UUID(reservation['matchId'])
    game = UUID(reservation['gameId'])
    result = UUID(realm.sql(f"SELECT id FROM app.match_result WHERE match_id='{match}' AND final_game_ids=ARRAY['{game}'::uuid]"))
    settle = f"SELECT app.settle_ante('{UUID(commitment['id'])}','{result}')"
    rank = f"SELECT app.consume_ranked_match_result('{result}')"
    ante = accepted(realm, settle)
    ratings = accepted(realm, rank)
    if ante != accepted(realm, settle) or ratings != accepted(realm, rank):
        raise RuntimeError('Terminal retry changed its disposition')
    rows = json.loads(realm.sql(f"""
        SELECT json_agg(json_build_object('seat',s.seat_index,'win',c.win,'delta',c.rating_delta,
                 'rating',c.rating_after,'coins',i.owned_quantity,'utcReward',coalesce(d.issued_coins,0),
                 'sameTimestamp',c.completed_at=r.committed_at) ORDER BY s.seat_index)
        FROM app.match_result r JOIN app.match_seat s ON s.match_id=r.match_id
        JOIN app.competitive_result c ON c.match_result_id=r.id AND c.account_id=s.account_id
        JOIN app.asset_owner o ON o.account_id=s.account_id AND o.state='active'
        JOIN app.asset_definition a ON a.currency_key='coin'
        JOIN app.inventory_asset i ON i.owner_id=o.id AND i.asset_definition_id=a.id
        LEFT JOIN app.daily_win_bucket d ON d.account_id=s.account_id AND d.utc_day=to_char(r.committed_at AT TIME ZONE 'UTC','YYYY-MM-DD')
        WHERE r.id='{result}'"""))
    expected = [(-8, -100, 0), (16, 200, 250), (-8, -100, 0)]
    if len(rows) != len(expected):
        raise RuntimeError('Expected one competitive result per original seat')
    for seat, (row, (delta, transfer, reward)) in enumerate(zip(rows, expected, strict=True)):
        balance = commitment['openingBalances'][seat] + transfer + reward
        if row != {'seat': seat, 'win': seat == 1, 'delta': delta, 'rating': 1000 + delta,
                   'coins': balance, 'utcReward': reward, 'sameTimestamp': True}:
            raise RuntimeError(f'Incorrect three-seat rank or reward attribution for seat {seat}')
    monthly = check_monthly_activity(realm, game, [0, 0, 0])
    return {'monthly':monthly,'matchResult':str(result),'ante':ante,'rankedResult':ratings,'credits':rows}


def check_monthly_activity(realm: DisposableSupabase, game: UUID, expected_turns: list[int]) -> list[dict]:
    """Consume original participation twice and verify database attribution for every seat."""
    consume = f"""SELECT json_agg(app.record_monthly_game_activity(p.id) ORDER BY s.seat_index)
        FROM app.game_participation p JOIN app.match_seat s ON s.id=p.match_seat_id
        WHERE p.game_id='{game}'"""
    first = json.loads(realm.sql(consume))
    if first != json.loads(realm.sql(consume)):
        raise RuntimeError('Monthly participation retry changed its original account credit')
    rows = json.loads(realm.sql(f"""SELECT json_agg(json_build_object(
        'turns',p.own_turns,'qualifies',p.real_game,'credit',a.id,'sameMonth',
        a.utc_month=to_char(p.completed_at AT TIME ZONE 'UTC','YYYY-MM'),
        'sameParticipant',a.source_ref=p.id::text) ORDER BY s.seat_index)
        FROM app.game_participation p JOIN app.match_seat s ON s.id=p.match_seat_id
        LEFT JOIN app.monthly_account_activity a ON a.account_id=p.account_id
        AND a.activity_policy_ref='monthly-real-game-v1'
        WHERE p.game_id='{game}'"""))
    for row, credit, turns in zip(rows, first, expected_turns, strict=True):
        qualifies = turns >= 3
        if (credit is not None) != qualifies or row != {
            'turns': turns, 'qualifies': qualifies, 'credit': credit,
            'sameMonth': True if qualifies else None,
            'sameParticipant': True if qualifies else None,
        }:
            raise RuntimeError('Monthly activity diverged from the original game participation')
    return rows


def check_no_contest_refund(realm: DisposableSupabase, reservation: dict, commitment: dict) -> dict:
    """Interrupt the opened game, refund the three original stakes, and drain its rank result."""
    game = UUID(reservation['gameId'])
    match = UUID(reservation['matchId'])
    stake = UUID(commitment['id'])
    incident = uuid4()
    accepted(realm, f"""SELECT app.pause_game_incident('{game}','fixture:source-reader',1,0,
        clock_timestamp(),'storage','fixture:ranked-incident','{incident}')""")
    basis = json.loads(realm.sql(f"""SELECT json_build_array(revision,availability_revision,incident_receipt_id)
        FROM app.match WHERE id='{match}' AND availability='recovering'"""))
    terminal = f"""SELECT app.finalize_unrecoverable_match('{match}',{int(basis[0])},{int(basis[1])},
        '{UUID(basis[2])}','fixture:ranked-restoration-failed')"""
    closed = accepted(realm, terminal)
    result = UUID(closed['terminalResultId'])
    refund = f"SELECT app.settle_ante('{stake}','{result}')"
    rank = f"SELECT app.consume_ranked_match_result('{result}')"
    settlement = accepted(realm, refund)
    ranked = accepted(realm, rank)
    if (closed, settlement, ranked) != tuple(accepted(realm, query) for query in (terminal, refund, rank)):
        raise RuntimeError('An original no-contest acknowledgment changed on retry')
    if settlement.get('state') != 'refunded':
        raise RuntimeError('The incident did not refund the committed stakes')
    rows = json.loads(realm.sql(f"""SELECT json_agg(json_build_object(
        'seat',s.seat_index,'coins',i.owned_quantity,'hold',h.state,'delta',c.rating_delta,
        'win',c.win,'rating',c.rating_after,'reward',coalesce(d.issued_coins,0))
        ORDER BY s.seat_index)
        FROM app.match_seat s JOIN app.ante_stake a ON a.match_seat_id=s.id AND a.commitment_id='{stake}'
        JOIN app.asset_hold h ON h.id=a.hold_id JOIN app.asset_owner o ON o.account_id=s.account_id
        JOIN app.asset_definition coin ON coin.currency_key='coin'
        JOIN app.inventory_asset i ON i.owner_id=o.id AND i.asset_definition_id=coin.id
        JOIN app.competitive_result c ON c.account_id=s.account_id AND c.match_result_id='{result}'
        LEFT JOIN app.daily_win_bucket d ON d.account_id=s.account_id AND d.utc_day=to_char(c.completed_at AT TIME ZONE 'UTC','YYYY-MM-DD')
        WHERE s.match_id='{match}'"""))
    expected = [dict(seat=index, coins=balance, hold='released', delta=0, win=False, rating=1000, reward=0)
                for index, balance in enumerate(commitment['openingBalances'])]
    if len(expected) != 3 or rows != expected:
        raise RuntimeError('Incorrect no-contest balance, hold, rank or daily reward')
    effect = json.loads(realm.sql(f"""SELECT json_build_object(
        'frontier',r.rank_sequence=f.last_ranked_sequence,'outcome',r.outcome,'cause',r.terminal_kind,
        'openedGame',r.final_game_ids=ARRAY['{game}'::uuid],'winnerCount',cardinality(r.winner_seat_ids),
        'gameResults',(SELECT count(*) FROM app.game_result WHERE game_id='{game}'),
        'participations',(SELECT count(*) FROM app.game_participation WHERE game_id='{game}'),
        'refundPostings',(SELECT count(*) FROM app.ledger_posting WHERE operation_id='{UUID(settlement["operationId"])}'))
        FROM app.match_result r JOIN app.format f ON f.id=r.ranking_format_id WHERE r.id='{result}'"""))
    if effect != dict(frontier=True,outcome='no_contest',cause='unrecoverable_incident',openedGame=True,
                      winnerCount=0,gameResults=0,participations=0,refundPostings=0):
        raise RuntimeError('Incorrect rank frontier or extra incident settlement effects')
    return dict(terminal=closed, refund=settlement, ranked=ranked, accounts=rows, effects=effect, retryVerified=True)
