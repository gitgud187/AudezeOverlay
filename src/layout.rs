use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Card {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub visible: bool,
}
impl Card {
    pub fn overlaps(self, o: Self) -> bool {
        self.x < o.x + o.w && self.x + self.w > o.x && self.y < o.y + o.h && self.y + self.h > o.y
    }
}
pub const NAMES: [&str; 6] = [
    "Game / Chat Mix",
    "Sidetone",
    "Noise cancellation",
    "AI noise removal",
    "Device status",
    "Equalizer",
];
pub fn defaults() -> [Card; 6] {
    [
        card(0, 0, 6, 12),
        card(6, 0, 6, 4),
        card(6, 4, 6, 4),
        card(6, 8, 6, 4),
        card(0, 12, 12, 3),
        Card {
            visible: false,
            ..card(0, 15, 6, 5)
        },
    ]
}
fn card(x: i32, y: i32, w: i32, h: i32) -> Card {
    Card {
        x,
        y,
        w,
        h,
        visible: true,
    }
}
pub fn minimum(_i: usize) -> (i32, i32) {
    // A short mix card uses its slider presentation instead of clipping a dial.
    (5, 3)
}

/// Grow a card at its origin and push intersecting neighbours in the direction
/// of growth. Each pass only moves forward, so six cards converge in at most
/// six passes. Failure leaves the original layout intact.
pub fn resize_pushing(
    cards: &mut [Card; 6],
    i: usize,
    proposed: Card,
    cols: i32,
    rows: i32,
) -> bool {
    if i >= cards.len() || !cards[i].visible || !valid_in(cards, cols, rows) {
        return false;
    }
    let mut next = *cards;
    let old = next[i];
    next[i].w = proposed.w.clamp(minimum(i).0, cols - old.x);
    next[i].h = proposed.h.clamp(minimum(i).1, rows - old.y);
    let horizontal = next[i].w > old.w && next[i].h <= old.h;
    let mut settled = [false; 6];
    settled[i] = true;
    for _ in 0..6 {
        let mut progress = false;
        for a in 0..6 {
            if !settled[a] || !next[a].visible {
                continue;
            }
            for b in 0..6 {
                if a == b || settled[b] || !next[b].visible || !next[a].overlaps(next[b]) {
                    continue;
                }
                if horizontal {
                    next[b].x = next[a].x + next[a].w;
                } else {
                    next[b].y = next[a].y + next[a].h;
                }
                progress = true;
            }
        }
        for b in 0..6 {
            if b != i && next[b] != cards[b] {
                settled[b] = true;
            }
        }
        // Resolve collisions between displaced cards in their original order.
        for a in 0..6 {
            for b in a + 1..6 {
                if !next[a].visible || !next[b].visible || !next[a].overlaps(next[b]) {
                    continue;
                }
                let (front, back) = if b == i
                    || (a != i
                        && if horizontal {
                            cards[b].x < cards[a].x
                        } else {
                            cards[b].y < cards[a].y
                        }) {
                    (b, a)
                } else {
                    (a, b)
                };
                if horizontal {
                    next[back].x = next[front].x + next[front].w;
                } else {
                    next[back].y = next[front].y + next[front].h;
                }
                settled[back] = true;
                progress = true;
            }
        }
        if valid_in(&next, cols, rows) {
            *cards = next;
            return true;
        }
        if !progress {
            break;
        }
    }
    false
}
/// Place on the nearest unoccupied grid cells; never silently overlap controls.
pub fn place(cards: &mut [Card; 6], i: usize, proposed: Card, rows: i32) -> bool {
    place_in(cards, i, proposed, 12, rows)
}
pub fn place_in(cards: &mut [Card; 6], i: usize, mut proposed: Card, cols: i32, rows: i32) -> bool {
    if i >= cards.len() || rows < minimum(i).1 {
        return false;
    }
    let (mw, mh) = minimum(i);
    if cols < mw {
        return false;
    }
    proposed.w = proposed.w.clamp(mw, cols);
    proposed.h = proposed.h.clamp(mh, rows);
    proposed.x = proposed.x.clamp(0, cols - proposed.w);
    proposed.y = proposed.y.clamp(0, rows - proposed.h);
    let mut candidates = vec![];
    for y in 0..=rows - proposed.h {
        for x in 0..=cols - proposed.w {
            candidates.push(((x - proposed.x).pow(2) + (y - proposed.y).pow(2), x, y));
        }
    }
    candidates.sort();
    for (_, x, y) in candidates {
        let c = Card { x, y, ..proposed };
        if cards
            .iter()
            .enumerate()
            .all(|(j, o)| j == i || !o.visible || !c.overlaps(*o))
        {
            cards[i] = c;
            return true;
        }
    }
    false
}

/// Preview a move against the layout captured at mouse-down. Reusing that snapshot
/// keeps a hovered swap stable instead of swapping the same two cards every frame.
/// `pointer` is in grid coordinates, so the card under the cursor is the drop slot.
pub fn preview_move(
    original: &[Card; 6],
    i: usize,
    proposed: Card,
    pointer: (f32, f32),
    rows: i32,
) -> Option<[Card; 6]> {
    preview_move_in(original, i, proposed, pointer, 12, rows)
}
pub fn preview_move_in(
    original: &[Card; 6],
    i: usize,
    mut proposed: Card,
    pointer: (f32, f32),
    cols: i32,
    rows: i32,
) -> Option<[Card; 6]> {
    if i >= original.len()
        || !original[i].visible
        || !valid_in(original, cols, rows)
        || !pointer.0.is_finite()
        || !pointer.1.is_finite()
    {
        return None;
    }
    if proposed.x == original[i].x && proposed.y == original[i].y {
        return Some(*original);
    }
    let target = original.iter().enumerate().find_map(|(j, c)| {
        (j != i
            && c.visible
            && pointer.0 >= c.x as f32
            && pointer.0 < (c.x + c.w) as f32
            && pointer.1 >= c.y as f32
            && pointer.1 < (c.y + c.h) as f32)
            .then_some(j)
    });
    if let Some(j) = target {
        return swap_into_slot(original, i, j, cols, rows);
    }

    // Moving preserves the dimensions chosen by the user. Only an occupied
    // destination can compact a card; free space is not an instruction to grow.
    let mut result = *original;
    proposed.w = original[i].w;
    proposed.h = original[i].h;
    place_in(&mut result, i, proposed, cols, rows).then_some(result)
}

#[cfg(test)]
fn valid(cards: &[Card; 6], rows: i32) -> bool {
    valid_in(cards, 12, rows)
}
pub fn valid_in(cards: &[Card; 6], cols: i32, rows: i32) -> bool {
    cards.iter().enumerate().all(|(i, c)| {
        !c.visible
            || (c.x >= 0
                && c.y >= 0
                && c.w >= minimum(i).0
                && c.h >= minimum(i).1
                && c.x + c.w <= cols
                && c.y + c.h <= rows
                && cards[i + 1..].iter().all(|o| !o.visible || !c.overlaps(*o)))
    })
}

fn in_slot(c: Card, i: usize, slot: Card, cols: i32, rows: i32) -> Card {
    let (min_w, min_h) = minimum(i);
    // Swapping a full dial retains room for its circular presentation. Cards
    // explicitly resized to a slider remain short when moved between slots.
    let min_h = if i == 0 && c.h >= 8 { 8 } else { min_h };
    let w = c.w.min(slot.w).max(min_w).min(cols);
    let h = c.h.min(slot.h).max(min_h).min(rows);
    Card {
        x: slot.x.clamp(0, cols - w),
        y: slot.y.clamp(0, rows - h),
        w,
        h,
        ..c
    }
}

fn swap_into_slot(
    original: &[Card; 6],
    i: usize,
    j: usize,
    cols: i32,
    rows: i32,
) -> Option<[Card; 6]> {
    let destination = in_slot(original[i], i, original[j], cols, rows);
    let replacement = in_slot(original[j], j, original[i], cols, rows);
    let mut preferred = *original;
    preferred[j] = replacement;
    let mut moving = vec![j];
    for (k, c) in original.iter().enumerate() {
        if k == i || k == j || !c.visible {
            continue;
        }
        // A tall mix card may need two small slots. Move only the neighbors
        // covered by those slots into the space the dragged card vacated.
        let shift = if c.overlaps(destination) {
            Some((original[i].x - original[j].x, original[i].y - original[j].y))
        } else if c.overlaps(replacement) {
            Some((original[j].x - original[i].x, original[j].y - original[i].y))
        } else {
            None
        };
        if let Some((dx, dy)) = shift {
            moving.push(k);
            preferred[k].x += dx;
            preferred[k].y += dy;
        }
    }
    let mut result = *original;
    result[i] = destination;
    let mut fixed = [true; 6];
    for &k in &moving {
        fixed[k] = false;
    }
    if result
        .iter()
        .enumerate()
        .any(|(k, c)| k != i && fixed[k] && c.visible && c.overlaps(destination))
    {
        return None;
    }
    let candidates: Vec<_> = moving
        .iter()
        .map(|&k| placement_candidates(preferred[k], k, cols, rows))
        .collect();
    // Only six cards exist. Bound speculative packing work so pointer movement
    // cannot turn a crowded layout into an expensive search on the UI thread.
    let mut budget = 12_000;
    if pack(
        &mut result,
        &mut fixed,
        &moving,
        &candidates,
        0,
        &mut budget,
    ) && valid_in(&result, cols, rows)
    {
        Some(result)
    } else {
        None
    }
}

fn placement_candidates(preferred: Card, i: usize, cols: i32, rows: i32) -> Vec<Card> {
    let (min_w, min_h) = minimum(i);
    let mut ranked = Vec::new();
    // Try the destination's useful dimensions first, then compact sizes. A card
    // is never made shorter than the renderer can display its controls.
    for (w, h) in [
        (preferred.w, preferred.h),
        (preferred.w, min_h),
        (min_w, preferred.h),
        (min_w, min_h),
    ] {
        for y in 0..=rows - h {
            for x in 0..=cols - w {
                let size_cost = (preferred.w - w).abs() + (preferred.h - h).abs();
                let score =
                    size_cost * 10_000 + (preferred.x - x).pow(2) + (preferred.y - y).pow(2);
                ranked.push((score, y, x, w, h));
            }
        }
    }
    ranked.sort_unstable();
    ranked.dedup();
    ranked
        .into_iter()
        .map(|(_, y, x, w, h)| Card {
            x,
            y,
            w,
            h,
            ..preferred
        })
        .collect()
}

fn pack(
    result: &mut [Card; 6],
    fixed: &mut [bool; 6],
    moving: &[usize],
    candidates: &[Vec<Card>],
    depth: usize,
    budget: &mut usize,
) -> bool {
    if depth == moving.len() {
        return true;
    }
    let i = moving[depth];
    for &c in &candidates[depth] {
        if *budget == 0 {
            return false;
        }
        *budget -= 1;
        if result
            .iter()
            .enumerate()
            .any(|(j, o)| fixed[j] && o.visible && c.overlaps(*o))
        {
            continue;
        }
        result[i] = c;
        fixed[i] = true;
        if pack(result, fixed, moving, candidates, depth + 1, budget) {
            return true;
        }
        fixed[i] = false;
    }
    false
}
/// Resize at the existing origin, stopping at neighbors instead of moving the card.
pub fn resize_in(cards: &mut [Card; 6], i: usize, proposed: Card, cols: i32, rows: i32) -> bool {
    if i >= cards.len() {
        return false;
    }
    let c = cards[i];
    let (mw, mh) = minimum(i);
    if cols - c.x < mw || rows - c.y < mh {
        return false;
    }
    let wanted_w = proposed.w.clamp(mw, cols - c.x);
    let wanted_h = proposed.h.clamp(mh, rows - c.y);
    let mut sizes = vec![];
    for h in mh..=wanted_h {
        for w in mw..=wanted_w {
            sizes.push(((wanted_w - w).pow(2) + (wanted_h - h).pow(2), w, h));
        }
    }
    sizes.sort_unstable();
    for (_, w, h) in sizes {
        let resized = Card { w, h, ..c };
        if cards
            .iter()
            .enumerate()
            .all(|(j, o)| j == i || !o.visible || !resized.overlaps(*o))
        {
            cards[i] = resized;
            return true;
        }
    }
    false
}
pub fn extent(cards: &[Card; 6]) -> (i32, i32) {
    cards
        .iter()
        .filter(|c| c.visible)
        .fold((0, 0), |(w, h), c| (w.max(c.x + c.w), h.max(c.y + c.h)))
}
/// Remove vertical gaps without changing widths, heights, or column choices.
pub fn compact(cards: &mut [Card; 6]) {
    let mut order: Vec<_> = (0..6).filter(|&i| cards[i].visible).collect();
    order.sort_by_key(|&i| (cards[i].y, cards[i].x, i));
    for i in order {
        for y in 0..cards[i].y {
            let candidate = Card { y, ..cards[i] };
            if cards
                .iter()
                .enumerate()
                .all(|(j, c)| j == i || !c.visible || !candidate.overlaps(*c))
            {
                cards[i] = candidate;
                break;
            }
        }
    }
}
/// Re-add a hidden card, extending the canvas if there is no existing free slot.
pub fn restore(cards: &mut [Card; 6], i: usize, cols: i32, rows: i32) -> Option<i32> {
    if i >= cards.len() || cols < minimum(i).0 {
        return None;
    }
    let mut proposed = cards[i];
    proposed.visible = true;
    proposed.w = proposed.w.clamp(minimum(i).0, cols);
    proposed.h = proposed.h.max(minimum(i).1).min(40);
    let extra = extent(cards).1 + proposed.h;
    let grown = rows.max(extra).min(80);
    [rows, grown]
        .into_iter()
        .find(|&available| place_in(cards, i, proposed, cols, available))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn growing_pushes_neighbours_and_rejects_an_impossible_extent_atomically() {
        let mut cards = defaults();
        let before = cards;
        let grow = Card { h: 7, ..cards[1] };
        assert!(resize_pushing(&mut cards, 1, grow, 12, 24));
        assert_eq!(cards[1], grow);
        assert_eq!(cards[2].y, 7);
        assert_eq!(cards[3].y, 11);
        assert_eq!(cards[4].y, 15);
        assert!(valid_in(&cards, 12, 24));
        let mut crowded = before;
        assert!(!resize_pushing(&mut crowded, 1, grow, 12, 15));
        assert_eq!(crowded, before);
        let mut wide = before;
        let grow = Card { w: 8, ..wide[0] };
        assert!(resize_pushing(&mut wide, 0, grow, 20, 20));
        assert_eq!(wide[1].x, 8);
        assert_eq!(wide[2].x, 8);
        assert!(valid_in(&wide, 20, 20));
    }
    #[test]
    fn minimal_mix_resizes_and_moves_without_regrowing() {
        let mut cards = defaults();
        let small = Card { h: 3, ..cards[0] };
        assert!(resize_pushing(&mut cards, 0, small, 12, 15));
        let moved = preview_move_in(
            &cards,
            0,
            Card {
                x: 6,
                y: 8,
                ..small
            },
            (8., 9.),
            12,
            15,
        )
        .unwrap();
        assert_eq!(moved[0].h, 3);
        assert!(valid_in(&moved, 12, 15));
    }
    #[test]
    fn moves_and_resizes_without_overlap() {
        let mut c = defaults();
        assert!(place(&mut c, 1, card(0, 15, 12, 4), 24));
        assert_eq!(c[1].y, 15);
        assert!(!c[1].overlaps(c[0]));
        let before = c;
        assert!(!place(&mut c, 0, card(0, 0, 12, 24), 24));
        assert_eq!(before, c);
    }

    #[test]
    fn equal_cards_exchange_slots_and_preview_is_stable() {
        let c = defaults();
        let proposed = Card { y: 4, ..c[1] };
        let result = preview_move(&c, 1, proposed, (9., 6.), 15).unwrap();
        assert_eq!(result[1], c[2]);
        assert_eq!(result[2], c[1]);
        assert_eq!(result[0], c[0]);
        assert_eq!(Some(result), preview_move(&c, 1, proposed, (9., 6.), 15));
        assert!(valid(&result, 15));
    }

    #[test]
    fn tall_card_adapts_and_displaced_neighbor_uses_vacated_space() {
        let c = defaults();
        let result = preview_move(&c, 0, Card { x: 6, ..c[0] }, (9., 2.), 15).unwrap();
        assert_eq!(result[0], card(6, 0, 6, 8));
        assert_eq!(result[1], card(0, 0, 6, 4));
        assert_eq!(result[2], card(0, 4, 6, 4));
        assert_eq!(result[3], c[3]);
        assert_eq!(result[4], c[4]);
        assert!(valid(&result, 15));
    }

    #[test]
    fn small_card_can_exchange_with_tall_card_without_becoming_oversized() {
        let c = defaults();
        let result = preview_move(&c, 1, Card { x: 0, ..c[1] }, (3., 2.), 15).unwrap();
        assert_eq!(result[1], card(0, 0, 6, 4));
        assert_eq!(result[0], card(6, 0, 6, 8));
        assert_eq!(result[2], card(0, 4, 6, 4));
        assert!(valid(&result, 15));
    }

    #[test]
    fn moving_into_empty_space_preserves_size() {
        let c = defaults();
        let result = preview_move(&c, 1, Card { y: 15, ..c[1] }, (8., 16.), 24).unwrap();
        assert_eq!(result[1], card(6, 15, 6, 4));
        assert!(valid(&result, 24));
    }

    #[test]
    fn a_manually_narrowed_card_does_not_regrow_on_move() {
        let mut c = defaults();
        c[2].visible = false;
        c[3].w = 5;
        c[3].x = 7;
        let result = preview_move(&c, 3, Card { y: 4, ..c[3] }, (9., 5.), 15).unwrap();
        assert_eq!(result[3], card(7, 4, 5, 4));
        assert!(!result[2].visible);
        assert!(valid(&result, 15));
    }

    #[test]
    fn swapping_onto_a_wide_card_does_not_inflate_the_dragged_card() {
        let c = defaults();
        let proposed = Card { y: 12, ..c[0] };
        let result = preview_move(&c, 0, proposed, (3., 13.), 15).unwrap();
        assert_eq!(result[0].w, c[0].w);
        assert!(valid(&result, 15));
        assert_eq!(c, defaults());
    }

    #[test]
    fn moves_stay_in_bounds_and_ignore_invalid_input() {
        let c = defaults();
        let result = preview_move(&c, 1, card(-100, 100, 6, 4), (-100., 100.), 24).unwrap();
        assert_eq!(result[1], card(0, 20, 6, 4));
        assert!(valid(&result, 24));
        assert!(preview_move(&c, 10, c[0], (0., 0.), 15).is_none());
        assert!(preview_move(&c, 1, c[0], (f32::NAN, 0.), 15).is_none());
        assert!(preview_move(&c, 1, c[0], (0., 0.), 2).is_none());
    }

    #[test]
    fn all_default_swaps_preserve_bounds_and_nonoverlap() {
        let c = defaults();
        for i in 0..5 {
            for j in 0..5 {
                let slot = c[j];
                let proposed = Card {
                    x: slot.x,
                    y: slot.y,
                    ..c[i]
                };
                if let Some(result) = preview_move(
                    &c,
                    i,
                    proposed,
                    (slot.x as f32 + 0.5, slot.y as f32 + 0.5),
                    15,
                ) {
                    assert!(valid(&result, 15), "invalid move {i} -> {j}: {result:?}");
                }
            }
        }
    }
    #[test]
    fn grown_canvas_accepts_moves_beyond_twelve_columns() {
        let c = defaults();
        let moved = preview_move_in(&c, 1, card(13, 0, 6, 4), (16., 2.), 20, 20).unwrap();
        assert_eq!(moved[1], card(13, 0, 6, 4));
        assert!(valid_in(&moved, 20, 20));
    }
    #[test]
    fn resize_stops_at_neighbor_and_keeps_its_origin() {
        let mut c = defaults();
        assert!(resize_in(&mut c, 1, card(6, 0, 12, 12), 12, 15));
        assert_eq!(c[1], defaults()[1]);
        assert!(resize_in(&mut c, 0, card(0, 0, 5, 8), 12, 15));
        assert_eq!(c[0], card(0, 0, 5, 8));
    }
    #[test]
    fn restore_extends_a_full_canvas_and_compaction_preserves_card_sizes() {
        let mut c = defaults();
        c[1].visible = false;
        c[2].y = 0;
        c[2].h = 8;
        let rows = restore(&mut c, 1, 12, 15).unwrap();
        assert_eq!(rows, 19);
        assert!(c[1].visible);
        assert!(valid(&c, rows));
        c[1].visible = false;
        c[4].y = 24;
        let sizes = c.map(|card| (card.w, card.h));
        compact(&mut c);
        assert_eq!(extent(&c).1, 15);
        assert_eq!(sizes, c.map(|card| (card.w, card.h)));
    }
}

/// Stationary hover expresses reorder intent; crossing a card never commits a swap.
#[derive(Clone, Copy, Debug, Default)]
pub struct HoverIntent {
    target: Option<usize>,
    anchor: (f32, f32),
    started: Option<std::time::Instant>,
}
impl HoverIntent {
    pub const DELAY: std::time::Duration = std::time::Duration::from_millis(320);
    pub fn update(
        &mut self,
        target: Option<usize>,
        point: (f32, f32),
        now: std::time::Instant,
    ) -> bool {
        let moved = (point.0 - self.anchor.0).hypot(point.1 - self.anchor.1) > 12.;
        if target != self.target || moved {
            self.target = target;
            self.anchor = point;
            self.started = target.map(|_| now);
        }
        self.started
            .is_some_and(|start| now.saturating_duration_since(start) >= Self::DELAY)
    }
    pub fn remaining(&self, now: std::time::Instant) -> Option<std::time::Duration> {
        self.started
            .map(|t| (t + Self::DELAY).saturating_duration_since(now))
            .filter(|d| !d.is_zero())
    }
}
pub fn hovered_card(cards: &[Card; 6], moving: usize, point: (f32, f32)) -> Option<usize> {
    cards.iter().enumerate().find_map(|(i, c)| {
        (i != moving
            && c.visible
            && point.0 >= c.x as f32
            && point.0 < (c.x + c.w) as f32
            && point.1 >= c.y as f32
            && point.1 < (c.y + c.h) as f32)
            .then_some(i)
    })
}
pub fn preview_free_move_in(
    original: &[Card; 6],
    i: usize,
    mut proposed: Card,
    cols: i32,
    rows: i32,
) -> Option<[Card; 6]> {
    let source = *original.get(i)?;
    proposed.w = source.w;
    proposed.h = source.h;
    let mut result = *original;
    place_in(&mut result, i, proposed, cols, rows).then_some(result)
}
#[cfg(test)]
mod hover_tests {
    use super::*;
    #[test]
    fn pass_over_is_not_swap_but_stationary_hover_is() {
        let mut intent = HoverIntent::default();
        let now = std::time::Instant::now();
        assert!(!intent.update(Some(1), (10., 10.), now));
        assert!(!intent.update(Some(1), (40., 10.), now + HoverIntent::DELAY));
        assert!(!intent.update(Some(2), (40., 10.), now + HoverIntent::DELAY));
        assert!(intent.update(Some(2), (42., 10.), now + HoverIntent::DELAY * 2));
        assert!(!intent.update(None, (42., 10.), now + HoverIntent::DELAY * 2));
    }
    #[test]
    fn status_and_controls_share_minimum_width_and_height() {
        for i in 1..5 {
            assert_eq!(minimum(i), (5, 3));
        }
    }
}
