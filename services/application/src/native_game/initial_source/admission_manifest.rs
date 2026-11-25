//! Server-selected admission for one accepted application release and legality snapshot.
use super::*;
use crate::actor::ActorError;
use crate::decks::PlayMode;
use crate::native_game::preparation_source::NativePreparationRepository;
use crate::price_source::EvidenceHash;
use std::collections::BTreeSet;
use std::path::Path;

// The pinned catalogue contains over 32,000 eligible Oracle identities. Bound
// both the encoded input and index size so startup can budget for either limit.
const MAX_MANIFEST_BYTES: usize = 16 * 1024 * 1024;
const MAX_APPROVED_CARDS: usize = 50_000;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TablePolicy {
    mode: crate::decks::PlayMode,
    structure: crate::rooms::configuration::MatchStructure,
    competitive: bool,
    controllers: Vec<crate::match_history::ControllerKind>,
    ai_seats: Option<u8>,
    between_game_seconds: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Profile {
    format_id: Uuid,
    format_key: String,
    profile_id: Uuid,
    profile_revision: String,
    profile_rules: String,
    profile_evidence: EvidenceHash,
    table_policy: Option<TablePolicy>,
    minimum_seats: crate::amount::Amount,
    seat_capacity: crate::amount::Amount,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AdmissionManifest {
    schema: String,
    release_id: Uuid,
    #[serde(skip)]
    pin: Option<EvidenceHash>,
    legality_revision_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    profiles: Vec<Profile>,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    cards: Vec<ApprovedCard>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ApprovedCard {
    oracle_id: Uuid,
    definition_revision_id: Uuid,
    program: EvidenceHash,
    #[serde(rename = "behaviorEvidence")]
    _behavior_evidence: EvidenceHash,
}

impl AdmissionManifest {
    pub(crate) fn release_id(&self) -> Uuid {
        self.release_id
    }

    pub(crate) fn from_env() -> Result<Option<Self>, ActorError> {
        let file = std::env::var_os("SPELLWAKE_NATIVE_ADMISSION_MANIFEST");
        let pin = std::env::var_os("SPELLWAKE_NATIVE_ADMISSION_MANIFEST_SHA256");
        match (file, pin) {
            (None, None) => Ok(None),
            (Some(file), Some(pin)) => {
                let bytes = crate::catalogue_source::read_bounded(
                    Path::new(&file),
                    MAX_MANIFEST_BYTES as u64,
                )
                .map_err(|_| ActorError::Configuration)?;
                Self::verified(&bytes, pin.to_str().ok_or(ActorError::Configuration)?).map(Some)
            }
            _ => Err(ActorError::Configuration),
        }
    }

    fn verified(bytes: &[u8], expected: &str) -> Result<Self, ActorError> {
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err(ActorError::Configuration);
        }
        let pin = digest(bytes);
        if pin.as_str() != expected {
            return Err(ActorError::Configuration);
        }
        let mut manifest: Self =
            crate::decode_wire_object(bytes).map_err(|_| ActorError::Configuration)?;
        let mut profiles = BTreeSet::new();
        let scoped = manifest.schema == "spellwake-accepted-constructed-v2";
        if (!scoped && manifest.schema != "spellwake-accepted-constructed-v1")
            || manifest.release_id.is_nil()
            || manifest.legality_revision_id.is_nil()
            || manifest.profiles.is_empty()
            || manifest.cards.is_empty()
            || manifest.cards.len() > MAX_APPROVED_CARDS
            || !manifest.profiles.iter().all(|p| {
                !p.format_id.is_nil()
                    && !p.profile_id.is_nil()
                    && matches!(
                        p.format_key.as_str(),
                        "standard" | "pioneer" | "modern" | "legacy" | "vintage" | "draft" | "cube"
                    )
                    && !p.profile_rules.is_empty()
                    && !p.profile_revision.is_empty()
                    && p.table_policy.is_some() == scoped
                    && (!matches!(p.format_key.as_str(), "draft" | "cube")
                        || p.table_policy.as_ref().is_some_and(|policy| {
                            policy.mode == crate::decks::PlayMode::Open
                                && policy.structure
                                    == crate::rooms::configuration::MatchStructure::Bo1
                                && !policy.competitive
                                && policy.controllers
                                    == [crate::match_history::ControllerKind::Human]
                        }))
                    && p.minimum_seats >= crate::amount::Amount::from(2)
                    && p.minimum_seats <= p.seat_capacity
                    && p.seat_capacity <= crate::amount::Amount::from(6)
                    && p.table_policy.as_ref().is_none_or(|table| {
                        table.between_game_seconds.is_none_or(|seconds| {
                            (1..=86400).contains(&seconds)
                                && table.structure
                                    == crate::rooms::configuration::MatchStructure::Bo3
                        }) && !table.controllers.is_empty()
                            && table.controllers.len() <= 2
                            && if table.controllers.len() == 1 {
                                table.controllers[0] == crate::match_history::ControllerKind::Human
                                    && table.ai_seats.is_none()
                            } else {
                                table.controllers[0] != table.controllers[1]
                                    && p.minimum_seats == p.seat_capacity
                                    && table.ai_seats.is_some_and(|n| {
                                        n > 0
                                            && crate::amount::Amount::from(u64::from(n))
                                                < p.seat_capacity
                                    })
                            }
                    })
                    && profiles.insert((p.format_id, p.profile_id))
            })
            || manifest
                .cards
                .iter()
                .any(|card| card.oracle_id.is_nil() || card.definition_revision_id.is_nil())
        {
            return Err(ActorError::Configuration);
        }
        // The byte digest authenticates the original order. The sorted entries
        // give every admission the same immutable lookup without rebuilding it.
        manifest.cards.sort_unstable_by_key(|card| card.oracle_id);
        if manifest
            .cards
            .windows(2)
            .any(|pair| pair[0].oracle_id == pair[1].oracle_id)
        {
            return Err(ActorError::Configuration);
        }
        manifest.pin = Some(pin);
        Ok(manifest)
    }

    // Every approval is in the pinned manifest. Caller-supplied bytes never select
    // a release, a format, a card program, or the evidence behind that program.
    fn check_entries(&self, entries: &[preparation_source::PreparationEntry]) -> bool {
        entries.iter().all(|e| {
            Uuid::parse_str(e.oracle_ref.as_str())
                .ok()
                .and_then(|id| self.approved_card(id))
                .is_some_and(|card| {
                    card.definition_revision_id == e.definition_revision_id
                        && card.program.as_str() == e.program_ref.as_str()
                        && e.definition_revision_ref.as_str().starts_with("catalogue:")
                })
        })
    }

    fn approved_card(&self, oracle: Uuid) -> Option<&ApprovedCard> {
        let index = self
            .cards
            .binary_search_by_key(&oracle, |card| card.oracle_id)
            .ok()?;
        self.cards.get(index)
    }

    fn bound_profile(
        &self,
        repo: &NativePreparationRepository,
        format: Uuid,
        profile: &preparation_source::PreparationProfile,
        release: &preparation_source::PreparationRelease,
    ) -> Result<Option<&Profile>, preparation_source::NativePreparationError> {
        let Some(bound) = self
            .profiles
            .iter()
            .find(|p| p.format_id == format && p.profile_id == profile.id)
        else {
            return Ok(None);
        };
        if self.release_id != release.id
            || self.pin.as_ref().map(EvidenceHash::as_str) != Some(release.evidence_ref.as_str())
            || profile.qualification_ref.as_str() != bound.profile_evidence.as_str()
            || profile.revision_ref.as_str() != bound.profile_revision
            || profile.rules_ref.as_str() != bound.profile_rules
            || profile.seat_capacity != bound.seat_capacity
            || profile.minimum_seats != bound.minimum_seats
            || profile.planar_pool_ref.is_some()
            || repo.constructed_format_key(format)? != bound.format_key
        {
            return Ok(None);
        }
        Ok(Some(bound))
    }

    pub(crate) fn reserved_format(
        &self,
        repo: &NativePreparationRepository,
        source: &preparation_source::NativeReservedGameMetadata,
    ) -> Result<Option<&str>, preparation_source::NativePreparationError> {
        let Some(profile) =
            self.bound_profile(repo, source.format_id, &source.profile, &source.release)?
        else {
            return Ok(None);
        };
        let ai = source
            .seats
            .iter()
            .filter(|seat| seat.controller_kind == crate::match_history::ControllerKind::Ai)
            .count();
        if source.legality_revision_id != self.legality_revision_id
            || !source
                .seats
                .iter()
                .all(|seat| self.check_entries(&seat.entries))
            || profile.table_policy.as_ref().is_some_and(|policy| {
                policy.mode != source.mode
                    || policy.competitive != source.competitive
                    || policy.structure != source.structure
                    || policy
                        .ai_seats
                        .is_some_and(|expected| usize::from(expected) != ai)
                    || source
                        .seats
                        .iter()
                        .any(|seat| !policy.controllers.contains(&seat.controller_kind))
            })
        {
            return Ok(None);
        }
        Ok(Some(&profile.format_key))
    }

    pub(crate) fn prepare_between(
        &self,
        repo: &NativePreparationRepository,
        programs: &NativeProgramSource,
        original: &crate::qualified_intents::BetweenGameIntent,
        owner: &crate::qualified_intents::BetweenDecisionOwner,
        account: Uuid,
    ) -> Result<Option<serde_json::Value>, preparation_source::NativePreparationError> {
        let Some((selection, hash)) = self.between_selection(repo, programs, original)? else {
            return Ok(None);
        };
        let Ok(prepared) =
            crate::qualified_intents::PreparedBetweenGame::new(original, account, owner, selection)
        else {
            return Ok(None);
        };
        Ok(Some(Self::between_payload(prepared.source(), &hash)))
    }

    pub(crate) fn between_payload(
        reply: &crate::deadline_ports::BetweenGameReply,
        hash: &str,
    ) -> serde_json::Value {
        serde_json::json!({
            "version": "native-between-prepared-v1",
            "sourceHash": hash,
            "replyRef": reply.reply_ref.as_str(),
            "qualificationRef": reply.qualification_ref.as_str()
        })
    }

    pub(crate) fn between_selection(
        &self,
        repo: &NativePreparationRepository,
        programs: &NativeProgramSource,
        original: &crate::qualified_intents::BetweenGameIntent,
    ) -> Result<
        Option<(crate::deadline_ports::BetweenGameReply, String)>,
        preparation_source::NativePreparationError,
    > {
        use crate::deadline_ports::BetweenGameReply;
        let Some(observation) = repo.between(
            original.decision_id,
            &original.expected_revision,
            original.selected_revision_id,
        )?
        else {
            return Ok(None);
        };
        let terms = observation.terms();
        if terms.r#match.legality_revision_id != self.legality_revision_id {
            return Ok(None);
        }
        let Some(profile) = self.bound_profile(
            repo,
            terms.r#match.format_id,
            &terms.profile,
            &terms.release,
        )?
        else {
            return Ok(None);
        };
        let allow_custom = terms.r#match.allow_custom_cards && terms.r#match.mode == PlayMode::Open;
        if profile.table_policy.as_ref().is_some_and(|table| {
            table.mode != terms.r#match.mode || table.structure != terms.r#match.structure
        }) || !self.check_entries(&terms.entries)
            || programs
                .verify_constructed_release_content_with_custom(
                    &terms.release,
                    &terms.entries,
                    &terms.custom_entries,
                )
                .is_err()
            || programs
                .inspect_selected_match_deck_with_custom(
                    &profile.format_key,
                    &terms.entries,
                    &terms.custom_entries,
                    allow_custom,
                )
                .is_err()
        {
            return Ok(None);
        }
        let hash = observation.source_hash();
        let reply = format!("native-between-reply-v1/{hash}").parse();
        let qualification = format!(
            "native-constructed-between-v1/{}/{hash}",
            profile.format_key
        )
        .parse();
        let (Ok(reply_ref), Ok(qualification_ref)) = (reply, qualification) else {
            return Ok(None);
        };
        let selection = BetweenGameReply {
            decision_id: terms.decision.id,
            basis: terms.r#match.revision.clone(),
            selected_revision_id: terms.selected_revision_id,
            request_id: original.request_id,
            reply_ref,
            qualification_ref,
        };
        Ok(Some((selection, hash.to_owned())))
    }

    pub(crate) fn prepare_room(
        &self,
        repo: &NativePreparationRepository,
        programs: &NativeProgramSource,
        room: Uuid,
        original: &crate::qualified_intents::RoomAdmissionIntent,
        actor: Uuid,
        countdown: bool,
    ) -> Result<Option<serde_json::Value>, preparation_source::NativePreparationError> {
        let Some(observation) = repo.room(
            room,
            &original.expected_revision,
            self.release_id,
            self.legality_revision_id,
        )?
        else {
            return Ok(None);
        };
        let t = observation.terms();
        let Some(slot) = t.slots.iter().find(|s| {
            s.account_id == Some(actor)
                && s.controller_kind == crate::match_history::ControllerKind::Human
        }) else {
            return Ok(None);
        };
        // The locked countdown writer records host-authority failures for seated callers.
        let Some(p) = self.bound_profile(repo, t.room.format_id, &t.profile, &t.release)? else {
            return Ok(None);
        };
        if p.table_policy.as_ref().is_some_and(|table| {
            let ai = t
                .slots
                .iter()
                .filter(|s| s.controller_kind == crate::match_history::ControllerKind::Ai)
                .count();
            let roster_exceeded = table.ai_seats.is_some_and(|expected| {
                ai > usize::from(expected)
                    || crate::amount::Amount::from(
                        (t.slots.len() - ai) as u64 + u64::from(expected),
                    ) > p.seat_capacity
            });
            roster_exceeded
                || table.mode != t.room.mode
                || table.structure != t.room.structure
                || table.competitive != t.room.competitive
                || t.slots
                    .iter()
                    .any(|slot| !table.controllers.contains(&slot.controller_kind))
        }) || t.slots.iter().any(|slot| slot.team_ref.is_some())
        {
            return Ok(None);
        }
        let Some(selected) = slot.selected_revision_id else {
            return Ok(None);
        };
        let qualification = if countdown {
            if !t
                .slots
                .iter()
                .all(|s| s.entries.as_deref().is_some_and(|e| self.check_entries(e)))
            {
                return Ok(None);
            }
            programs
                .constructed_room_reference(&observation, &p.format_key)
                .ok()
        } else {
            let Some(entries) = slot.entries.as_deref() else {
                return Ok(None);
            };
            let custom_entries = slot.custom_entries.as_deref().unwrap_or(&[]);
            let allow_custom =
                t.room.allow_custom_cards && t.room.mode == PlayMode::Open && !t.room.competitive;
            if !self.check_entries(entries)
                || programs
                    .verify_constructed_release_content_with_custom(
                        &t.release,
                        entries,
                        custom_entries,
                    )
                    .is_err()
            {
                return Ok(None);
            }
            let Ok(counts) = programs.selected_match_deck_counts_with_custom(
                &p.format_key,
                entries,
                custom_entries,
                allow_custom,
            ) else {
                return Ok(None);
            };
            if counts.is_err() {
                return Ok(Some(serde_json::json!({
                    "version": "native-ready-rejected-v1",
                    "sourceHash": observation.source_hash(),
                    "releaseId": self.release_id,
                    "legalityRevisionId": self.legality_revision_id,
                    "qualificationRef": "native-ready-deck-rejected-v1",
                    "selectedRevisionId": selected,
                })));
            }
            serde_json::to_vec(&(
                "native-constructed-ready-v1",
                self.release_id,
                self.legality_revision_id,
                p.profile_id,
                p.format_id,
                &p.format_key,
                actor,
                selected,
                observation.source_hash(),
            ))
            .ok()
            .map(|material| {
                format!(
                    "native-constructed-ready-v1/{}/{}",
                    p.format_key,
                    digest(&material).as_str()
                )
            })
        };
        // A Ready that can complete or revise a countdown has two distinct
        // proofs: ownership of this deck and the executable roster as a whole.
        let full_room = if !countdown
            && !slot.ready
            && (t.room.state == "countdown"
                || t.slots.iter().all(|s| s.ready || s.slot_id == slot.slot_id))
            && t.slots
                .iter()
                .all(|s| s.entries.as_deref().is_some_and(|e| self.check_entries(e)))
        {
            programs
                .constructed_room_reference(&observation, &p.format_key)
                .ok()
        } else {
            None
        };
        Ok(qualification.map(|reference|{
            let mut prepared=serde_json::json!({"version":if countdown {"native-countdown-prepared-v1"} else {"native-ready-prepared-v1"},
                "sourceHash":observation.source_hash(),"releaseId":self.release_id,"legalityRevisionId":self.legality_revision_id,"qualificationRef":reference});
            if !countdown {prepared["selectedRevisionId"]=serde_json::json!(selected);}
            if let Some(full_room)=full_room {prepared["countdownQualificationRef"]=serde_json::json!(full_room);}
            prepared
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn seven_format_manifest_accepts_private_bo1_profiles_and_excludes_commander() {
        let evidence = digest(b"fixture evidence");
        let profiles: Vec<_> = [
            "standard", "pioneer", "modern", "legacy", "vintage", "draft", "cube",
        ]
        .into_iter()
        .map(|format| {
            json!({
                "formatId": Uuid::new_v4(), "formatKey": format, "profileId": Uuid::new_v4(),
                "profileRevision": "fixture-v1", "profileRules": "human-open-bo1",
                "profileEvidence": evidence, "minimumSeats": "2", "seatCapacity": "2",
                "tablePolicy": {"mode": "open", "structure": "bo1", "competitive": false,
                                "controllers": ["human"]},
            })
        })
        .collect();
        let mut document = json!({
            "schema":"spellwake-accepted-constructed-v2", "releaseId":Uuid::new_v4(),
            "legalityRevisionId":Uuid::new_v4(), "profiles": profiles,
            "cards":[{"oracleId":Uuid::new_v4(), "definitionRevisionId":Uuid::new_v4(),
                      "program":evidence, "behaviorEvidence":evidence}],
        });
        let verify = |document: &serde_json::Value| {
            let bytes = serde_json::to_vec(document).unwrap();
            AdmissionManifest::verified(&bytes, digest(&bytes).as_str())
        };
        assert_eq!(verify(&document).unwrap().profiles.len(), 7);
        document["profiles"][5]["tablePolicy"]["competitive"] = json!(true);
        assert!(verify(&document).is_err());
        document["profiles"][5]["tablePolicy"]["competitive"] = json!(false);
        document["profiles"][0]["formatKey"] = json!("commander");
        assert!(verify(&document).is_err());
    }

    #[test]
    fn manifest_binds_approved_bytes_and_unique_published_card_identity() {
        let hash = digest(b"retained behavior test");
        let card = json!({"oracleId":Uuid::new_v4(),"definitionRevisionId":Uuid::new_v4(),"program":hash,"behaviorEvidence":hash});
        let profile = json!({"formatId":Uuid::new_v4(),"formatKey":"legacy","profileId":Uuid::new_v4(),"profileRevision":"v1","profileRules":"ordinary-v1",
            "profileEvidence":hash,"minimumSeats":"2","seatCapacity":"3"});
        let original = json!({"schema":"spellwake-accepted-constructed-v1","releaseId":Uuid::new_v4(),"legalityRevisionId":Uuid::new_v4(),"profiles":[profile],"cards":[card]});
        let read = |value: &serde_json::Value| {
            let bytes = serde_json::to_vec(value).unwrap();
            AdmissionManifest::verified(&bytes, digest(&bytes).as_str())
        };
        assert!(read(&original).is_ok());
        let mut scoped = original.clone();
        scoped["schema"] = json!("spellwake-accepted-constructed-v2");
        assert!(read(&scoped).is_err());
        scoped["profiles"][0]["tablePolicy"] = json!({"mode":"collection","structure":"bo3","competitive":true,"controllers":["human"]});
        assert!(read(&scoped).is_ok());
        for kinds in [json!([]), json!(["human", "human"]), json!(["unknown"])] {
            let mut invalid = scoped.clone();
            invalid["profiles"][0]["tablePolicy"]["controllers"] = kinds;
            assert!(read(&invalid).is_err());
        }
        assert!(
            AdmissionManifest::verified(&serde_json::to_vec(&original).unwrap(), hash.as_str())
                .is_err()
        );
        for (field, value) in [
            ("cards", json!([])),
            ("cards", json!([card, card])),
            ("profiles", json!([profile, profile])),
            ("releaseId", json!(Uuid::nil())),
        ] {
            let mut changed = original.clone();
            changed[field] = value;
            assert!(read(&changed).is_err());
        }
        let mut altered = original.clone();
        altered["cards"][0]["program"] = json!("fixture");
        assert!(read(&altered).is_err());
    }
    #[test]
    fn accepts_and_indexes_catalogue_sized_manifests_with_their_original_pin() {
        let hash = digest(b"qualification fixture");
        let cards: Vec<_> = (1..=32_155)
            .rev()
            .map(|index| {
                json!({
                    "oracleId": Uuid::from_u128(index),
                    "definitionRevisionId": Uuid::from_u128(100_000 + index),
                    "program": hash,
                    "behaviorEvidence": hash,
                })
            })
            .collect();
        let profiles: Vec<_> = ["standard", "pioneer", "modern", "legacy", "vintage"]
            .into_iter()
            .map(|key| json!({
                "formatId": Uuid::new_v4(), "formatKey": key,
                "profileId": Uuid::new_v4(), "profileRevision": "v1", "profileRules": "ordinary-v1",
                "profileEvidence": hash, "minimumSeats": "2", "seatCapacity": "2",
            }))
            .collect();
        let document = json!({
            "schema": "spellwake-accepted-constructed-v1",
            "releaseId": Uuid::new_v4(), "legalityRevisionId": Uuid::new_v4(),
            "profiles": profiles, "cards": cards,
        });
        let bytes = serde_json::to_vec(&document).unwrap();
        assert!(bytes.len() > 2 * 1024 * 1024);
        let pin = digest(&bytes);
        let manifest = AdmissionManifest::verified(&bytes, pin.as_str()).unwrap();
        assert_eq!(manifest.pin.as_ref(), Some(&pin));
        assert_eq!(manifest.cards.len(), 32_155);
        for index in [1, 16_000, 32_155] {
            let found = manifest.approved_card(Uuid::from_u128(index)).unwrap();
            assert_eq!(
                found.definition_revision_id,
                Uuid::from_u128(100_000 + index)
            );
        }
        assert!(manifest.approved_card(Uuid::from_u128(32_156)).is_none());
        assert!(
            AdmissionManifest::verified(&vec![b' '; MAX_MANIFEST_BYTES + 1], pin.as_str()).is_err()
        );
    }
}
