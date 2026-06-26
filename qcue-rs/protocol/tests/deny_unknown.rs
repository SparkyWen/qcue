#![allow(clippy::unwrap_used, clippy::expect_used)]
// QCue S1-R4 — inbound DTOs reject unknown fields; the provider_data bag accepts them.
use protocol::Message;
use serde_json::json;

#[test]
fn test_message_denies_unknown_field() {
    let bad = json!({"role":"User","content":"hi","sneaky_extra":true});
    let r: Result<Message, _> = serde_json::from_value(bad);
    assert!(r.is_err(), "Message must reject unknown top-level fields");
}

#[test]
fn test_provider_data_bag_accepts_unknown() {
    let ok = json!({"role":"Assistant","provider_data":{"any_vendor_key":123}});
    let m: Message = serde_json::from_value(ok).unwrap();
    assert!(m.provider_data.unwrap().get("any_vendor_key").is_some());
}
