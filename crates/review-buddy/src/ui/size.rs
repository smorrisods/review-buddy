//! The minimum terminal size and the notice shown below it.

pub const MIN_WIDTH: u16 = 100;
pub const MIN_HEIGHT: u16 = 30;

pub fn is_too_small(width: u16, height: u16) -> bool {
    width < MIN_WIDTH || height < MIN_HEIGHT
}

/// The headline, worded for whichever dimension falls short.
pub fn headline(width: u16, height: u16) -> &'static str {
    match (width < MIN_WIDTH, height < MIN_HEIGHT) {
        (true, true) => "Make me a little bigger",
        (true, false) => "Make me a little wider",
        _ => "Make me a little taller",
    }
}

pub fn detail(width: u16, height: u16) -> String {
    format!(
        "Review Buddy needs at least {MIN_WIDTH}×{MIN_HEIGHT}. This window is {width}×{height}."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundary_is_inclusive() {
        assert!(!is_too_small(MIN_WIDTH, MIN_HEIGHT));
        assert!(is_too_small(MIN_WIDTH - 1, MIN_HEIGHT));
        assert!(is_too_small(MIN_WIDTH, MIN_HEIGHT - 1));
        assert!(is_too_small(0, 0));
    }

    #[test]
    fn headline_names_the_short_dimension() {
        assert!(headline(80, 40).contains("wider"));
        assert!(headline(160, 20).contains("taller"));
        assert!(headline(80, 20).contains("bigger"));
    }

    #[test]
    fn detail_reports_both_sizes() {
        let d = detail(80, 24);
        assert!(d.contains("100×30") && d.contains("80×24"));
    }
}
