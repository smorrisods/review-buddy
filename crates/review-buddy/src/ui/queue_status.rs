//! The status cluster at the right of a queue row's second line: what other people have said
//! about the change, how much talk it has, how CI is doing and how big it is.
//!
//! Every piece has a fixed width so the pieces line up down the list, and a blank piece keeps
//! its column. Glyphs and numbers carry the meaning; colour only reinforces it. When the pane is
//! narrow, pieces drop from the right (size, then CI words, then comments, then review state).

use rb_core::{ChangeSummary, CiState, OpenThreads, Signals};
use rb_theme::Role;

use crate::app::ChangeInfo;
use crate::config::QueuePiece;

/// Cells between two pieces.
const GAP: usize = 2;
/// Cells between the reason text and the cluster, at the least.
pub const LEAD: usize = 2;

/// One run of text in one role.
pub type Seg = (String, Role);

/// Everything the cluster reads about one change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    pub signals: Signals,
    pub adds: u32,
    pub dels: u32,
    pub files: u32,
    pub ci: CiState,
    /// The reason text already says CI is failing, so the cluster doesn't repeat it.
    pub reason_says_ci: bool,
}

impl Facts {
    /// The list's facts, filled in from the change's loaded details where the list left gaps
    /// (GitLab's list has no approvals, line counts or thread counts).
    pub fn of(change: &ChangeSummary, info: Option<&ChangeInfo>, reason: &str) -> Self {
        let mut facts = Self {
            signals: change.signals,
            adds: change.adds,
            dels: change.dels,
            files: change.files,
            ci: change.ci,
            reason_says_ci: reason.contains("CI failing"),
        };
        let Some(info) = info else {
            return facts;
        };
        if let Some(full) = &info.summary {
            let s = full.signals;
            facts.signals.approvals = s.approvals;
            facts.signals.changes_requested = s.changes_requested;
            facts.signals.outstanding = s.outstanding;
            facts.signals.review_required = s.review_required;
            if facts.adds + facts.dels == 0 {
                facts.adds = full.adds;
                facts.dels = full.dels;
            }
        }
        if !info.threads.is_empty() && !matches!(facts.signals.open_threads, OpenThreads::Count(_))
        {
            let open = info
                .threads
                .iter()
                .filter(|t| !t.resolved && !t.pending)
                .count();
            facts.signals.open_threads =
                OpenThreads::Count(u32::try_from(open).unwrap_or(u32::MAX));
        }
        facts
    }
}

pub fn width(piece: QueuePiece) -> usize {
    match piece {
        QueuePiece::Review => 11,
        QueuePiece::Comments => 12,
        QueuePiece::Ci => 10,
        QueuePiece::Size => 11,
    }
}

/// The pieces that fit in `room` cells, in priority order. Later pieces drop first.
pub fn plan(enabled: &[QueuePiece], room: usize) -> Vec<QueuePiece> {
    let mut out = Vec::new();
    let mut used = 0;
    for piece in [
        QueuePiece::Review,
        QueuePiece::Comments,
        QueuePiece::Ci,
        QueuePiece::Size,
    ] {
        if !enabled.contains(&piece) {
            continue;
        }
        let next = used + width(piece) + if out.is_empty() { 0 } else { GAP };
        if next > room {
            break;
        }
        used = next;
        out.push(piece);
    }
    out
}

/// Cells the whole cluster takes, gaps included.
pub fn total_width(pieces: &[QueuePiece]) -> usize {
    pieces.iter().map(|p| width(*p)).sum::<usize>() + GAP * pieces.len().saturating_sub(1)
}

/// The cluster as segments: each piece padded to its width, joined by [`GAP`] spaces.
pub fn segments(pieces: &[QueuePiece], facts: &Facts) -> Vec<Seg> {
    let mut out = Vec::new();
    for (i, piece) in pieces.iter().enumerate() {
        if i > 0 {
            out.push((" ".repeat(GAP), Role::Muted));
        }
        let mut cell = match piece {
            QueuePiece::Review => review(facts),
            QueuePiece::Comments => comments(facts),
            QueuePiece::Ci => ci(facts),
            QueuePiece::Size => size(facts),
        };
        pad(&mut cell, width(*piece), *piece == QueuePiece::Size);
        out.extend(cell);
    }
    out
}

fn text_width(segs: &[Seg]) -> usize {
    segs.iter().map(|(t, _)| super::text::cells(t)).sum()
}

fn pad(cell: &mut Vec<Seg>, to: usize, right_align: bool) {
    let fill = to.saturating_sub(text_width(cell));
    if fill == 0 {
        return;
    }
    let blank = (" ".repeat(fill), Role::Muted);
    if right_align {
        cell.insert(0, blank);
    } else {
        cell.push(blank);
    }
}

fn count(n: u32, cap: u32) -> String {
    n.min(cap).to_string()
}

/// `✓2 ✕1 ○1`: other people's approvals, change requests and outstanding reviewers. A slot with
/// nothing to say stays blank. A required review nobody was asked for shows a bare `○`.
fn review(f: &Facts) -> Vec<Seg> {
    let s = &f.signals;
    let slot = |glyph: &str, n: u32, role: Role| -> Vec<Seg> {
        if n == 0 {
            return vec![("   ".to_string(), Role::Muted)];
        }
        let mut text = format!("{glyph}{}", count(n, 99));
        let fill = 3_usize.saturating_sub(text.chars().count());
        text.push_str(&" ".repeat(fill));
        vec![(text, role)]
    };
    let mut out = slot("✓", s.approvals, Role::Success);
    out.push((" ".to_string(), Role::Muted));
    out.extend(slot("✕", s.changes_requested, Role::Warning));
    out.push((" ".to_string(), Role::Muted));
    if s.outstanding == 0 && s.review_required {
        out.push(("○  ".to_string(), Role::TextSecondary));
    } else {
        out.extend(slot("○", s.outstanding, Role::TextSecondary));
    }
    out
}

/// `¶4 3 open`: the comment total and, when any are open, the unresolved threads. `·` stands in
/// for an open count that hasn't loaded.
fn comments(f: &Facts) -> Vec<Seg> {
    let s = &f.signals;
    if s.comments == 0 && !s.open_threads.any_open() {
        return Vec::new();
    }
    let mut total = format!("¶{}", count(s.comments, 999));
    total.push_str(&" ".repeat(4_usize.saturating_sub(total.chars().count())));
    let mut out = vec![(total, Role::TextSecondary), (" ".to_string(), Role::Muted)];
    match s.open_threads {
        OpenThreads::Count(n) if n > 0 => {
            out.push((format!("{} open", count(n, 99)), Role::Warning));
        }
        OpenThreads::Any => out.push(("open".to_string(), Role::Warning)),
        OpenThreads::Unknown => out.push(("·".to_string(), Role::Muted)),
        OpenThreads::Count(_) => {}
    }
    out
}

/// `CI running` or `CI failing`; quiet states are left to the leading dot.
fn ci(f: &Facts) -> Vec<Seg> {
    match f.ci {
        CiState::Running => vec![("CI running".to_string(), Role::Warning)],
        CiState::Fail if !f.reason_says_ci => vec![("CI failing".to_string(), Role::Danger)],
        _ => Vec::new(),
    }
}

/// `+120 −8`, or the file count when the forge's list gave no line counts.
fn size(f: &Facts) -> Vec<Seg> {
    if f.adds + f.dels == 0 {
        return match f.files {
            0 => Vec::new(),
            1 => vec![("1 file".to_string(), Role::Muted)],
            n => vec![(format!("{} files", short(n)), Role::Muted)],
        };
    }
    vec![
        (format!("+{}", short(f.adds)), Role::Success),
        (" ".to_string(), Role::Muted),
        (format!("−{}", short(f.dels)), Role::Muted),
    ]
}

/// At most four characters: `999`, `1.2k`, `12k`, `999k`, then `999k`.
fn short(n: u32) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=9_999 => format!("{}.{}k", n / 1_000, (n % 1_000) / 100),
        10_000..=999_999 => format!("{}k", n / 1_000),
        _ => "999k".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> Facts {
        Facts {
            signals: Signals::default(),
            adds: 0,
            dels: 0,
            files: 0,
            ci: CiState::Pass,
            reason_says_ci: false,
        }
    }

    fn flat(pieces: &[QueuePiece], f: &Facts) -> String {
        segments(pieces, f).into_iter().map(|(t, _)| t).collect()
    }

    const ALL: [QueuePiece; 4] = [
        QueuePiece::Review,
        QueuePiece::Comments,
        QueuePiece::Ci,
        QueuePiece::Size,
    ];

    #[test]
    fn every_row_has_the_same_width_so_columns_line_up() {
        let mut busy = facts();
        busy.signals = Signals {
            comments: 12,
            open_threads: OpenThreads::Count(2),
            approvals: 2,
            changes_requested: 1,
            outstanding: 1,
            review_required: true,
        };
        busy.adds = 120;
        busy.dels = 8;
        busy.ci = CiState::Running;
        for f in [&busy, &facts()] {
            let text = flat(&ALL, f);
            assert_eq!(text.chars().count(), total_width(&ALL), "{text:?}");
        }
        assert_eq!(
            flat(&ALL, &busy),
            "✓2  ✕1  ○1   ¶12  2 open   CI running      +120 −8"
        );
    }

    #[test]
    fn a_quiet_change_is_all_blank() {
        assert_eq!(flat(&ALL, &facts()).trim(), "");
    }

    #[test]
    fn review_state_slots_keep_their_columns() {
        let mut f = facts();
        f.signals.changes_requested = 1;
        assert_eq!(flat(&[QueuePiece::Review], &f), "    ✕1     ");
        f.signals.changes_requested = 0;
        f.signals.review_required = true;
        assert_eq!(flat(&[QueuePiece::Review], &f).trim(), "○");
        f.signals.outstanding = 3;
        assert_eq!(flat(&[QueuePiece::Review], &f).trim(), "○3");
    }

    #[test]
    fn open_threads_read_as_a_count_a_flag_or_a_dot() {
        let mut f = facts();
        f.signals.comments = 4;
        f.signals.open_threads = OpenThreads::Count(0);
        assert_eq!(flat(&[QueuePiece::Comments], &f).trim(), "¶4");
        f.signals.open_threads = OpenThreads::Count(2);
        assert_eq!(flat(&[QueuePiece::Comments], &f).trim(), "¶4   2 open");
        f.signals.open_threads = OpenThreads::Any;
        assert_eq!(flat(&[QueuePiece::Comments], &f).trim(), "¶4   open");
        f.signals.open_threads = OpenThreads::Unknown;
        assert_eq!(flat(&[QueuePiece::Comments], &f).trim(), "¶4   ·");
    }

    #[test]
    fn ci_words_stay_quiet_when_the_dot_or_reason_already_says_it() {
        let mut f = facts();
        assert_eq!(flat(&[QueuePiece::Ci], &f).trim(), "");
        f.ci = CiState::Fail;
        assert_eq!(flat(&[QueuePiece::Ci], &f).trim(), "CI failing");
        f.reason_says_ci = true;
        assert_eq!(flat(&[QueuePiece::Ci], &f).trim(), "");
        f.ci = CiState::Running;
        assert_eq!(flat(&[QueuePiece::Ci], &f).trim(), "CI running");
    }

    #[test]
    fn size_falls_back_to_files_and_abbreviates_big_numbers() {
        let mut f = facts();
        f.files = 7;
        assert_eq!(flat(&[QueuePiece::Size], &f).trim(), "7 files");
        f.adds = 12_345;
        f.dels = 1_200;
        assert_eq!(flat(&[QueuePiece::Size], &f).trim(), "+12k −1.2k");
        assert_eq!(short(1_000_000), "999k");
    }

    #[test]
    fn narrow_panes_drop_size_then_ci_then_comments_then_review() {
        let total = total_width(&ALL);
        assert_eq!(plan(&ALL, total), ALL);
        assert_eq!(
            plan(&ALL, total - 1),
            [QueuePiece::Review, QueuePiece::Comments, QueuePiece::Ci]
        );
        let two = total_width(&ALL[..2]);
        assert_eq!(plan(&ALL, two), [QueuePiece::Review, QueuePiece::Comments]);
        assert_eq!(plan(&ALL, width(QueuePiece::Review)), [QueuePiece::Review]);
        assert!(plan(&ALL, 5).is_empty());
    }

    #[test]
    fn chosen_pieces_keep_their_order_and_off_is_empty() {
        let pick = [QueuePiece::Size, QueuePiece::Review];
        assert_eq!(
            plan(&pick, 200),
            [QueuePiece::Review, QueuePiece::Size],
            "priority order, whatever the config order"
        );
        assert!(plan(&[], 200).is_empty());
    }
}
