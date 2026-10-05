use proptest::prelude::*;
use rb_diff::{parse_patch, LineKind};

fn patchish() -> impl Strategy<Value = String> {
    let line = prop_oneof![
        "@@ -[0-9]{1,3},[0-9]{1,2} \\+[0-9]{1,3},[0-9]{1,2} @@.{0,10}",
        "[ +-].{0,20}",
        "\\\\ No newline at end of file",
        ".{0,20}",
        Just(String::new()),
    ];
    prop::collection::vec(line, 0..60).prop_map(|v| v.join("\n"))
}

proptest! {
    #[test]
    fn parsing_never_panics(s in "\\PC{0,400}") {
        let _ = parse_patch(&s);
    }

    #[test]
    fn numbers_and_positions_are_monotonic(s in patchish()) {
        let p = parse_patch(&s);
        let mut last_pos = 0u32;
        for (hunk_idx, hunk) in p.hunks.iter().enumerate() {
            let mut last_old = 0u32;
            let mut last_new = 0u32;
            if hunk_idx > 0 {
                prop_assert!(hunk.header_position > last_pos);
                last_pos = hunk.header_position;
            }
            for l in &hunk.lines {
                prop_assert!(l.position > last_pos);
                last_pos = l.position;
                match l.kind {
                    LineKind::Context => prop_assert!(l.old_no.is_some() && l.new_no.is_some()),
                    LineKind::Added => prop_assert!(l.old_no.is_none() && l.new_no.is_some()),
                    LineKind::Removed => prop_assert!(l.old_no.is_some() && l.new_no.is_none()),
                }
                if let Some(o) = l.old_no {
                    prop_assert!(o >= last_old);
                    last_old = o.saturating_add(1);
                }
                if let Some(n) = l.new_no {
                    prop_assert!(n >= last_new);
                    last_new = n.saturating_add(1);
                }
            }
        }
        let ids: Vec<_> = p.iter().map(|(id, _)| id).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        prop_assert_eq!(ids, sorted);
    }
}
