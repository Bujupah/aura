//! Turns the agent's arrangement — a zone and a size per window, in order —
//! into rectangles that are guaranteed to be on screen and not to overlap.

use crate::topics::{Placement, Size, Zone};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }
}

const MARGIN: f64 = 24.0;
const GAP: f64 = 8.0;
const COLUMN_WIDTH: f64 = 340.0;

pub fn dimensions(size: Size) -> (f64, f64) {
    match size {
        Size::Small => (300.0, 96.0),
        Size::Medium => (300.0, 172.0),
        Size::Large => (COLUMN_WIDTH, 240.0),
        Size::Tall => (COLUMN_WIDTH, 420.0),
    }
}

/// Places `windows` inside `area`, returning one entry per window in the
/// same order.
///
/// Each zone stacks from its corner toward the middle of that edge: top zones
/// downward, bottom zones upward. A stack that would run off the screen or
/// into another window continues in a further column toward the centre.
/// `right_inset` keeps the right-hand zones clear of the status overlay's
/// column.
///
/// `obstacles` are rectangles already taken — windows the seller dragged
/// somewhere themselves — which are left alone and kept clear.
///
/// The agent may ask for more than the screen can hold. A window that fits
/// nowhere at its requested size is tried at each smaller size, and is left
/// out (`None`) only if even the smallest does not fit. Whatever is returned
/// is on screen and overlaps nothing.
pub fn arrange(
    area: Rect,
    right_inset: f64,
    windows: &[Placement],
    obstacles: &[Rect],
) -> Vec<Option<Rect>> {
    let left_edge = area.x + MARGIN;
    let right_edge = area.right() - MARGIN - right_inset;
    let top_edge = area.y + MARGIN;
    let bottom_edge = area.bottom() - MARGIN;

    // Obstacles are windows the seller has placed by hand: they are never
    // moved, and nothing is placed over them.
    let mut placed: Vec<Rect> = obstacles.to_vec();
    let mut rects = vec![None; windows.len()];

    for (index, placement) in windows.iter().enumerate() {
        let top = matches!(placement.zone, Zone::TopLeft | Zone::TopRight);
        let right = matches!(placement.zone, Zone::TopRight | Zone::BottomRight);

        'sizes: for size in sizes_down_from(placement.size) {
            let (width, height) = dimensions(size);
            for column in 0.. {
                let shift = column as f64 * (COLUMN_WIDTH + GAP);
                let x = if right { right_edge - shift - width } else { left_edge + shift };
                if x < left_edge || x + width > right_edge {
                    break;
                }
                // Nearest free spot to the zone's corner: the corner itself,
                // or just past any window already in this column.
                let in_column = placed.iter().filter(|other| other.x < x + width && x < other.right());
                let mut candidates: Vec<f64> = if top {
                    std::iter::once(top_edge).chain(in_column.map(|other| other.bottom() + GAP)).collect()
                } else {
                    std::iter::once(bottom_edge - height)
                        .chain(in_column.map(|other| other.y - GAP - height))
                        .collect()
                };
                candidates.sort_by(|a, b| if top { a.total_cmp(b) } else { b.total_cmp(a) });

                let spot = candidates
                    .into_iter()
                    .map(|y| Rect { x, y, width, height })
                    .find(|candidate| {
                        candidate.y >= top_edge
                            && candidate.bottom() <= bottom_edge
                            && placed.iter().all(|other| apart(candidate, other))
                    });
                if let Some(rect) = spot {
                    placed.push(rect);
                    rects[index] = Some(rect);
                    break 'sizes;
                }
            }
        }
    }
    rects
}

/// The requested size, then every smaller one.
fn sizes_down_from(size: Size) -> impl Iterator<Item = Size> {
    const DESCENDING: [Size; 4] = [Size::Tall, Size::Large, Size::Medium, Size::Small];
    let start = DESCENDING.iter().position(|&candidate| candidate == size).unwrap_or(0);
    DESCENDING.into_iter().skip(start)
}

/// Whether two windows are separated by at least the gap, in some direction.
fn apart(a: &Rect, b: &Rect) -> bool {
    a.right() + GAP <= b.x || b.right() + GAP <= a.x || a.bottom() + GAP <= b.y || b.bottom() + GAP <= a.y
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topics::MAX_WINDOWS;

    const SCREEN: Rect = Rect { x: 0.0, y: 25.0, width: 1512.0, height: 920.0 };
    const INSET: f64 = 352.0;

    fn at(zone: Zone, size: Size) -> Placement {
        Placement { zone, size }
    }

    fn place(area: Rect, inset: f64, windows: &[Placement]) -> Vec<Option<Rect>> {
        arrange(area, inset, windows, &[])
    }

    fn overlap(a: &Rect, b: &Rect) -> bool {
        a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
    }

    fn all(rects: Vec<Option<Rect>>) -> Vec<Rect> {
        rects.into_iter().map(|rect| rect.expect("every window should fit")).collect()
    }

    fn assert_tidy(area: Rect, rects: &[Option<Rect>]) {
        let rects: Vec<Rect> = rects.iter().flatten().copied().collect();
        for (i, rect) in rects.iter().enumerate() {
            assert!(
                rect.x >= area.x && rect.right() <= area.right() && rect.y >= area.y && rect.bottom() <= area.bottom(),
                "window {i} is off screen: {rect:?}"
            );
            for (j, other) in rects.iter().enumerate().skip(i + 1) {
                assert!(!overlap(rect, other), "windows {i} and {j} overlap: {rect:?} {other:?}");
            }
        }
    }

    #[test]
    fn a_top_stack_runs_downward_from_its_corner_in_the_given_order() {
        let rects = all(place(SCREEN, INSET, &[at(Zone::TopLeft, Size::Small), at(Zone::TopLeft, Size::Medium)]));
        assert_eq!(rects[0], Rect { x: 24.0, y: 49.0, width: 300.0, height: 96.0 });
        assert_eq!((rects[1].x, rects[1].y), (24.0, 49.0 + 96.0 + GAP));
    }

    #[test]
    fn a_bottom_stack_runs_upward_from_its_corner() {
        let rects = all(place(SCREEN, INSET, &[at(Zone::BottomLeft, Size::Small), at(Zone::BottomLeft, Size::Small)]));
        assert_eq!(rects[0].bottom(), SCREEN.bottom() - 24.0);
        assert_eq!(rects[1].bottom(), rects[0].y - GAP);
    }

    #[test]
    fn right_hand_zones_stay_clear_of_the_overlay_column() {
        let rects = all(place(SCREEN, INSET, &[at(Zone::TopRight, Size::Large), at(Zone::BottomRight, Size::Small)]));
        for rect in rects {
            assert!(rect.right() <= SCREEN.right() - 24.0 - INSET);
        }
    }

    #[test]
    fn a_full_column_continues_in_the_next_one_toward_the_centre() {
        let windows = vec![at(Zone::TopLeft, Size::Large); 5];
        let rects = place(SCREEN, INSET, &windows);
        assert_tidy(SCREEN, &rects);
        let rects = all(rects);
        assert!(rects[4].x > rects[0].x, "the fifth large window should wrap");
    }

    #[test]
    fn opposite_stacks_on_one_side_never_collide() {
        let mut windows = vec![at(Zone::TopRight, Size::Large); 3];
        windows.extend(vec![at(Zone::BottomRight, Size::Large); 3]);
        assert_tidy(SCREEN, &place(SCREEN, INSET, &windows));
    }

    #[test]
    fn every_arrangement_the_agent_can_ask_for_is_on_screen_without_overlap() {
        let zones = [Zone::TopLeft, Zone::TopRight, Zone::BottomLeft, Zone::BottomRight];
        let sizes = [Size::Small, Size::Medium, Size::Large, Size::Tall];
        // A deterministic sweep over many mixed arrangements at the limit.
        for seed in 0..500usize {
            let windows: Vec<Placement> = (0..MAX_WINDOWS)
                .map(|i| {
                    let n = seed.wrapping_mul(31).wrapping_add(i * 7 + seed / (i + 1));
                    at(zones[n % 4], sizes[(n / 4) % 4])
                })
                .collect();
            let rects = place(SCREEN, INSET, &windows);
            assert_eq!(rects.len(), MAX_WINDOWS);
            assert_tidy(SCREEN, &rects);
            // A full set of mixed sizes fits a laptop screen with at most
            // the odd window shrunk or left out, never most of them.
            assert!(rects.iter().flatten().count() >= MAX_WINDOWS - 1, "seed {seed} dropped too many");
        }
        // The worst cases: everything large, or everything tall, in one corner.
        assert_tidy(SCREEN, &place(SCREEN, INSET, &[at(Zone::BottomRight, Size::Large); MAX_WINDOWS]));
        assert_tidy(SCREEN, &place(SCREEN, INSET, &[at(Zone::TopLeft, Size::Tall); 6]));
    }

    #[test]
    fn a_window_that_does_not_fit_is_shrunk_before_it_is_left_out() {
        // Room for one column of two tall windows and little else.
        let cramped = Rect { x: 0.0, y: 0.0, width: 340.0 + 2.0 * 24.0, height: 920.0 };
        let rects = place(cramped, 0.0, &[at(Zone::TopLeft, Size::Tall); 4]);
        assert_tidy(cramped, &rects);
        let heights: Vec<Option<f64>> = rects.iter().map(|rect| rect.map(|rect| rect.height)).collect();
        assert_eq!(heights, [Some(420.0), Some(420.0), None, None]);

        let roomier = Rect { height: 1100.0, ..cramped };
        let rects = place(roomier, 0.0, &[at(Zone::TopLeft, Size::Tall); 3]);
        assert_tidy(roomier, &rects);
        assert_eq!(rects[2].map(|rect| rect.height), Some(172.0), "the third should shrink to fit");
    }

    #[test]
    fn windows_the_seller_placed_by_hand_are_kept_clear() {
        // The seller dragged a window into the top-left corner.
        let pinned = Rect { x: 24.0, y: 49.0, width: 300.0, height: 172.0 };
        let rects = arrange(SCREEN, INSET, &[at(Zone::TopLeft, Size::Medium), at(Zone::TopLeft, Size::Small)], &[pinned]);
        for rect in rects.iter().flatten() {
            assert!(!overlap(rect, &pinned), "{rect:?} covers the pinned window");
        }
        assert_tidy(SCREEN, &rects);
        // The corner is taken, so the stack starts just below it.
        assert_eq!(rects[0].unwrap().y, pinned.bottom() + GAP);
    }
}
