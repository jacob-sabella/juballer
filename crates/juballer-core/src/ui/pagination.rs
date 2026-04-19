//! Generic pagination primitive + easing helpers.
//!
//! Callers feed a flat [`Vec<T>`] and a page size; [`Paginator`] slices
//! current / neighbour pages, drives `next_page` / `prev_page`, and
//! optionally exposes an eased [`Transition`] the renderer can read
//! to animate a slide or fade between pages.
//!
//! The paginator is render-agnostic — it only tracks state + time.
//! Whether a caller shows the animation, how it shows it (slide, fade,
//! cross-dissolve), and over what duration is up to the consumer.

use std::time::Instant;

/// Direction of the current transition relative to page indices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// `to_page > from_page` — animate in from the right (or whichever
    /// direction "forward" means in the caller's UI).
    Forward,
    /// `to_page < from_page` — animate in from the left.
    Back,
}

/// Active page-change animation.
///
/// `direction` is stored explicitly (not inferred from page indices) because
/// wrap-around navigation produces transitions like `2 → 0` (forward wrap)
/// and `0 → 2` (back wrap) where raw index comparison would give the wrong
/// visual direction for the slide animation.
#[derive(Clone, Copy, Debug)]
pub struct Transition {
    pub from_page: usize,
    pub to_page: usize,
    pub started: Instant,
    pub duration_ms: u32,
    pub direction: Direction,
}

impl Transition {
    /// Raw 0..1 progress; clamps at edges.
    pub fn progress(&self) -> f32 {
        let elapsed_ms = self.started.elapsed().as_secs_f32() * 1000.0;
        (elapsed_ms / self.duration_ms.max(1) as f32).clamp(0.0, 1.0)
    }

    /// Cosine ease-in-out of [`Self::progress`] — smooth curve that
    /// starts and ends at zero velocity. Suits sliding UIs.
    pub fn eased(&self) -> f32 {
        let t = self.progress();
        0.5 - 0.5 * (std::f32::consts::PI * t).cos()
    }

    pub fn is_done(&self) -> bool {
        self.progress() >= 1.0
    }

    pub fn direction(&self) -> Direction {
        self.direction
    }
}

/// Default transition duration in ms when callers don't specify one.
pub const DEFAULT_TRANSITION_MS: u32 = 220;

pub struct Paginator<T> {
    items: Vec<T>,
    per_page: usize,
    current_page: usize,
    transition: Option<Transition>,
}

impl<T> Paginator<T> {
    /// Build a paginator over `items`. `per_page` must be > 0; the
    /// implementation saturates to 1 to avoid divide-by-zero.
    pub fn new(items: Vec<T>, per_page: usize) -> Self {
        Self {
            items,
            per_page: per_page.max(1),
            current_page: 0,
            transition: None,
        }
    }

    pub fn per_page(&self) -> usize {
        self.per_page
    }

    pub fn page_count(&self) -> usize {
        if self.items.is_empty() {
            1
        } else {
            self.items.len().div_ceil(self.per_page)
        }
    }

    pub fn current_page(&self) -> usize {
        self.current_page
    }

    /// Jump to `page` with no transition.
    ///
    /// Out-of-range pages clamp to the last valid page (or 0 if empty). Used for restoring
    /// paginator state from external context (e.g. pre-focusing a saved page on re-entry).
    pub fn jump_to(&mut self, page: usize) {
        let max = self.page_count().saturating_sub(1);
        self.current_page = page.min(max);
        self.transition = None;
    }

    /// Direct read of the underlying flat slice across all pages. Used by callers that need
    /// to search the whole list (e.g. to compute the page+slot for a known item).
    pub fn items(&self) -> &[T] {
        &self.items
    }

    pub fn total(&self) -> usize {
        self.items.len()
    }

    /// Slice of items shown on `page`. Out-of-range returns empty.
    pub fn items_on_page(&self, page: usize) -> &[T] {
        if page >= self.page_count() {
            return &[];
        }
        let start = page * self.per_page;
        let end = (start + self.per_page).min(self.items.len());
        &self.items[start..end]
    }

    pub fn current_items(&self) -> &[T] {
        self.items_on_page(self.current_page)
    }

    /// Start a transition to the next page, wrapping to page 0 from the last page so nav
    /// arrows feel "always armed". No-op when another transition is running or only one page
    /// exists. Returns true if a transition actually started.
    pub fn next_page(&mut self, duration_ms: u32) -> bool {
        if self.transition.is_some() {
            return false;
        }
        let page_count = self.page_count();
        if page_count <= 1 {
            return false;
        }
        let from = self.current_page;
        // Wrap forward: last page → page 0. The UI wants the slide to still
        // read as "forward" (incoming content from the right) so direction
        // is hard-coded rather than inferred from to/from indices.
        let next = if from + 1 >= page_count { 0 } else { from + 1 };
        self.current_page = next;
        // Zero-duration nav skips the animation state entirely so
        // successive calls can run back-to-back (tests and programmatic
        // multi-page jumps rely on this).
        if duration_ms > 0 {
            self.transition = Some(Transition {
                from_page: from,
                to_page: next,
                started: Instant::now(),
                duration_ms,
                direction: Direction::Forward,
            });
        }
        true
    }

    /// Start a transition to the previous page. Wraps from page 0 back to
    /// the last page. No-op only when another transition is running or
    /// the paginator has a single page.
    pub fn prev_page(&mut self, duration_ms: u32) -> bool {
        if self.transition.is_some() {
            return false;
        }
        let page_count = self.page_count();
        if page_count <= 1 {
            return false;
        }
        let from = self.current_page;
        // Wrap back: page 0 → last page. Direction is hard-coded Back so
        // the slide still reads "backward" (incoming content from the left)
        // despite the to-index being numerically larger.
        let prev = if from == 0 { page_count - 1 } else { from - 1 };
        self.current_page = prev;
        if duration_ms > 0 {
            self.transition = Some(Transition {
                from_page: from,
                to_page: prev,
                started: Instant::now(),
                duration_ms,
                direction: Direction::Back,
            });
        }
        true
    }

    /// Active transition, if any.
    pub fn transition(&self) -> Option<&Transition> {
        self.transition.as_ref()
    }

    /// Clear the transition once it's finished. Safe to call every frame;
    /// no-op while the animation is still running.
    pub fn tick(&mut self) {
        if let Some(t) = self.transition.as_ref() {
            if t.is_done() {
                self.transition = None;
            }
        }
    }

    /// True when no transition is running.
    pub fn is_idle(&self) -> bool {
        self.transition.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_count_ceiling() {
        let p: Paginator<i32> = Paginator::new((0..25).collect(), 10);
        assert_eq!(p.page_count(), 3);
    }

    /// 61 entries, 12 per page → 6 pages. NEXT pressed every ~250 ms (just past the 220 ms
    /// transition window) for 7 presses. Expected sequence: 0→1→2→3→4→5→0→1. Mirrors the
    /// caller pattern: `tick()` at top of frame, `next_page(DEFAULT_TRANSITION_MS)` on press.
    #[test]
    fn picker_input_simulation_wraps_at_end() {
        use std::time::Duration;
        let mut p: Paginator<i32> = Paginator::new((0..61).collect(), 12);
        assert_eq!(p.page_count(), 6);

        let mut visited = vec![p.current_page()];
        for _ in 0..7 {
            p.tick();
            std::thread::sleep(Duration::from_millis(DEFAULT_TRANSITION_MS as u64 + 30));
            p.tick();
            let started = p.next_page(DEFAULT_TRANSITION_MS);
            assert!(started, "press blocked unexpectedly");
            visited.push(p.current_page());
        }
        assert_eq!(visited, vec![0, 1, 2, 3, 4, 5, 0, 1], "wrap sequence wrong");
    }

    /// Rapid double-press within the 220 ms transition window: the second press must return
    /// false (busy) but the first press must still have wrapped from the last page to page 0.
    #[test]
    fn picker_rapid_press_blocks_second_call_but_first_still_wraps() {
        let mut p: Paginator<i32> = Paginator::new((0..61).collect(), 12);
        // Hop straight to last page (zero-duration jumps so we don't
        // burn 5 × 220 ms in test time).
        for _ in 0..5 {
            assert!(p.next_page(0));
            p.tick();
        }
        assert_eq!(p.current_page(), 5);

        let started_a = p.next_page(DEFAULT_TRANSITION_MS);
        let started_b = p.next_page(DEFAULT_TRANSITION_MS);
        assert!(started_a, "first wrap press did not start a transition");
        assert!(
            !started_b,
            "second press should be blocked by active transition"
        );
        assert_eq!(
            p.current_page(),
            0,
            "wrap should still have advanced to page 0"
        );
    }

    #[test]
    fn empty_has_one_page() {
        let p: Paginator<i32> = Paginator::new(vec![], 10);
        assert_eq!(p.page_count(), 1);
        assert_eq!(p.current_items().len(), 0);
    }

    #[test]
    fn current_items_slices_correctly() {
        let mut p: Paginator<i32> = Paginator::new((0..25).collect(), 10);
        assert_eq!(p.current_items(), &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
        assert!(p.next_page(0));
        // Clear the zero-duration transition before querying current items.
        p.tick();
        assert_eq!(p.current_items(), &[10, 11, 12, 13, 14, 15, 16, 17, 18, 19]);
        assert!(p.next_page(0));
        p.tick();
        assert_eq!(p.current_items(), &[20, 21, 22, 23, 24]);
    }

    #[test]
    fn next_past_last_page_is_noop_when_single_page() {
        // Single-page paginator has nowhere to wrap to — staying put is the
        // sensible choice so the UI doesn't fake visual movement.
        let mut p: Paginator<i32> = Paginator::new((0..5).collect(), 10);
        assert_eq!(p.page_count(), 1);
        assert!(!p.next_page(0));
        assert_eq!(p.current_page(), 0);
    }

    #[test]
    fn prev_from_first_is_noop_when_single_page() {
        let mut p: Paginator<i32> = Paginator::new((0..5).collect(), 10);
        assert_eq!(p.page_count(), 1);
        assert!(!p.prev_page(0));
        assert_eq!(p.current_page(), 0);
    }

    #[test]
    fn next_page_wraps_from_last_to_first_with_forward_direction() {
        // 3 pages (0..30, 10 per page). Advance to the last page, then
        // `next_page` again should wrap to page 0 and start a Forward
        // transition so the slide animation still reads left→right.
        let mut p: Paginator<i32> = Paginator::new((0..30).collect(), 10);
        assert!(p.next_page(0));
        p.tick();
        assert!(p.next_page(0));
        p.tick();
        assert_eq!(p.current_page(), 2);

        assert!(p.next_page(100));
        assert_eq!(p.current_page(), 0);
        let t = p.transition().expect("wrap should start a transition");
        assert_eq!(t.from_page, 2);
        assert_eq!(t.to_page, 0);
        assert_eq!(t.direction(), Direction::Forward);
    }

    #[test]
    fn prev_page_wraps_from_first_to_last_with_back_direction() {
        // 3 pages. From page 0, `prev_page` wraps to page 2 and should
        // animate as Back so content slides in from the left.
        let mut p: Paginator<i32> = Paginator::new((0..30).collect(), 10);
        assert_eq!(p.current_page(), 0);

        assert!(p.prev_page(100));
        assert_eq!(p.current_page(), 2);
        let t = p.transition().expect("wrap should start a transition");
        assert_eq!(t.from_page, 0);
        assert_eq!(t.to_page, 2);
        assert_eq!(t.direction(), Direction::Back);
    }

    #[test]
    fn wrap_around_is_zero_duration_safe() {
        // Zero-duration wrap jumps without establishing a transition, same
        // as a normal zero-duration nav.
        let mut p: Paginator<i32> = Paginator::new((0..30).collect(), 10);
        assert!(p.prev_page(0));
        assert_eq!(p.current_page(), 2);
        assert!(p.is_idle());
        assert!(p.next_page(0));
        assert_eq!(p.current_page(), 0);
        assert!(p.is_idle());
    }

    #[test]
    fn second_transition_blocked_until_tick_clears_first() {
        let mut p: Paginator<i32> = Paginator::new((0..50).collect(), 10);
        assert!(p.next_page(DEFAULT_TRANSITION_MS));
        // Immediate second attempt rejected — caller should tick() after
        // animation completes before starting another.
        assert!(!p.next_page(DEFAULT_TRANSITION_MS));
    }

    #[test]
    fn direction_forward_vs_back() {
        let mut p: Paginator<i32> = Paginator::new((0..30).collect(), 10);
        p.next_page(100);
        let t = p.transition().unwrap();
        assert_eq!(t.direction(), Direction::Forward);
        p.tick();
        // Give it a moment to complete a zero-duration-variant simulation.
        std::thread::sleep(std::time::Duration::from_millis(120));
        p.tick();
        assert!(p.is_idle());
        p.prev_page(100);
        let t = p.transition().unwrap();
        assert_eq!(t.direction(), Direction::Back);
    }

    #[test]
    fn eased_curve_endpoints() {
        let t = Transition {
            from_page: 0,
            to_page: 1,
            started: Instant::now(),
            duration_ms: 100,
            direction: Direction::Forward,
        };
        // At birth, progress ≈ 0 → eased ≈ 0.
        assert!(t.eased() < 0.02);
        // Simulate completion: duration_ms = 1 and sleep.
        std::thread::sleep(std::time::Duration::from_millis(120));
        // Progress is already clamped to 1.
        assert!((t.eased() - 1.0).abs() < 1e-3);
    }
}
