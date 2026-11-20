//! Signed player choices and the normal worker on a genuinely admitted reservation.
use super::*;

#[test]
#[ignore = "requires an isolated admitted two- or three-human reservation, release package and signed sessions"]
fn players_select_the_starter_and_recover_the_original_after_worker_delivery() {
    let file = std::env::var_os("SPELLWAKE_NATIVE_SOURCE_RESERVATION").unwrap();
    let reservation: Reservation = crate::decode_wire_object(&fs::read(file).unwrap()).unwrap();
    let game = reservation.game_id;
    let seats = &reservation.seat_ids;
    assert!((2..=3).contains(&seats.len()));
    let owner = std::env::var("SPELLWAKE_NATIVE_HOST_OWNER").unwrap();
    let http = http::HttpProbe::owned(
        &std::env::var("SPELLWAKE_SUPABASE_URL").unwrap(),
        &std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY").unwrap(),
        &owner,
    );
    let tokens: Vec<_> = [
        "SPELLWAKE_NATIVE_STORAGE_CALLER",
        "SPELLWAKE_NATIVE_SOURCE_PEER_BEARER",
        "SPELLWAKE_NATIVE_SOURCE_OUTSIDER_BEARER",
    ]
    .into_iter()
    .take(seats.len())
    .map(|name| std::env::var(name).unwrap())
    .collect();
    let worker = NativeMatchWorker::from_env().unwrap();
    let repo = NativeSourceRepository::from_env().unwrap();
    let path = format!("/v1/games/{game}/opening-choice");
    assert!(repo.opening(game).unwrap().is_none());
    let observe = json!({"gameId":game});
    let observed: Vec<_> = tokens
        .iter()
        .map(|token| http.admission_receipt(token, &path, &observe, 200))
        .collect();
    let choosers: Vec<_> = observed
        .iter()
        .enumerate()
        .filter(|(_, r)| r["data"]["yourChoice"] == true)
        .collect();
    let [(index, roll)] = choosers.as_slice() else {
        panic!("exactly one chooser");
    };
    let chooser = roll["data"]["chooserSeatId"]
        .as_str()
        .unwrap()
        .parse::<Uuid>()
        .unwrap();
    assert!(
        observed
            .iter()
            .all(|r| r["data"]["chooserSeatId"] == chooser.to_string())
    );
    let first = *seats.iter().find(|seat| **seat != chooser).unwrap();
    let select = json!({"gameId":game,"firstSeatId":first});
    let peer = (*index + 1) % tokens.len();
    http.admission_receipt(&tokens[peer], &path, &select, 404);
    let original = http.admission_receipt(&tokens[*index], &path, &select, 200);
    assert_eq!(original["data"]["firstSeatId"], first.to_string());
    worker.sweep().unwrap();
    let live = repo.read(game).unwrap().unwrap();
    assert_eq!(live.match_id(), reservation.match_id);
    assert_eq!(live.context_ids().1, reservation.release_id);
    assert_eq!(live.context_ids().2, reservation.profile_id);
    assert_eq!(
        http.admission_receipt(&tokens[*index], &path, &select, 200),
        original
    );
    http.admission_receipt(
        &tokens[*index],
        &path,
        &json!({"gameId":game,"firstSeatId":chooser}),
        409,
    );
    assert_eq!(
        http.read(Some(&tokens[*index]), game, live.context_ids().0, 200)["status"],
        "success"
    );
}
