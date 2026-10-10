//! A block's state sent as what changed (#713). An agent block's state is
//! its fields and a window of its transcript: `entries`, the first of them
//! numbered `entries_from`. A patch is the same object with the window's
//! unchanged entries left out: its `entries` start at `entries_at`, and the
//! entries in `[entries_from, entries_at)` are the ones the client has
//! already. Every other field is whole. A state with no `entries_at` is
//! whole.

use serde_json::Value;

/// Where a patch's entries start, or `None` for a whole state.
pub fn patch_at(state: &Value) -> Option<u64> {
    state.get("entries_at")?.as_u64()
}

/// `state` (whole) as a patch for a client that has its entries before
/// `at` already.
pub fn to_patch(state: &Value, at: u64) -> Value {
    let from = state["entries_from"].as_u64().unwrap_or(0);
    let mut out = serde_json::Map::new();
    for (k, v) in state.as_object().into_iter().flatten().filter(|(k, _)| *k != "entries") {
        out.insert(k.clone(), v.clone());
    }
    let entries = state["entries"].as_array().map(Vec::as_slice).unwrap_or_default();
    let skip = (at.saturating_sub(from) as usize).min(entries.len());
    out.insert("entries".into(), entries[skip..].to_vec().into());
    out.insert("entries_at".into(), at.max(from).into());
    out.into()
}

/// `older` followed by `newer`, as one: what a client that got both would
/// have. Whole if `older` was; `None` when `newer` doesn't follow from
/// `older` (it keeps entries `older` doesn't have), so both must be sent.
pub fn merge(older: &Value, newer: Value) -> Option<Value> {
    let Some(at) = patch_at(&newer) else { return Some(newer) };
    let from = newer["entries_from"].as_u64().unwrap_or(0);
    let old_at = patch_at(older).unwrap_or_else(|| older["entries_from"].as_u64().unwrap_or(0));
    let old = older["entries"].as_array().map(Vec::as_slice).unwrap_or_default();
    // What `older` holds of the entries `newer` keeps, [start, at).
    let start = from.max(old_at).min(at);
    if start < at && old_at + (old.len() as u64) < at {
        return None;
    }
    if patch_at(older).is_none() && old_at > from && at > from {
        return None;
    }
    let kept = if start < at { &old[(start - old_at) as usize..(at - old_at) as usize] } else { &[] };
    let mut out = newer;
    let tail = out["entries"].take();
    let mut entries = kept.to_vec();
    entries.extend(tail.as_array().cloned().unwrap_or_default());
    out["entries"] = entries.into();
    match patch_at(older) {
        Some(_) => out["entries_at"] = start.into(),
        None => {
            out.as_object_mut().map(|o| o.remove("entries_at"));
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn whole(from: u64, entries: &[&str]) -> Value {
        json!({ "status": "working", "entries_from": from, "entries": entries })
    }

    #[test]
    fn a_patch_leaves_out_what_the_client_has() {
        let p = to_patch(&whole(10, &["a", "b", "c", "d"]), 12);
        assert_eq!(p, json!({ "status": "working", "entries_from": 10, "entries_at": 12, "entries": ["c", "d"] }));
    }

    #[test]
    fn a_patch_on_a_whole_state_is_whole() {
        let s = whole(0, &["a", "b", "c"]);
        let mut p = to_patch(&whole(0, &["a", "b", "C", "d"]), 2);
        p["status"] = "ready".into();
        let m = merge(&s, p).unwrap();
        assert_eq!(m, json!({ "status": "ready", "entries_from": 0, "entries": ["a", "b", "C", "d"] }));
    }

    #[test]
    fn the_window_moving_on_drops_the_oldest() {
        let s = whole(0, &["a", "b", "c"]);
        let m = merge(&s, to_patch(&whole(1, &["b", "c", "d"]), 3)).unwrap();
        assert_eq!(m, whole(1, &["b", "c", "d"]));
    }

    #[test]
    fn two_patches_make_one() {
        // The first changed from 2 on, the second from 3 on: together,
        // from 2 on, with the first's entry 2.
        let a = to_patch(&whole(0, &["a", "b", "c", "d"]), 2);
        let b = to_patch(&whole(0, &["a", "b", "c", "D", "e"]), 3);
        let m = merge(&a, b).unwrap();
        assert_eq!(m["entries_at"], 2);
        assert_eq!(m["entries"], json!(["c", "D", "e"]));
        // ...and the other way round, the second's start wins.
        let a = to_patch(&whole(0, &["a", "b", "c", "d"]), 3);
        let b = to_patch(&whole(0, &["a", "B", "c", "d"]), 1);
        assert_eq!(merge(&a, b).unwrap()["entries"], json!(["B", "c", "d"]));
    }

    #[test]
    fn a_whole_state_replaces_anything() {
        let a = to_patch(&whole(0, &["a", "b"]), 1);
        assert_eq!(merge(&a, whole(0, &["x"])).unwrap(), whole(0, &["x"]));
    }

    #[test]
    fn a_patch_that_keeps_more_than_was_sent_does_not_merge() {
        let a = to_patch(&whole(0, &["a", "b"]), 1);
        assert!(merge(&a, to_patch(&whole(0, &["a", "b", "c", "d"]), 4)).is_none());
    }
}
