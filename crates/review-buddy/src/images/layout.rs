//! How big an image is drawn in the description, in terminal cells.

/// The widest an image gets, as a share of the pane.
pub const WIDTH_PERCENT: u32 = 60;
/// The tallest an image gets, in rows.
pub const MAX_ROWS: u16 = 14;
/// Panes narrower than this give the image the whole width.
const NARROW: u16 = 20;

/// Cells `(columns, rows)` for an image of `pixels` on a terminal whose cells are `font` pixels.
///
/// An image keeps its aspect ratio, is never drawn larger than its natural size (or the size its
/// `width` and `height` attributes ask for), and is shrunk to fit within [`WIDTH_PERCENT`] of
/// `pane_width` and [`MAX_ROWS`] rows.
pub fn fit(
    pixels: (u32, u32),
    asked: (Option<u32>, Option<u32>),
    font: (u16, u16),
    pane_width: u16,
) -> (u16, u16) {
    let (iw, ih) = (f64::from(pixels.0.max(1)), f64::from(pixels.1.max(1)));
    let (fw, fh) = (f64::from(font.0.max(1)), f64::from(font.1.max(1)));
    let (dw, dh) = match asked {
        (Some(w), Some(h)) => (f64::from(w), f64::from(h)),
        (Some(w), None) => (f64::from(w), f64::from(w) * ih / iw),
        (None, Some(h)) => (f64::from(h) * iw / ih, f64::from(h)),
        (None, None) => (iw, ih),
    };
    let max_cols = if pane_width < NARROW {
        pane_width.max(1)
    } else {
        u16::try_from(u32::from(pane_width) * WIDTH_PERCENT / 100).unwrap_or(u16::MAX)
    };
    let scale = (f64::from(max_cols) * fw / dw)
        .min(f64::from(MAX_ROWS) * fh / dh)
        .min(1.0);
    let cols = ((dw * scale / fw).ceil() as u16).clamp(1, max_cols);
    let rows = ((dh * scale / fh).ceil() as u16).clamp(1, MAX_ROWS);
    (cols, rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FONT: (u16, u16) = (10, 20);

    #[test]
    fn small_images_keep_their_natural_size() {
        assert_eq!(fit((100, 40), (None, None), FONT, 100), (10, 2));
        assert_eq!(fit((1, 1), (None, None), FONT, 100), (1, 1));
    }

    #[test]
    fn wide_images_shrink_to_sixty_percent_of_the_pane() {
        let (cols, rows) = fit((1600, 400), (None, None), FONT, 100);
        assert_eq!(cols, 60);
        assert_eq!(
            rows, 8,
            "400px wide per 1600 keeps the 4:1 ratio: 60 cols are 600px, 150px tall"
        );
    }

    #[test]
    fn tall_images_are_capped_in_rows_and_keep_their_ratio() {
        let (cols, rows) = fit((400, 4000), (None, None), FONT, 100);
        assert_eq!(rows, MAX_ROWS);
        assert!(cols <= 3, "{cols}");
    }

    #[test]
    fn width_and_height_attributes_set_the_size_they_ask_for() {
        assert_eq!(fit((1000, 500), (Some(200), None), FONT, 100), (20, 5));
        assert_eq!(fit((1000, 500), (None, Some(100)), FONT, 100), (20, 5));
        assert_eq!(fit((1000, 500), (Some(300), Some(60)), FONT, 100), (30, 3));
        assert!(
            fit((1000, 500), (Some(900), None), FONT, 100).0 <= 60,
            "still capped"
        );
    }

    #[test]
    fn narrow_panes_give_the_image_the_whole_width() {
        assert_eq!(fit((1000, 100), (None, None), FONT, 16).0, 16);
        assert_eq!(fit((1000, 100), (None, None), FONT, 0).0, 1);
    }

    #[test]
    fn the_result_always_fits_the_limits() {
        for (w, h) in [(1, 5000), (5000, 1), (333, 777), (4096, 4096)] {
            for pane in [1u16, 10, 19, 20, 40, 120] {
                let (cols, rows) = fit((w, h), (None, None), FONT, pane);
                assert!(
                    cols >= 1 && (1..=MAX_ROWS).contains(&rows),
                    "{w}x{h} in {pane}: {cols}x{rows}"
                );
                let limit = if pane < NARROW {
                    pane.max(1)
                } else {
                    (u32::from(pane) * WIDTH_PERCENT / 100) as u16
                };
                assert!(cols <= limit, "{w}x{h} in {pane}: {cols}x{rows}");
            }
        }
    }
}
