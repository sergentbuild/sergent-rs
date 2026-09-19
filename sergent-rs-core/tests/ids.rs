//! Framework identifier grammar: prefix plus 32 lowercase hex.

use sergent_rs_core::ids::{IdError, RunId, SceneId, TargetId};

fn is_hex32(body: &str) -> bool {
    body.len() == 32 && body.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[test]
fn minted_run_id_has_the_run_prefix_and_32_hex() {
    let id = RunId::mint();
    let (prefix, body) = id.as_str().split_once('_').unwrap();
    assert_eq!(prefix, "run");
    assert!(is_hex32(body));
}

#[test]
fn minted_ids_are_distinct() {
    assert_ne!(RunId::mint(), RunId::mint());
}

#[test]
fn fixed_prefix_parse_rejects_wrong_prefix_and_malformed_body() {
    assert!(matches!(
        RunId::parse("op_0123456789abcdef0123456789abcdef"),
        Err(IdError::WrongPrefix {
            expected: "run",
            ..
        })
    ));
    assert!(matches!(
        RunId::parse("run_0123456789abcdef0123456789abcde"),
        Err(IdError::Malformed { .. })
    ));
    assert!(matches!(
        RunId::parse("run_0123456789ABCDEF0123456789abcdef"),
        Err(IdError::Malformed { .. })
    ));
}

#[test]
fn open_prefix_ids_accept_an_application_chosen_prefix() {
    let scene = SceneId::mint("board").unwrap();
    assert!(scene.as_str().starts_with("board_"));
    let raw = "doc_0123456789abcdef0123456789abcdef";
    assert_eq!(TargetId::parse(raw).unwrap().as_str(), raw);
}

#[test]
fn open_prefix_mint_rejects_an_invalid_prefix() {
    assert!(matches!(
        SceneId::mint("1bad"),
        Err(IdError::BadPrefix { .. })
    ));
    assert!(matches!(SceneId::mint(""), Err(IdError::BadPrefix { .. })));
}

#[test]
fn scene_ids_convert_to_target_ids_without_text_crossing() {
    let scene = SceneId::mint("board").unwrap();
    let from_borrowed = TargetId::from(&scene);
    let from_owned = TargetId::from(scene.clone());

    assert_eq!(from_borrowed, scene);
    assert_eq!(scene, from_owned);
    assert_eq!(from_borrowed, from_owned);
}

#[test]
fn persistence_owned_ids_deserialize_through_the_validating_boundary() {
    // A valid id round-trips; an invalid one fails on the persistence-load path.
    let json = "\"doc_0123456789abcdef0123456789abcdef\"";
    let id: SceneId = serde_json::from_str(json).unwrap();
    assert_eq!(id.as_str(), "doc_0123456789abcdef0123456789abcdef");
    assert!(serde_json::from_str::<SceneId>("\"not-an-id\"").is_err());
}
