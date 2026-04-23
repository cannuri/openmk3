//! Build a synthetic .nksf, parse it back, assert round-trip fidelity.
//!
//! This does NOT test against real NI files (those live under the user's
//! license and can't ship in the repo). Instead it encodes known-shape
//! MessagePack payloads into a RIFF/NIKS container with the four expected
//! chunks, then walks the parser and checks every field.

use std::io::Write;

use nks_parse::{NksFile, NksPluginId};

fn riff_le(tag: &[u8; 4], body: &[u8]) -> Vec<u8> {
    // Real NKS sub-chunks carry a 4-byte LE version prefix before the
    // msgpack payload. Match that layout here so the tests exercise the
    // same code path as production.
    let mut payload = Vec::with_capacity(4 + body.len());
    payload.extend_from_slice(&1u32.to_le_bytes());
    payload.extend_from_slice(body);
    let mut out = Vec::with_capacity(payload.len() + 8);
    out.extend_from_slice(tag);
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&payload);
    if payload.len() % 2 == 1 { out.push(0); }
    out
}

#[test]
fn round_trip_nisi_plid_nica_pchk() {
    // NISI payload: msgpack map with known keys.
    let nisi = rmp_serde::to_vec_named(&serde_json::json!({
        "name": "Acid Lead",
        "vendor": "TestVendor",
        "author": "cannuri",
        "comment": "unit-test preset",
        "bankchain": ["Massive X", "Factory", "Leads"],
        "types": [["Synth Lead", "Hard Lead"], ["Bass", "Synth Bass"]],
        "modes": ["Bright", "Arpeggiated"],
        "deviceType": "INST",
    })).unwrap();
    // PLID payload: VST3 UID.
    let plid = rmp_serde::to_vec_named(&serde_json::json!({
        "VST3.uid": [0x11223344i32, 0x55667788u32 as i32, 0xaabbccddu32 as i32, 0x10203040],
        "VST3.plugin_name": "Massive X",
        "plugin_name": "Massive X",
    })).unwrap();
    let nica = b"\x80".to_vec(); // msgpack fixmap size 0 — valid-but-empty
    let pchk = b"PLUGIN-STATE-BLOB-xxxxxxxx".to_vec();

    let mut body = Vec::new();
    body.extend_from_slice(b"NIKS");
    body.extend(riff_le(b"NISI", &nisi));
    body.extend(riff_le(b"PLID", &plid));
    body.extend(riff_le(b"NICA", &nica));
    body.extend(riff_le(b"PCHK", &pchk));
    let mut whole = Vec::new();
    whole.extend_from_slice(b"RIFF");
    whole.extend_from_slice(&(body.len() as u32).to_le_bytes());
    whole.extend_from_slice(&body);

    let tmp = tempfile::Builder::new().suffix(".nksf").tempfile().unwrap();
    tmp.as_file().write_all(&whole).unwrap();
    tmp.as_file().sync_all().unwrap();

    let nks = NksFile::scan(tmp.path()).expect("scan");
    assert_eq!(nks.summary.name.as_deref(), Some("Acid Lead"));
    assert_eq!(nks.summary.vendor.as_deref(), Some("TestVendor"));
    assert_eq!(nks.summary.author.as_deref(), Some("cannuri"));
    assert_eq!(nks.summary.bankchain.as_ref().unwrap().len(), 3);
    assert_eq!(nks.summary.types.as_ref().unwrap()[0][1], "Hard Lead");
    assert_eq!(nks.summary.modes.as_ref().unwrap().len(), 2);
    assert_eq!(nks.summary.device_type.as_deref(), Some("INST"));

    match &nks.plugin {
        NksPluginId::Vst3 { uid, name } => {
            assert_eq!(uid[0] as u32, 0x11223344);
            assert_eq!(uid[3] as u32, 0x10203040);
            assert_eq!(name.as_deref(), Some("Massive X"));
        }
        other => panic!("expected Vst3, got {other:?}"),
    }
    assert_eq!(nks.pchk_len, pchk.len() as u64);
    let state = nks.read_state().unwrap();
    assert_eq!(state, pchk);
}

#[test]
fn rejects_non_niks_container() {
    let tmp = tempfile::Builder::new().suffix(".nksf").tempfile().unwrap();
    let mut f = tmp.as_file();
    f.write_all(b"RIFF\x04\x00\x00\x00WAVE").unwrap();
    f.sync_all().unwrap();
    let err = NksFile::scan(tmp.path()).unwrap_err();
    assert!(matches!(err, nks_parse::NksError::NotNks), "got {err:?}");
}
