"""Managed HTTP benefit projections over explicit controlled payment/membership inputs.

Stripe changes use the maintained signed loopback provider. Membership rows are
normalized test fixtures, not actual Patreon exchange or financial evidence.
No fixture cosmetic reference qualifies approved/loadable production artwork.
"""
import uuid
from typing import Callable
from disposable_supabase import DisposableSupabase
from human_api_acceptance import Request, Expect, Fixture
from qualification_http import object_json

PaymentState = Callable[[str, str, str], None]


def qualify_benefit_projections(realm: DisposableSupabase, request: Request, expect: Expect,
                                owner_token: str, peer_token: str, owner: str, peer: str,
                                cosmetic: str, first_purchase: str, fixture: Fixture,
                                payment_state: PaymentState) -> None:
    """Prove badge currentness and source-specific entitlement fallback through real HTTP."""
    def get(label: str, path: str, token: str = owner_token) -> dict:
        response = expect('benefit-' + label, request(path, token, None), 200)
        if response.get('status') != 'success':
            raise RuntimeError('Benefit projection expected standard success')
        return object_json(response['data'])

    def post(label: str, path: str, body: dict, status: int = 202) -> dict:
        return expect('benefit-' + label, request(path, owner_token, body), status)

    def resource(response: dict) -> str:
        return str(uuid.UUID(object_json(response['data'])['resourceId']))

    def owned(label: str, expected: bool) -> None:
        page = get(label, '/v1/cosmetics?limit=100')
        choices = [object_json(item) for item in page['items'] if object_json(item)['id'] == cosmetic]
        if len(choices) != 1 or choices[0]['isOwned'] is not expected or choices[0]['isFree'] is not False:
            raise RuntimeError('Cosmetic HTTP lost the exact paid entitlement state')

    def badge(label: str, expected: bool) -> None:
        own = get(label + '-own', '/v1/account/profile')
        public = get(label + '-public', '/v1/players/' + owner + '/profile', peer_token)
        if own['accountId'] != owner or own['supporter'] is not expected or public['viewerAccountId'] != peer or object_json(public['person'])['id'] != owner or public['supporter'] is not expected:
            raise RuntimeError('Profile HTTP badge disagreed with current membership or caller context')
        if set(public) != {'viewerAccountId', 'person', 'cardArtProfile', 'supporter'}:
            raise RuntimeError('Public profile exposed private benefit source fields')

    badge('badge-before-membership', False)
    tier, event, membership = [str(uuid.uuid4()) for _ in range(3)]
    # Same deliberately normalized source seam as the isolated cosmetics tests.
    # This proves current projection, not Patreon OAuth/payment acquisition.
    realm.sql("INSERT INTO app.supporter_tier(id,patreon_tier_ref,name,rank) VALUES('" + tier + "','" + tier + "','Normalized HTTP badge fixture',(SELECT coalesce(max(rank),0)+1 FROM app.supporter_tier)); "
              "INSERT INTO app.provider_event(id,provider,external_event_ref,account_id,received_at,event_type,payload_ref,processing_state) VALUES('" + event + "','patreon','" + event + "','" + owner + "',clock_timestamp(),'test-only:normalized-membership','test-only:evidence','verified'); "
              "INSERT INTO app.patreon_membership(id,account_id,patreon_member_ref,active_tier_id,active,source_event_id) VALUES('" + membership + "','" + owner + "','" + membership + "','" + tier + "',true,'" + event + "')")
    badge('badge-current-membership', True)
    owned('paid-cosmetic-with-membership', True)
    realm.sql("UPDATE app.patreon_membership SET active=false,active_tier_id=NULL WHERE id='" + membership + "'")
    badge('badge-cancelled-membership', False)
    owned('paid-cosmetic-after-membership', True)

    # Two actual controlled purchase sources fund the same item. One reversal
    # cannot revoke another source; loss of the final source must resolve to free.
    second = resource(post('second-purchase', '/v1/purchases', {'requestId':str(uuid.uuid4()), 'cosmeticId':cosmetic}))
    payment_state(second, 'paid', 'second-paid')
    if realm.sql("SELECT count(*) FROM app.cosmetic_grant WHERE account_id='" + owner + "' AND cosmetic_id='" + cosmetic + "' AND state='active'") != '2':
        raise RuntimeError('Controlled purchases did not establish two independent grants')
    account = get('before-selection', '/v1/account')
    free = object_json(account['defaults'])
    selected = dict(free, backId=cosmetic)
    selection = {'requestId':str(uuid.uuid4()), 'expected':free, 'appearance':selected}
    selected_ack = post('select-defaults', '/v1/account/defaults', selection)
    deck = resource(post('create-selected-deck', '/v1/decks', {'requestId':str(uuid.uuid4()),'name':'HTTP entitlement witness','mode':'open','formatId':fixture['format_id']}))
    detail_path = '/v1/decks/' + deck
    entry = {'definitionId':fixture['definition'],'printingId':fixture['printing'],'section':'main','quantity':'1'}
    save = {'requestId':str(uuid.uuid4()),'deckId':deck,'expectedRevisionId':None,'entries':[entry]}
    revision = resource(post('seal-selected-deck', detail_path + '/revisions', save))
    initial = get('selected-deck', detail_path)
    initial_revision = object_json(initial['revision'])
    if object_json(initial['deck'])['appearance'] != selected or initial_revision['id'] != revision or initial_revision['entries'] != [entry]:
        raise RuntimeError('Entitlement witness lost selected appearance or immutable contents')

    payment_state(first_purchase, 'refunded', 'first-source-reversed')
    owned('other-source-preserves-owned', True)
    before_last = get('other-source-preserves-defaults', '/v1/account')
    if before_last['defaults'] != selected or object_json(get('other-source-preserves-deck', detail_path)['deck'])['appearance'] != selected:
        raise RuntimeError('Reversing one source erased another valid entitlement selection')

    payment_state(second, 'refunded', 'last-source-reversed')
    owned('last-source-loses-owned', False)
    fallback_account = get('last-source-default-fallback', '/v1/account')
    fallback_detail = get('last-source-deck-fallback', detail_path)
    if fallback_account['defaults'] != free or object_json(fallback_detail['deck'])['appearance'] != free or object_json(fallback_detail['revision']) != initial_revision:
        raise RuntimeError('Last entitlement loss did not fall back freely without rewriting contents')
    if int(fallback_account['appearanceRevision']) <= int(before_last['appearanceRevision']):
        raise RuntimeError('Last-source fallback did not invalidate current appearance')
    if post('original-selection-replay', '/v1/account/defaults', selection) != selected_ack:
        raise RuntimeError('Historical selection replay lost its original acknowledgment')
    recovered = post('original-selection-recovery', '/v1/account/defaults/disposition', selection, 200)
    if object_json(recovered['data']) != {'state':'accepted','response':selected_ack}:
        raise RuntimeError('Historical selection disposition changed after entitlement loss')
    if get('replay-keeps-free-defaults', '/v1/account')['defaults'] != free or object_json(get('replay-keeps-free-deck', detail_path)['deck'])['appearance'] != free:
        raise RuntimeError('Historical acknowledgment silently re-equipped revoked appearance')
    # Verified funds restoration creates a new grant; it does not silently equip
    # the old selection. Leave paid ownership available to the next defaults test.
    payment_state(first_purchase, 'failed_refund', 'first-source-restored')
    owned('restored-source-owned', True)
    restored_detail = get('restoration-keeps-free-deck', detail_path)
    if get('restoration-keeps-free-defaults', '/v1/account')['defaults'] != free or object_json(restored_detail['deck'])['appearance'] != free or object_json(restored_detail['revision']) != initial_revision:
        raise RuntimeError('Restored entitlement rewrote selection or immutable deck revision')
