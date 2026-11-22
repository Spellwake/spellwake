"""Real HTTP exchange and pack transactions over an explicitly synthetic catalogue.

Fixture funding and qualified product metadata use the same guarded database
shapes as the database tests. This does not qualify physical supply or card rules.
"""
import json
import uuid
from typing import Callable
from human_api_acceptance import Request, Expect
from disposable_supabase import DisposableSupabase
from qualification_http import object_json


def qualify_economy_api(realm: DisposableSupabase, request: Request, expect: Expect,
                        owner_token: str, peer_token: str, owner: str, peer: str,
                        fresh_identity: Callable[[], str]) -> str:
    """Exercise original-session commands, exact recovery and actual asset holds."""
    def post(label: str, body: dict, token: str = owner_token, lookup: bool = False, status: int = 202) -> dict:
        path = '/v1/economy/commands' + ('/disposition' if lookup else '')
        return expect('economy-' + label, request(path, token, body), status)

    def get(label: str, path: str, token: str = owner_token, status: int = 200) -> dict:
        return expect('economy-' + label, request(path, token, None), status)

    def resource(response: dict) -> str:
        if response.get('status') != 'success':
            raise RuntimeError('Economy HTTP expected standard success')
        return str(uuid.UUID(object_json(response['data'])['resourceId']))

    def recover(label: str, body: dict, accepted: dict, token: str = owner_token) -> None:
        before = effects()
        result = post(label, body, token, True, 200)
        if object_json(result['data']) != {'state': 'accepted', 'response': accepted}:
            raise RuntimeError('Economy HTTP recovery changed original accepted terms')
        if effects() != before:
            raise RuntimeError('Economy disposition lookup changed financial effects')

    def literal(value: object) -> str:
        return "'" + json.dumps(value, ensure_ascii=False).replace("'", "''") + "'::jsonb"

    actors = "('" + owner + "','" + peer + "')"

    def effects() -> str:
        """Snapshot exact rows, quantities and stock without clock-dependent fields."""
        return realm.sql("SELECT jsonb_build_object("
            "'inventories',(SELECT jsonb_agg(jsonb_build_array(i.id,i.owned_quantity::text,v.held::text,i.revision::text) ORDER BY i.id) FROM app.inventory_asset i JOIN app.asset_owner o ON o.id=i.owner_id JOIN app.asset_availability v ON v.inventory_asset_id=i.id WHERE o.account_id IN " + actors + "),"
            "'operations',(SELECT jsonb_agg(jsonb_build_array(id,state) ORDER BY id) FROM app.ledger_operation WHERE actor_account_id IN " + actors + "),"
            "'postings',(SELECT jsonb_agg(jsonb_build_array(p.id,p.inventory_asset_id,p.direction,p.quantity::text) ORDER BY p.id) FROM app.ledger_posting p JOIN app.inventory_asset i ON i.id=p.inventory_asset_id JOIN app.asset_owner o ON o.id=i.owner_id WHERE o.account_id IN " + actors + "),"
            "'holds',(SELECT jsonb_agg(jsonb_build_array(h.id,h.state) ORDER BY h.id) FROM app.asset_hold h JOIN app.asset_owner o ON o.id=h.owner_id WHERE o.account_id IN " + actors + "),"
            "'stock',(SELECT jsonb_agg(jsonb_build_array(id,remaining_quantity::text,revision::text) ORDER BY id) FROM app.supply_stock))")

    def inventory_state(actor: str, asset: str) -> dict:
        return object_json(json.loads(realm.sql("SELECT coalesce((SELECT jsonb_build_object('owned',v.owned::text,'held',v.held::text,'available',v.available::text) FROM app.inventory_asset i JOIN app.asset_owner o ON o.id=i.owner_id JOIN app.asset_availability v ON v.inventory_asset_id=i.id WHERE o.account_id='" + actor + "' AND i.asset_definition_id='" + asset + "'),jsonb_build_object('owned','0','held','0','available','0'))")))

    def inventory_http(label: str, actor: str, token: str, asset: str, expected: dict[str, str], wallet: bool = False) -> None:
        """Compare actual caller rows with known command effects and independent row identity/revision."""
        items: list[dict] = []
        cursor: str | None = None
        seen: set[str] = set()
        for page_index in range(100):
            path = '/v1/inventory?limit=100' + ('' if cursor is None else '&after=' + cursor)
            page = object_json(get('inventory-' + label + '-page-' + str(page_index), path, token)['data'])
            if set(page) != {'items', 'nextCursor'} or not isinstance(page['items'], list):
                raise RuntimeError('Inventory HTTP introduced an owner selector or changed its page envelope')
            items.extend(object_json(item) for item in page['items'])
            cursor = page['nextCursor']
            if cursor is None:
                break
            cursor = str(uuid.UUID(cursor))
            if cursor in seen:
                raise RuntimeError('Inventory HTTP cursor repeated')
            seen.add(cursor)
        else:
            raise RuntimeError('Inventory HTTP fixture pagination did not terminate')
        ids = [item['id'] for item in items]
        if len(ids) != len(set(ids)):
            raise RuntimeError('Inventory HTTP duplicated a row across pages')
        matching = [item for item in items if item['assetId'] == asset]
        if expected['owned'] == '0':
            if matching:
                raise RuntimeError('Inventory HTTP exposed an absent or zero-owned asset')
        else:
            if len(matching) != 1:
                raise RuntimeError('Inventory HTTP omitted the exact owned asset')
            item = matching[0]
            identity = object_json(json.loads(realm.sql("SELECT jsonb_build_object('id',i.id,'revision',i.revision::text) FROM app.inventory_asset i JOIN app.asset_owner o ON o.id=i.owner_id WHERE o.account_id='" + actor + "' AND i.asset_definition_id='" + asset + "'")))
            if {key: item[key] for key in ['owned', 'held', 'available']} != expected or item['id'] != identity['id'] or item['revision'] != identity['revision']:
                raise RuntimeError('Inventory HTTP changed exact quantities, row identity or revision')
        if ids:
            # Identity-only independent witness: never derive expected quantities
            # from the same page or let a second principal see the owner's rows.
            array = "ARRAY[" + ','.join("'" + str(uuid.UUID(value)) + "'::uuid" for value in ids) + "]"
            foreign = realm.sql("SELECT count(*) FROM app.inventory_asset i JOIN app.asset_owner o ON o.id=i.owner_id WHERE i.id=ANY(" + array + ") AND o.account_id IS DISTINCT FROM '" + actor + "'::uuid")
            if foreign != '0':
                raise RuntimeError('Inventory HTTP crossed its authenticated owner boundary')
        if wallet:
            account = object_json(get('wallet-' + label, '/v1/account', token)['data'])
            if account['accountId'] != actor or object_json(account['wallet']) != expected:
                raise RuntimeError('Account HTTP wallet disagreed with exact held inventory')

    coin = str(uuid.UUID(realm.sql("SELECT id FROM app.asset_definition WHERE currency_key='coin'")))
    # Test-only committed postings supply an explicit balance for each actor;
    # neither HTTP commands nor an end-user credential can mint these fixtures.
    for actor in [owner, peer]:
        operation = str(uuid.uuid4())
        realm.sql("BEGIN; INSERT INTO app.ledger_operation(id,kind,source_ref,source_scope,actor_account_id,created_at,state) VALUES('" + operation + "','test','" + operation + "','test','" + actor + "',clock_timestamp(),'pending'); "
                  "INSERT INTO app.ledger_posting(operation_id,inventory_asset_id,direction,quantity,source_kind,source_ref) SELECT '" + operation + "',app.ensure_inventory(o.id,'" + coin + "'),'credit',14500,'test','" + operation + "' FROM app.asset_owner o WHERE o.account_id='" + actor + "'; "
                  "UPDATE app.ledger_operation SET state='committed' WHERE id='" + operation + "'; COMMIT;")

    def held(actor: str) -> int:
        return int(inventory_state(actor, coin)['held'])

    outsider_token = fresh_identity()
    expect('economy-register-outsider', request('/v1/account', outsider_token,
           {'username': 'Out' + uuid.uuid4().hex[:16]}), 200)
    bundle = [{'assetId': coin, 'quantity': '10'}]
    initial_coin = inventory_state(owner, coin)
    initial_hold = int(initial_coin['held'])
    inventory_http('coin-before-trade', owner, owner_token, coin, initial_coin, True)
    original = {'kind': 'trade_create', 'command': {'requestId': str(uuid.uuid4()), 'recipientId': peer, 'give': bundle, 'request': bundle}}
    accepted = post('trade-create', original)
    trade = resource(accepted)
    if held(owner) != initial_hold + 10:
        raise RuntimeError('Actual trade did not hold the sender assets')
    inventory_http('coin-held-trade', owner, owner_token, coin, dict(initial_coin, held=str(initial_hold + 10), available=str(int(initial_coin['available']) - 10)), True)
    recover('trade-create-recover', original, accepted)
    for token, actor in [(owner_token, owner), (peer_token, peer)]:
        detail = object_json(get('trade-participant', '/v1/trades/' + trade, token)['data'])
        if detail['accountId'] != actor or object_json(detail['trade'])['id'] != trade:
            raise RuntimeError('Trade detail changed its caller or exact resource')
    get('trade-existing-foreign-private', '/v1/trades/' + trade, outsider_token, 404)
    altered = dict(original, command=dict(original['command'], give=[{'assetId': coin, 'quantity': '9'}]))
    post('trade-lookup-changed-bundle', altered, lookup=True, status=409)
    if held(owner) != initial_hold + 10:
        raise RuntimeError('Conflicting lookup changed a held trade')
    response = {'kind': 'trade_response', 'command': {'requestId': str(uuid.uuid4()), 'tradeId': trade, 'action': 'accept'}}
    completed = post('trade-accept', response, peer_token)
    recover('trade-accept-recover', response, completed, peer_token)
    settled_effects = effects()
    if post('trade-accept-repeat', response, peer_token) != completed or held(owner) != initial_hold or effects() != settled_effects:
        raise RuntimeError('Trade replay transferred assets or retained the completed hold')
    inventory_http('coin-after-trade', owner, owner_token, coin, initial_coin, True)
    detail = object_json(get('trade-accepted-detail', '/v1/trades/' + trade)['data'])
    if object_json(detail['trade'])['state'] != 'accepted' or object_json(detail['trade'])['settlement'] is None:
        raise RuntimeError('Accepted trade lost its recorded settlement')
    for action, token in [('reject', peer_token), ('cancel', owner_token)]:
        offer = dict(original, command=dict(original['command'], requestId=str(uuid.uuid4())))
        offered = post('trade-' + action + '-offer', offer)
        response = {'kind': 'trade_response', 'command': {'requestId': str(uuid.uuid4()), 'tradeId': resource(offered), 'action': action}}
        closed = post('trade-' + action, response, token)
        recover('trade-' + action + '-recover', response, closed, token)
        if held(owner) != initial_hold:
            raise RuntimeError('Closed trade did not release original held assets')

    # One deterministic synthetic slot lets the actual pack open prove its exact
    # output without treating fixture qualification as real-world source evidence.
    set_id, product, recipe, slot, definition, printing, card = [str(uuid.uuid4()) for _ in range(7)]
    evidence = 'sha256:' + 'a' * 64
    realm.sql("INSERT INTO app.card_set(id,external_set_ref,name) VALUES('" + set_id + "','" + set_id + "','HTTP synthetic set'); "
              "INSERT INTO app.booster_product(id,set_id,name,external_product_ref) VALUES('" + product + "','" + set_id + "','HTTP synthetic pack','" + product + "'); "
              "INSERT INTO app.booster_recipe(id,product_id,revision_ref,evidence_ref) VALUES('" + recipe + "','" + product + "','fixture-v1','test-only:recipe'); "
              "INSERT INTO app.booster_slot(id,recipe_id,slot_key,draws) VALUES('" + slot + "','" + recipe + "','fixture',1); "
              "INSERT INTO app.card_definition(id,oracle_ref,name) VALUES('" + definition + "','" + definition + "','HTTP synthetic card'); "
              "INSERT INTO app.printing(id,definition_id,set_id,external_printing_ref,rarity,finish,image_ref) VALUES('" + printing + "','" + definition + "','" + set_id + "','" + printing + "','rare','nonfoil','test-only:art'); "
              "INSERT INTO app.booster_slot_card(slot_id,printing_id,weight) VALUES('" + slot + "','" + printing + "',1); "
              "INSERT INTO app.asset_definition(id,kind,printing_id) VALUES('" + card + "','card','" + printing + "'); "
              "SELECT app.qualify_booster_recipe('" + recipe + "','" + evidence + "');")
    plan = literal({'version': 'pack-opening-v1', 'groups': []})
    realm.sql("SELECT app.qualify_booster_opening('" + recipe + "'," + plan + ",'sha256:'||encode(sha256(convert_to(" + plan + "::text,'UTF8')),'hex'))")
    realm.sql("SELECT app.publish_supply_window('" + recipe + "',10,10,1,1,'" + evidence + "','" + str(uuid.uuid4()) + "','baseline',NULL,'" + evidence + "',NULL)")
    quote = object_json(object_json(get('pack-quote', '/v1/packs/products/' + recipe + '/quote?quantity=1')['data'])['quote'])
    buy = {'kind': 'pack_buy', 'command': {'requestId': str(uuid.uuid4()), 'recipeId': recipe, 'quantity': '1',
           'expectedPolicyId': quote['policyId'], 'expectedRevision': quote['revision'], 'expectedTotal': quote['total']}}
    coins_before_pack = int(inventory_state(owner, coin)['owned'])
    purchased = post('pack-buy', buy)
    recover('pack-buy-recover', buy, purchased)
    purchased_effects = effects()
    if post('pack-buy-repeat', buy) != purchased or effects() != purchased_effects:
        raise RuntimeError('Pack purchase repeated stock, minting or coin effects')
    if int(inventory_state(owner, coin)['owned']) != coins_before_pack - int(quote['total']):
        raise RuntimeError('Actual pack purchase debited the wrong coin quantity')
    if realm.sql("SELECT remaining_quantity::text FROM app.supply_stock WHERE recipe_id='" + recipe + "'") != '9':
        raise RuntimeError('Actual pack purchase did not consume exactly one shop pack')
    inventory = str(uuid.UUID(realm.sql("SELECT i.id FROM app.inventory_asset i JOIN app.asset_definition d ON d.id=i.asset_definition_id JOIN app.asset_owner o ON o.id=i.owner_id WHERE o.account_id='" + owner + "' AND d.recipe_id='" + recipe + "'")))
    opening = {'kind': 'pack_open', 'command': {'requestId': str(uuid.uuid4()), 'inventoryAssetId': inventory}}
    opened = post('pack-open', opening)
    recover('pack-open-recover', opening, opened)
    opened_effects = effects()
    if post('pack-open-repeat', opening) != opened or effects() != opened_effects:
        raise RuntimeError('Pack replay minted cards or consumed another owned pack')
    if realm.sql("SELECT owned_quantity::text FROM app.inventory_asset WHERE id='" + inventory + "'") != '0' or inventory_state(owner, card) != {'owned': '1', 'held': '0', 'available': '1'}:
        raise RuntimeError('Actual opening did not consume one pack for one card')
    outcome = object_json(get('pack-owned-outcome', '/v1/packs/openings/' + resource(opened))['data'])
    if outcome['accountId'] != owner or object_json(outcome['outcome'])['contents'] != [{'printingId': printing, 'quantity': '1'}]:
        raise RuntimeError('Actual pack opening lost exact deterministic contents')
    get('pack-existing-foreign-private', '/v1/packs/openings/' + resource(opened), peer_token, 404)

    inventory_http('card-after-open', owner, owner_token, card, {'owned':'1','held':'0','available':'1'})
    lot = {'assetId': card, 'quantity': '1'}
    assessed = object_json(json.loads(realm.sql("SELECT app.assess_bundle(" + literal([lot]) + ")")))
    total = object_json(assessed['total'])
    price = str((int(total['numerator']) + int(total['denominator']) - 1) // int(total['denominator']))
    listing = {'kind': 'listing_create', 'command': {'requestId': str(uuid.uuid4()), 'lot': lot, 'totalPrice': price}}
    listed = post('listing-create', listing)
    recover('listing-create-recover', listing, listed)
    if inventory_state(owner, card) != {'owned': '1', 'held': '1', 'available': '0'}:
        raise RuntimeError('Actual public listing did not hold its entire lot')
    inventory_http('card-held-listing', owner, owner_token, card, {'owned':'1','held':'1','available':'0'})
    inventory_http('foreign-card-before-sale', peer, peer_token, card, {'owned':'0','held':'0','available':'0'})
    listing_id = resource(listed)
    detail = object_json(get('listing-public-detail', '/v1/market/listings/' + listing_id, peer_token)['data'])
    record = object_json(detail['listing'])
    purchase = {'kind': 'listing_buy', 'command': {'requestId': str(uuid.uuid4()), 'listingId': listing_id,
                'expectedRevision': record['revision'], 'expectedTotal': price}}
    seller_coins = int(inventory_state(owner, coin)['owned'])
    buyer_coins = int(inventory_state(peer, coin)['owned'])
    bought = post('listing-buy', purchase, peer_token)
    recover('listing-buy-recover', purchase, bought, peer_token)
    sale_effects = effects()
    if post('listing-buy-repeat', purchase, peer_token) != bought or effects() != sale_effects:
        raise RuntimeError('Public sale replay changed accepted whole-lot terms')
    if int(inventory_state(owner, coin)['owned']) != seller_coins + int(price) or int(inventory_state(peer, coin)['owned']) != buyer_coins - int(price):
        raise RuntimeError('Actual public sale changed the sealed coin price')
    if inventory_state(owner, card)['owned'] != '0' or inventory_state(peer, card) != {'owned': '1', 'held': '0', 'available': '1'}:
        raise RuntimeError('Actual sale did not move exactly one complete card lot')
    inventory_http('seller-card-after-sale', owner, owner_token, card, {'owned':'0','held':'0','available':'0'})
    inventory_http('buyer-card-after-sale', peer, peer_token, card, {'owned':'1','held':'0','available':'1'})
    seller_wallet = {'owned':str(seller_coins + int(price)),'held':inventory_state(owner, coin)['held'],'available':str(seller_coins + int(price) - held(owner))}
    buyer_wallet = {'owned':str(buyer_coins - int(price)),'held':inventory_state(peer, coin)['held'],'available':str(buyer_coins - int(price) - held(peer))}
    inventory_http('seller-coin-after-sale', owner, owner_token, coin, seller_wallet, True)
    inventory_http('buyer-coin-after-sale', peer, peer_token, coin, buyer_wallet, True)
    if realm.sql("SELECT state FROM app.market_listing WHERE id='" + listing_id + "'") != 'sold':
        raise RuntimeError('Public listing did not record its terminal sale')
    other_listing = dict(listing, command=dict(listing['command'], requestId=str(uuid.uuid4())))
    offered = post('listing-cancel-offer', other_listing, peer_token)
    cancellation = {'kind': 'listing_cancel', 'command': {'requestId': str(uuid.uuid4()), 'listingId': resource(offered)}}
    inventory_http('peer-card-held-before-cancel', peer, peer_token, card, {'owned':'1','held':'1','available':'0'})
    canceled = post('listing-cancel', cancellation, peer_token)
    recover('listing-cancel-recover', cancellation, canceled, peer_token)
    inventory_http('card-after-cancel', peer, peer_token, card, {'owned':'1','held':'0','available':'1'})
    if inventory_state(peer, card) != {'owned': '1', 'held': '0', 'available': '1'} or realm.sql("SELECT state FROM app.market_listing WHERE id='" + resource(offered) + "'") != 'canceled':
        raise RuntimeError('Listing cancellation did not release its exact original card lot')
    return printing
