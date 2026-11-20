//! Controlled RPC decoding witnesses; these do not qualify actual Auth, SQL or providers.
use super::*;
use serde::de::DeserializeOwned;

/// Spell out declaration order: an arbitrary map iteration order is not a valid positional witness.
type ObjectPath<'a> = (&'a str, &'a [&'a str]);

fn projection_matrix<T: DeserializeOwned>(value: Value, paths: &[ObjectPath<'_>]) {
    let mut variants = Vec::new();
    for (path, fields) in paths {
        let mut bad = value.clone();
        let object = bad.pointer_mut(path).expect("fixture object path");
        assert_eq!(
            object.as_object().expect("fixture object").len(),
            fields.len(),
            "{path}"
        );
        let positional = fields
            .iter()
            .map(|name| object.get(name).expect("complete field list").clone())
            .collect();
        *object = Value::Array(positional);
        variants.push(bad);
    }
    let claims = claims();
    let bearer = token(&claims);
    let steps = std::iter::once(value)
        .chain(variants)
        .map(|data| {
            let mut response = step(&bearer, json!({"status":"success","data":data}));
            response.path = "/rest/v1/rpc/read_shape_fixture".into();
            response
        })
        .collect();
    let mock = Mock::new(steps);
    let client = client(&mock.base);
    // Domain validation has its own tests. Here even otherwise parseable arrays must
    // fail the real shared caller projection transport before reaching validation.
    let mut validated = 0;
    for expected_success in std::iter::once(true).chain(paths.iter().map(|_| false)) {
        let result = client.scoped_projection_keys::<T>(
            &bearer,
            "read_shape_fixture",
            &json!({}),
            &[],
            |_| {
                validated += 1;
                Ok(())
            },
        );
        if expected_success {
            assert!(matches!(result, Ok(ApiResponse::Success { .. })));
        } else {
            assert!(matches!(result, Err(error) if error == ActorError::Unavailable));
        }
    }
    assert_eq!(
        validated, 1,
        "only the valid object reaches domain validation"
    );
    assert_eq!(mock.finish().len(), paths.len() + 1);
}

fn fixture(bytes: &str) -> Value {
    serde_json::from_str(bytes).unwrap()
}

fn nullable_objects<T: DeserializeOwned>(value: Value, paths: &[&str]) {
    assert!(serde_json::from_value::<crate::provider_json::Object<T>>(value.clone()).is_ok());
    for path in paths {
        let object = value.pointer(path).unwrap().as_object().unwrap();
        let mut null = value.clone();
        *null.pointer_mut(path).unwrap() = Value::Null;
        assert!(
            serde_json::from_value::<crate::provider_json::Object<T>>(null).is_ok(),
            "null: {path}"
        );
        let (parent, field) = path.rsplit_once('/').unwrap();
        let mut missing = value.clone();
        missing
            .pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(
            serde_json::from_value::<crate::provider_json::Object<T>>(missing).is_err(),
            "missing: {path}"
        );
        let mut unknown = value.clone();
        unknown
            .pointer_mut(path)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unrecognized".into(), json!(true));
        assert!(
            serde_json::from_value::<crate::provider_json::Object<T>>(unknown).is_err(),
            "unknown: {path}"
        );

        let selected = Value::Object(object.clone()).to_string();
        let (key, field_value) = object.iter().next().unwrap();
        let duplicated = format!("{{{}:{},{}", json!(key), field_value, &selected[1..]);
        let raw = value.to_string().replacen(&selected, &duplicated, 1);
        // Keep the duplicate in raw bytes until the real caller projection decoder.
        let claims = claims();
        let bearer = token(&claims);
        let mut response = step(&bearer, Value::Null);
        response.path = "/rest/v1/rpc/read_shape_fixture".into();
        response.body = format!(r#"{{"status":"success","data":{raw}}}"#).into_bytes();
        let mock = Mock::new(vec![response]);
        let result = client(&mock.base).scoped_projection_keys::<T>(
            &bearer,
            "read_shape_fixture",
            &json!({}),
            &[],
            |_| panic!("duplicate reached domain validator"),
        );
        assert!(
            matches!(result, Err(error) if error == ActorError::Unavailable),
            "duplicate: {path}"
        );
        assert_eq!(mock.finish().len(), 1);
    }
}

fn social_person(id: Uuid) -> Value {
    json!({"id":id,"username":"peer_user","isDeleted":false,"relationship":"friend","friendshipId":id,"isBlocked":false,"canContact":true,"conversationId":id,"unreadMessages":"0","supporter":false})
}

#[test]
fn session_and_local_identity_objects_reject_complete_arrays_and_duplicates() {
    let original = claims();
    let bearer = token(&original);
    let owner = Uuid::new_v4();
    let mut positional = success(&original, "active", json!(owner));
    positional["data"] = json!([
        original["sub"],
        original["session_id"],
        "email_code",
        "active",
        owner
    ]);
    let mock = Mock::new(vec![
        step(&bearer, positional),
        step(&bearer, success(&original, "active", json!(owner))),
    ]);
    let client = client(&mock.base);
    assert!(matches!(
        client.resolve(&bearer),
        Err(error) if error == ActorError::Unavailable
    ));
    assert_eq!(client.resolve(&bearer).unwrap().account(), Some(owner));
    assert_eq!(mock.finish().len(), 2);

    let mut root_array = original.clone();
    let root = &original["spellwake_auth"];
    root_array["spellwake_auth"] =
        json!([1, "email_code", root["bindingId"], root["sessionGrantId"]]);
    let claims_array = json!([
        original["sub"],
        original["session_id"],
        ISSUER,
        "authenticated",
        original["exp"],
        "authenticated",
        root
    ]);
    let raw = original.to_string();
    let duplicate_subject =
        raw.replace("\"sub\":", &format!("\"sub\":{},\"sub\":", original["sub"]));
    let duplicate_authority = raw.replace("\"version\":1", "\"version\":1,\"version\":1");
    let header = br#"{"alg":"HS256","typ":"JWT"}"#;
    let encode = |h: &[u8], c: &[u8]| {
        format!(
            "{}.{}.{}",
            URL_SAFE_NO_PAD.encode(h),
            URL_SAFE_NO_PAD.encode(c),
            URL_SAFE_NO_PAD.encode(b"synthetic")
        )
    };
    let invalid = [
        token(&root_array),
        token(&claims_array),
        encode(br#"["HS256"]"#, raw.as_bytes()),
        encode(br#"{"alg":"HS256","alg":"HS256"}"#, raw.as_bytes()),
        encode(header, duplicate_subject.as_bytes()),
        encode(header, duplicate_authority.as_bytes()),
    ];
    let mock = Mock::new(vec![]);
    let client = super::client(&mock.base);
    for bearer in invalid {
        assert!(matches!(
            client.resolve(&bearer),
            Err(ActorError::Unauthorized)
        ));
    }
    assert!(mock.finish().is_empty());
    let mut extensible = original;
    extensible["aud"] = json!(["authenticated"]);
    extensible["app_metadata"] = json!({"provider":"email"});
    assert!(parse_claims(&token(&extensible), ISSUER).is_ok());
}

#[test]
fn owner_wallet_inventory_and_deck_records_require_objects() {
    let id = Uuid::new_v4();
    let appearance = json!({"backId":id,"sleeveId":id,"diceId":id,"playmatId":null});
    projection_matrix::<crate::account::AccountSummary>(
        json!({"accountId":id,"username":"player_one","state":"active","appearanceRevision":"1","defaults":appearance,"wallet":{"owned":"9007199254740993","held":"1","available":"9007199254740992"}}),
        &[
            (
                "",
                &[
                    "accountId",
                    "username",
                    "state",
                    "appearanceRevision",
                    "defaults",
                    "wallet",
                ],
            ),
            ("/defaults", &["backId", "sleeveId", "diceId", "playmatId"]),
            ("/wallet", &["owned", "held", "available"]),
        ],
    );
    let deck = json!({"id":id,"name":"Deck","mode":"collection","currentRevisionId":id,"format":{"id":id,"key":"standard","name":"Standard"},"appearance":appearance});
    projection_matrix::<crate::account::DeckLibraryPage>(
        json!({"items":[deck],"nextCursor":null}),
        &[
            ("", &["items", "nextCursor"]),
            (
                "/items/0",
                &[
                    "id",
                    "name",
                    "mode",
                    "currentRevisionId",
                    "format",
                    "appearance",
                ],
            ),
            ("/items/0/format", &["id", "key", "name"]),
            (
                "/items/0/appearance",
                &["backId", "sleeveId", "diceId", "playmatId"],
            ),
        ],
    );
    projection_matrix::<crate::decks::reads::DeckDetail>(
        json!({"deck":deck,"revision":{"id":id,"number":"1","entries":[{"definitionId":id,"printingId":id,"section":"main","quantity":"1"}]}}),
        &[
            ("", &["deck", "revision"]),
            (
                "/deck",
                &[
                    "id",
                    "name",
                    "mode",
                    "currentRevisionId",
                    "format",
                    "appearance",
                ],
            ),
            ("/revision", &["id", "number", "entries"]),
            (
                "/revision/entries/0",
                &["definitionId", "printingId", "section", "quantity"],
            ),
        ],
    );
    projection_matrix::<crate::inventory::InventoryPage>(
        json!({"items":[{"id":id,"assetId":id,"revision":"1","owned":"2","held":"1","available":"1","asset":{"kind":"coin","currencyKey":"coin"}}],"nextCursor":null}),
        &[
            ("", &["items", "nextCursor"]),
            (
                "/items/0",
                &[
                    "id",
                    "assetId",
                    "revision",
                    "owned",
                    "held",
                    "available",
                    "asset",
                ],
            ),
            ("/items/0/asset", &["kind", "currencyKey"]),
        ],
    );
    projection_matrix::<crate::decks::reads::FormatPage>(
        json!({"items":[{"id":id,"key":"standard","name":"Standard"}],"nextCursor":null}),
        &[
            ("", &["items", "nextCursor"]),
            ("/items/0", &["id", "key", "name"]),
        ],
    );
}

#[test]
fn social_people_messages_and_nullable_cursors_require_objects() {
    let id = Uuid::new_v4();
    let person = social_person(id);
    let fields = &[
        "id",
        "username",
        "isDeleted",
        "relationship",
        "friendshipId",
        "isBlocked",
        "canContact",
        "conversationId",
        "unreadMessages",
        "supporter",
    ];
    projection_matrix::<crate::social::SocialPeoplePage>(
        json!({"accountId":id,"items":[person],"nextCursor":null}),
        &[
            ("", &["accountId", "items", "nextCursor"]),
            ("/items/0", fields),
        ],
    );
    projection_matrix::<crate::social::SocialStatus>(
        json!({"accountId":id,"unreadMessages":"0","personId":id,"person":person}),
        &[
            ("", &["accountId", "unreadMessages", "personId", "person"]),
            ("/person", fields),
        ],
    );
    projection_matrix::<crate::social::DirectMessagePage>(
        json!({"accountId":id,"conversationId":id,"person":person,"items":[{"id":id,"senderId":id,"author":"peer_user","body":"Hello","sentAt":"2026-11-02T12:00:00.000001Z"}],"nextCursor":{"sentAt":"2026-11-02T12:00:00.000001Z","id":id}}),
        &[
            (
                "",
                &[
                    "accountId",
                    "conversationId",
                    "person",
                    "items",
                    "nextCursor",
                ],
            ),
            ("/person", fields),
            ("/items/0", &["id", "senderId", "author", "body", "sentAt"]),
            ("/nextCursor", &["sentAt", "id"]),
        ],
    );
}

#[test]
fn catalogue_and_commerce_pages_require_object_elements() {
    let id = Uuid::new_v4();
    let set = json!({"id":id,"name":"Test set","code":null});
    let card = json!({"id":id,"name":"Test card","oracleRef":id.to_string()});
    let printing = json!({"id":id,"definitionId":id,"externalPrintingRef":id.to_string(),"rarity":"rare","finish":"nonfoil","collectorNumber":null,"language":null,"set":set});
    projection_matrix::<crate::catalogue::CatalogueLabels>(
        json!({"cards":[card],"printings":[printing]}),
        &[
            ("", &["cards", "printings"]),
            ("/cards/0", &["id", "name", "oracleRef"]),
            (
                "/printings/0",
                &[
                    "id",
                    "definitionId",
                    "externalPrintingRef",
                    "rarity",
                    "finish",
                    "collectorNumber",
                    "language",
                    "set",
                ],
            ),
            ("/printings/0/set", &["id", "name", "code"]),
        ],
    );
    projection_matrix::<crate::catalogue::SetPage>(
        json!({"items":[set],"nextCursor":null}),
        &[
            ("", &["items", "nextCursor"]),
            ("/items/0", &["id", "name", "code"]),
        ],
    );
    projection_matrix::<crate::catalogue::CardPage>(
        json!({"items":[card],"nextCursor":null}),
        &[
            ("", &["items", "nextCursor"]),
            ("/items/0", &["id", "name", "oracleRef"]),
        ],
    );
    projection_matrix::<crate::catalogue::PrintingPage>(
        json!({"items":[printing],"nextCursor":null}),
        &[
            ("", &["items", "nextCursor"]),
            (
                "/items/0",
                &[
                    "id",
                    "definitionId",
                    "externalPrintingRef",
                    "rarity",
                    "finish",
                    "collectorNumber",
                    "language",
                    "set",
                ],
            ),
        ],
    );
    projection_matrix::<crate::store::StorePage<crate::store::CosmeticSummary>>(
        json!({"items":[{"id":id,"name":"Sleeve","kind":"sleeve","isFree":false,"isOwned":true,"includedWithPatreon":false}],"nextCursor":null}),
        &[
            ("", &["items", "nextCursor"]),
            (
                "/items/0",
                &[
                    "id",
                    "name",
                    "kind",
                    "isFree",
                    "isOwned",
                    "includedWithPatreon",
                ],
            ),
        ],
    );
    let purchase = json!({"purchaseId":id,"cosmeticId":id,"createdAt":"2026-11-02T12:00:00Z","state":"pending","revision":"1","price":{"currency":"usd","minorUnits":"500"},"checkout":{"status":"ready","url":"https://checkout.stripe.com/test","expiresAt":"2026-11-03T12:00:00Z"}});
    projection_matrix::<crate::stripe::PurchaseView>(
        purchase.clone(),
        &[
            (
                "",
                &[
                    "purchaseId",
                    "cosmeticId",
                    "createdAt",
                    "state",
                    "revision",
                    "price",
                    "checkout",
                ],
            ),
            ("/price", &["currency", "minorUnits"]),
            ("/checkout", &["status", "url", "expiresAt"]),
        ],
    );
    projection_matrix::<crate::store::StorePage<crate::stripe::PurchaseView>>(
        json!({"items":[purchase],"nextCursor":null}),
        &[
            ("", &["items", "nextCursor"]),
            (
                "/items/0",
                &[
                    "purchaseId",
                    "cosmeticId",
                    "createdAt",
                    "state",
                    "revision",
                    "price",
                    "checkout",
                ],
            ),
        ],
    );
}

#[test]
fn exchange_participants_and_exact_settlement_records_require_objects() {
    let f = fixture(include_str!("../../tests/fixtures/exchange-reads.json"));
    let trade_fields = &[
        "id",
        "sender",
        "recipient",
        "revision",
        "state",
        "createdAt",
        "give",
        "request",
        "settlement",
    ];
    let settlement_fields = &[
        "outgoing",
        "incoming",
        "outgoingAllowed",
        "incomingAllowed",
        "senderRestricted",
        "recipientRestricted",
    ];
    projection_matrix::<crate::exchange_reads::OwnedTrade>(
        f["acceptedTrade"].clone(),
        &[
            ("", &["accountId", "trade"]),
            ("/trade", trade_fields),
            ("/trade/sender", &["id", "username", "isDeleted"]),
            ("/trade/recipient", &["id", "username", "isDeleted"]),
            ("/trade/give/0", &["assetId", "quantity", "asset"]),
            ("/trade/request/0", &["assetId", "quantity", "asset"]),
            ("/trade/settlement", settlement_fields),
            (
                "/trade/settlement/outgoing",
                &["policyId", "catalogueRevision", "total"],
            ),
            (
                "/trade/settlement/incoming",
                &["policyId", "catalogueRevision", "total"],
            ),
            (
                "/trade/settlement/outgoing/total",
                &["numerator", "denominator"],
            ),
        ],
    );
    projection_matrix::<crate::exchange_reads::TradePage>(
        json!({"accountId":f["acceptedTrade"]["accountId"],"items":[f["acceptedTrade"]["trade"]],"nextCursor":null}),
        &[
            ("", &["accountId", "items", "nextCursor"]),
            ("/items/0", trade_fields),
        ],
    );
    let listing_fields = &[
        "id",
        "seller",
        "assetId",
        "asset",
        "quantity",
        "totalPrice",
        "state",
        "revision",
        "participant",
    ];
    projection_matrix::<crate::exchange_reads::OwnedListing>(
        f["soldListing"].clone(),
        &[
            ("", &["accountId", "listing"]),
            ("/listing", listing_fields),
            ("/listing/seller", &["id", "username", "isDeleted"]),
            (
                "/listing/participant",
                &["buyer", "paidCoins", "completedAt", "settlement"],
            ),
            (
                "/listing/participant/buyer",
                &["id", "username", "isDeleted"],
            ),
            ("/listing/participant/settlement", settlement_fields),
        ],
    );
    projection_matrix::<crate::exchange_reads::ListingPage>(
        json!({"accountId":f["soldListing"]["accountId"],"items":[f["soldListing"]["listing"]],"nextCursor":null}),
        &[
            ("", &["accountId", "items", "nextCursor"]),
            ("/items/0", listing_fields),
        ],
    );
}

#[test]
fn prediction_source_book_and_position_records_require_objects() {
    let f = fixture(include_str!("../../tests/fixtures/prediction-reads.json"));
    let market_fields = &[
        "id",
        "printingId",
        "priceMetric",
        "targetUsd",
        "condition",
        "deadline",
        "tradingClosesAt",
        "observationStartsAt",
        "samplingPolicy",
        "terminalSlot",
        "rulesRevision",
        "payoutCoins",
        "state",
        "acceptingOrders",
        "outcome",
        "resolvedAt",
    ];
    projection_matrix::<crate::prediction_reads::OwnedMarket>(
        f["market"].clone(),
        &[
            ("", &["accountId", "market"]),
            ("/market", &["terms", "book"]),
            ("/market/terms", market_fields),
            ("/market/book/0", &["side", "limitCost", "quantity"]),
        ],
    );
    projection_matrix::<crate::prediction_reads::MarketPage>(
        f["markets"].clone(),
        &[
            ("", &["accountId", "items", "nextCursor"]),
            ("/items/0", market_fields),
        ],
    );
    projection_matrix::<crate::prediction_reads::OrderPage>(
        f["orders"].clone(),
        &[
            ("", &["accountId", "items", "nextCursor"]),
            (
                "/items/0",
                &[
                    "id",
                    "marketId",
                    "side",
                    "limitCost",
                    "quantity",
                    "filledQuantity",
                    "actualFilledCost",
                    "unfilledReservation",
                    "state",
                    "acceptedSequence",
                    "acceptedAt",
                ],
            ),
        ],
    );
    projection_matrix::<crate::prediction_reads::PositionPage>(
        f["positions"].clone(),
        &[
            ("", &["accountId", "items", "nextCursor"]),
            (
                "/items/0",
                &[
                    "id",
                    "marketId",
                    "orderId",
                    "side",
                    "quantity",
                    "actualCost",
                    "fixedPayout",
                    "matchedAt",
                ],
            ),
        ],
    );
    let source = fixture(include_str!(
        "../../tests/fixtures/prediction-publication.json"
    ));
    projection_matrix::<crate::prediction_publication::OwnedPublicationSource>(
        source["source"].clone(),
        &[
            ("", &["accountId", "source"]),
            (
                "/source",
                &[
                    "printingId",
                    "probeId",
                    "priceMetric",
                    "currentUsd",
                    "acquiredAt",
                    "usableUntil",
                ],
            ),
        ],
    );
    let id = Uuid::new_v4();
    projection_matrix::<crate::prediction_publication::OwnedProbeBasis>(
        json!({"accountId":id,"basis":{"printingId":id,"externalPrintingId":id,"slot":"2026-11-02"}}),
        &[
            ("", &["accountId", "basis"]),
            ("/basis", &["printingId", "externalPrintingId", "slot"]),
        ],
    );
}

#[test]
fn pack_products_quotes_and_opening_contents_require_objects() {
    let f = fixture(include_str!("../../tests/fixtures/pack-shop.json"));
    projection_matrix::<crate::pack_shop::PackProductPage>(
        f["products"].clone(),
        &[
            ("", &["accountId", "items", "nextCursor"]),
            (
                "/items/0",
                &["id", "productId", "setId", "name", "setName", "revisionRef"],
            ),
        ],
    );
    projection_matrix::<crate::pack_shop::OwnedPackQuote>(
        f["purchase"].clone(),
        &[
            ("", &["accountId", "quote"]),
            (
                "/quote",
                &[
                    "recipeId",
                    "productId",
                    "policyId",
                    "revision",
                    "remaining",
                    "unitPrice",
                    "quantity",
                    "total",
                    "purpose",
                ],
            ),
        ],
    );
    let id = Uuid::new_v4();
    projection_matrix::<crate::packs::OwnedPackOutcome>(
        json!({"accountId":id,"outcome":{"id":id,"inventoryAssetId":id,"recipeId":id,"operationId":id,"contents":[{"printingId":id,"quantity":"2"}]}}),
        &[
            ("", &["accountId", "outcome"]),
            (
                "/outcome",
                &[
                    "id",
                    "inventoryAssetId",
                    "recipeId",
                    "operationId",
                    "contents",
                ],
            ),
            ("/outcome/contents/0", &["printingId", "quantity"]),
        ],
    );
}

#[test]
fn nullable_object_fields_preserve_required_null_unknown_and_duplicate_contracts() {
    let id = Uuid::new_v4();
    let person = social_person(id);
    nullable_objects::<crate::social::SocialStatus>(
        json!({"accountId":id,"unreadMessages":"0","personId":id,"person":person}),
        &["/person"],
    );
    nullable_objects::<crate::social::DirectMessagePage>(
        json!({"accountId":id,"conversationId":id,"person":person,"items":[],"nextCursor":{"sentAt":"2026-11-02T12:00:00.000001Z","id":id}}),
        &["/nextCursor"],
    );
    let appearance = json!({"backId":id,"sleeveId":id,"diceId":id,"playmatId":null});
    let deck = json!({"id":id,"name":"Deck","mode":"collection","currentRevisionId":id,"format":{"id":id,"key":"standard","name":"Standard"},"appearance":appearance});
    nullable_objects::<crate::decks::reads::DeckDetail>(
        json!({"deck":deck,"revision":{"id":id,"number":"1","entries":[]}}),
        &["/revision"],
    );
    let exchange = fixture(include_str!("../../tests/fixtures/exchange-reads.json"));
    nullable_objects::<crate::exchange_reads::OwnedTrade>(
        exchange["acceptedTrade"].clone(),
        &["/trade/settlement"],
    );
    nullable_objects::<crate::exchange_reads::OwnedListing>(
        exchange["soldListing"].clone(),
        &[
            "/listing/participant",
            "/listing/participant/buyer",
            "/listing/participant/settlement",
        ],
    );
}
