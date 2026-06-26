// QCue S2-R32/A-R26 — snapshot once; mid-session writes don't change prefix bytes (pitfall #2).
// S2-R33/A-R30 — the prefetch pipeline ranks (strict JSON, never throws), reads top-K, and fences the
// survivors into the message TAIL.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use ideas::recall::curated::CuratedSnapshot;
use ideas::recall::prefetch::{run_prefetch, PrefetchItem};

#[test]
fn curated_snapshot_byte_stable_after_midsession_write() {
    let snap = CuratedSnapshot::freeze("MEM v1", "USER v1");
    let h_before = snap.prefix_hash();
    // simulate a mid-session memory write to disk — the FROZEN snapshot is untouched (pitfall #2).
    let _new_disk_state = CuratedSnapshot::freeze("MEM v2", "USER v2");
    assert_eq!(snap.prefix_hash(), h_before); // the live session's prefix bytes are unchanged
}

#[tokio::test]
async fn prefetch_pipeline_ranks_reads_and_tail_fences() {
    // scan → manifest → cheap-rank (strict JSON, never throws) → read top-K → fenced tail
    let manifest = vec![
        PrefetchItem {
            id: "a".into(),
            keywords: "rust async".into(),
            content: "Tokio is an async runtime.".into(),
        },
        PrefetchItem { id: "b".into(), keywords: "graphs".into(), content: "Graph theory basics.".into() },
    ];
    // a ranker that returns valid JSON selecting "a"
    let tail =
        run_prefetch(manifest.clone(), |_m| r#"{"selected":["a"]}"#.to_string(), 20_000, 60_000, 0).await;
    assert!(tail.contains("Tokio is an async runtime."));
    assert!(tail.contains("<untrusted_source")); // fenced tail (A-R32)
    assert!(!tail.contains("Graph theory")); // unranked item dropped

    // a malformed ranker → [] → empty tail, the turn proceeds (A-R31)
    let empty = run_prefetch(manifest, |_m| "garbage".to_string(), 20_000, 60_000, 0).await;
    assert_eq!(empty, "");
}
