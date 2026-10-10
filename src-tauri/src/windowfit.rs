//! Keeping the window on a screen that can hold it.
//!
//! Two ways it ends up somewhere nobody can reach. The configured size is
//! 1800x1130, which is larger than a 13-inch laptop's usable area, so a first
//! run on a small screen opens with the bottom and the right edge past it. And
//! the window-state plugin restores wherever it was last, which is a monitor
//! that may not be plugged in any more — a window at x=2560 on a single
//! built-in display is a window nobody can drag back.
//!
//! Pure arithmetic, so it can be checked without a screen. Physical pixels
//! throughout: that is what Tauri reports for both the window and the monitor,
//! and mixing them with logical ones is off by the scale factor.

/// A rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self { x, y, width, height }
    }
}

/// The window moved and shrunk as little as it takes to sit inside `area`.
///
/// Shrink before move: a window wider than the screen cannot be placed inside
/// it at any position, so the size is settled first and the position is then
/// clamped against what is left. A window that already fits is returned
/// unchanged, so the ordinary case moves nothing.
#[must_use]
pub fn fit_within(window: Rect, area: Rect) -> Rect {
    let width = window.width.min(area.width);
    let height = window.height.min(area.height);

    // Where the far edge may start. `width` is already no wider than the area,
    // so this cannot land left of it.
    let last_x = area.x.saturating_add(i32::try_from(area.width.saturating_sub(width)).unwrap_or(0));
    let last_y =
        area.y.saturating_add(i32::try_from(area.height.saturating_sub(height)).unwrap_or(0));

    Rect {
        x: window.x.clamp(area.x, last_x),
        y: window.y.clamp(area.y, last_y),
        width,
        height,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, reason = "test assertions")]
mod tests {
    use super::*;

    /// A 1440x900 laptop, less the menu bar: what `work_area` reports.
    const LAPTOP: Rect = Rect::new(0, 25, 1440, 875);

    #[test]
    fn a_window_that_already_fits_is_left_alone() {
        let window = Rect::new(100, 100, 800, 600);
        assert_eq!(fit_within(window, LAPTOP), window);
    }

    #[test]
    fn a_window_larger_than_the_screen_is_shrunk_to_it() {
        // The configured size, on a 13-inch laptop.
        let fitted = fit_within(Rect::new(0, 25, 1800, 1130), LAPTOP);
        assert_eq!(fitted, Rect::new(0, 25, 1440, 875));
    }

    #[test]
    fn a_window_hanging_off_the_right_is_moved_back_not_shrunk() {
        let fitted = fit_within(Rect::new(1200, 100, 800, 600), LAPTOP);
        assert_eq!(fitted.width, 800, "it fits, so its size is nobody's business");
        assert_eq!(fitted.x, 640, "and its right edge lands on the screen's");
        assert_eq!(fitted.x + 800, LAPTOP.x + 1440);
    }

    #[test]
    fn a_window_above_the_work_area_is_pulled_under_the_menu_bar() {
        // Restored from a screen with no menu bar to one with.
        let fitted = fit_within(Rect::new(100, 0, 800, 600), LAPTOP);
        assert_eq!(fitted.y, 25);
    }

    #[test]
    fn a_window_on_a_monitor_that_is_gone_comes_home() {
        // x=2560 is the second display that is no longer plugged in.
        let fitted = fit_within(Rect::new(2560, 400, 1200, 800), LAPTOP);
        assert!(fitted.x >= LAPTOP.x, "at {}", fitted.x);
        assert!(fitted.x + i32::try_from(fitted.width).unwrap() <= LAPTOP.x + 1440);
        assert!(fitted.y + i32::try_from(fitted.height).unwrap() <= LAPTOP.y + 875);
    }

    #[test]
    fn a_monitor_left_of_the_primary_keeps_its_negative_coordinates() {
        // A second display to the left is a negative origin, and a window that
        // fits there must be left where it is rather than dragged to zero.
        let left = Rect::new(-1920, 0, 1920, 1080);
        let window = Rect::new(-1800, 100, 1200, 800);
        assert_eq!(fit_within(window, left), window);
    }

    #[test]
    fn the_configured_size_centred_on_a_13_inch_retina_screen_fills_its_work_area() {
        // The geometry logged in #247 on macOS 11.7: 1800x1131 points centred
        // on a 1280x800-point Retina screen, all in physical pixels at 2x. The
        // work area is the screen less a 25-point menu bar. Both sides are
        // physical, so the result is the whole work area, 1280x775 points.
        let screen = Rect::new(0, 50, 2560, 1550);
        let fitted = fit_within(Rect::new(-520, -306, 3600, 2262), screen);
        assert_eq!(fitted, screen);
    }

    #[test]
    fn the_result_always_fits(){
        // Whatever it is handed, including sizes and positions that make no
        // sense, what comes back is inside the area.
        for window in [
            Rect::new(-9000, -9000, 10_000, 10_000),
            Rect::new(9000, 9000, 1, 1),
            Rect::new(0, 0, 0, 0),
        ] {
            let fitted = fit_within(window, LAPTOP);
            assert!(fitted.x >= LAPTOP.x && fitted.y >= LAPTOP.y, "{fitted:?}");
            assert!(fitted.width <= LAPTOP.width && fitted.height <= LAPTOP.height, "{fitted:?}");
            let right = fitted.x + i32::try_from(fitted.width).unwrap();
            let bottom = fitted.y + i32::try_from(fitted.height).unwrap();
            assert!(right <= LAPTOP.x + 1440 && bottom <= LAPTOP.y + 875, "{fitted:?}");
        }
    }
}
