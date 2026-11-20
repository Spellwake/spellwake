//! Explicit input-only syntax/seed-value construction; never initializes native state.
use super::*;

fn read(name: &str, expected: &str) -> Result<(Vec<u8>, Value), String> {
    let path = env::var_os(name).ok_or_else(|| format!("explicit {name} required"))?;
    let bytes = fs::read(&path).map_err(|error| error.to_string())?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    if digest != expected {
        return Err(format!("{name} differs from frozen input bytes"));
    }
    let value = packet::unique_json(&bytes)?;
    let size = bytes.len();
    Ok((
        bytes,
        json!({"path":Path::new(&path),"sha256_parsed":digest,"bytes":size,"document":value}),
    ))
}

#[test]
#[ignore = "explicit input-only v2 preflight; no engine construction or selected gameplay"]
fn selected_inputs() {
    let output = env::var_os("SPELLWAKE_PRIVATE_PREFLIGHT_OUTPUT")
        .expect("explicit preflight output required");
    let result = (|| -> Result<Value, String> {
        let (bytes, base) = read("SPELLWAKE_PRIVATE_INPUT", INPUT_SHA256)?;
        let packet = packet::Packet::parse(&bytes)?;
        let definitions = packet.definition_inputs()?;
        let mut admitted = Vec::new();
        let mut rejected = Vec::new();
        for fixture in rows(&base["document"], "fixtures")? {
            let id = string(fixture, "id")?;
            let selected = packet.select(id)?;
            match runner(&selected) {
                Ok(route) => {
                    selected.prepare_seed(
                        packet::SeedMode::TrustedUnit,
                        if route == Runner::Admission {
                            packet::DefinitionPolicy::DeferToOwner
                        } else {
                            packet::DefinitionPolicy::ValidateSelected
                        },
                    )?;
                    admitted.push(json!({"id":id,"runner":format!("{route:?}")}));
                }
                Err(reason) => rejected.push(json!({"id":id,"reason":reason})),
            }
        }
        let (_, definition_input) = read(
            "SPELLWAKE_PRIVATE_DEFINITION_INPUT",
            definitions::INPUT_SHA256,
        )?;
        let definition_ids = definitions::preflight(&definition_input["document"], &definitions)?;
        let (_, matcher_input) = read("SPELLWAKE_PRIVATE_MATCHER_INPUT", matcher::INPUT_SHA256)?;
        let matcher_ids = matcher::preflight(&matcher_input["document"], &definitions)?;
        let unique = |ids: &[String]| ids.iter().collect::<BTreeSet<_>>().len() == ids.len();
        if admitted.len() != 66
            || rejected.len() != 92
            || definition_ids.len() != 33
            || matcher_ids.len() != 98
            || !unique(&definition_ids)
            || !unique(&matcher_ids)
        {
            return Err("finite selected input inventory differs".into());
        }
        Ok(
            json!({"status":"completed_input_only_preflight","base_sha256":base["sha256_parsed"],"definition_sha256":definition_input["sha256_parsed"],"matcher_sha256":matcher_input["sha256_parsed"],"main_admitted":admitted,"main_other_scope_rejected":rejected,"definition_ids":definition_ids,"matcher_ids":matcher_ids,"native_owner_construction_or_execution":false,"expected_data_read":false,"qualification_claim":"none"}),
        )
    })();
    let report = match &result {
        Ok(value) => value.clone(),
        Err(error) => {
            json!({"status":"failed_input_only_preflight","error":error,"native_owner_construction_or_execution":false})
        }
    };
    fs::write(output, serde_json::to_vec_pretty(&report).unwrap())
        .expect("write pure preflight report");
    assert!(result.is_ok(), "input-only preflight failed: {result:?}");
}
