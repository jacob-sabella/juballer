use super::{Axis, Node, PaneId, Sizing};
use crate::Rect;
use indexmap::IndexMap;

/// Solve the layout tree against an outer rect. Returns one Rect per leaf Pane.
///
/// Sizing semantics within a Stack:
/// * `Fixed(px)` consumes exactly `px` pixels along the stack axis (capped to remaining).
/// * `Ratio(r)` shares the leftover space (after Fixed/Auto are subtracted) proportionally.
/// * `Auto`     is treated as `Ratio(1.0)` for v0.1 (no shrink-to-content yet — reserved).
///
/// Cross-axis size for every child is the full available cross dimension of the stack.
pub fn solve(root: &Node, outer: Rect) -> IndexMap<PaneId, Rect> {
    let mut out = IndexMap::new();
    place(root, outer, &mut out);
    out
}

fn place(node: &Node, rect: Rect, out: &mut IndexMap<PaneId, Rect>) {
    match node {
        Node::Pane(id) => {
            out.insert(*id, rect);
        }
        Node::Stack {
            dir,
            gap_px,
            children,
        } => {
            let child_rects = compute_stack(*dir, *gap_px, children, rect);
            for ((_, child), child_rect) in children.iter().zip(child_rects) {
                place(child, child_rect, out);
            }
        }
    }
}

fn compute_stack(dir: Axis, gap_px: u16, children: &[(Sizing, Node)], rect: Rect) -> Vec<Rect> {
    if children.is_empty() {
        return Vec::new();
    }
    let n = children.len() as u32;
    let total_gap = gap_px as u32 * n.saturating_sub(1);
    let main_total = match dir {
        Axis::Horizontal => rect.w,
        Axis::Vertical => rect.h,
    };
    let main_avail = main_total.saturating_sub(total_gap);

    // First pass: sum fixed pixels.
    let mut fixed_sum: u32 = 0;
    let mut ratio_sum: f32 = 0.0;
    for (sz, _) in children {
        match sz {
            Sizing::Fixed(px) => fixed_sum = fixed_sum.saturating_add(*px as u32),
            Sizing::Ratio(r) => ratio_sum += r.max(0.0),
            Sizing::Auto => ratio_sum += 1.0,
        }
    }
    let leftover = main_avail.saturating_sub(fixed_sum);

    // Second pass: assign sizes.
    let mut sizes: Vec<u32> = Vec::with_capacity(children.len());
    let mut accum = 0u32;
    for (i, (sz, _)) in children.iter().enumerate() {
        let s = match sz {
            Sizing::Fixed(px) => (*px as u32).min(main_avail.saturating_sub(accum)),
            Sizing::Ratio(r) => {
                if ratio_sum <= 0.0 {
                    0
                } else {
                    let f = (*r).max(0.0) / ratio_sum;
                    if i + 1 == children.len() {
                        // Last ratio child gets all remaining ratio space (no rounding loss).
                        leftover.saturating_sub(
                            sizes
                                .iter()
                                .zip(children.iter())
                                .filter(|(_, (sz, _))| {
                                    matches!(sz, Sizing::Ratio(_) | Sizing::Auto)
                                })
                                .map(|(s, _)| *s)
                                .sum::<u32>(),
                        )
                    } else {
                        ((leftover as f32 * f).round() as u32).min(leftover)
                    }
                }
            }
            Sizing::Auto => {
                if ratio_sum <= 0.0 {
                    0
                } else {
                    let f = 1.0 / ratio_sum;
                    ((leftover as f32 * f).round() as u32).min(leftover)
                }
            }
        };
        sizes.push(s);
        accum += s;
    }

    // Lay out as rects.
    let mut rects = Vec::with_capacity(children.len());
    let mut cursor = 0i32;
    for (i, &s) in sizes.iter().enumerate() {
        let r = match dir {
            Axis::Horizontal => Rect::new(rect.x + cursor, rect.y, s, rect.h),
            Axis::Vertical => Rect::new(rect.x, rect.y + cursor, rect.w, s),
        };
        rects.push(r);
        cursor += s as i32;
        if i + 1 < children.len() {
            cursor += gap_px as i32;
        }
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outer() -> Rect {
        Rect::new(0, 0, 1000, 400)
    }

    #[test]
    fn single_pane_fills_outer() {
        let t = Node::Pane("only");
        let m = solve(&t, outer());
        assert_eq!(m["only"], outer());
    }

    #[test]
    fn horizontal_two_equal_ratios() {
        let t = Node::Stack {
            dir: Axis::Horizontal,
            gap_px: 0,
            children: vec![
                (Sizing::Ratio(1.0), Node::Pane("a")),
                (Sizing::Ratio(1.0), Node::Pane("b")),
            ],
        };
        let m = solve(&t, outer());
        assert_eq!(m["a"], Rect::new(0, 0, 500, 400));
        assert_eq!(m["b"], Rect::new(500, 0, 500, 400));
    }

    #[test]
    fn vertical_fixed_then_ratio() {
        let t = Node::Stack {
            dir: Axis::Vertical,
            gap_px: 0,
            children: vec![
                (Sizing::Fixed(48), Node::Pane("hdr")),
                (Sizing::Ratio(1.0), Node::Pane("body")),
            ],
        };
        let m = solve(&t, outer());
        assert_eq!(m["hdr"], Rect::new(0, 0, 1000, 48));
        assert_eq!(m["body"], Rect::new(0, 48, 1000, 352));
    }

    #[test]
    fn gap_consumes_main_axis_pixels() {
        let t = Node::Stack {
            dir: Axis::Horizontal,
            gap_px: 10,
            children: vec![
                (Sizing::Ratio(1.0), Node::Pane("a")),
                (Sizing::Ratio(1.0), Node::Pane("b")),
                (Sizing::Ratio(1.0), Node::Pane("c")),
            ],
        };
        // 1000 - 2*10 = 980 split 3 ways; last gets remainder.
        let m = solve(&t, outer());
        assert_eq!(m["a"].w, 327);
        assert_eq!(m["b"].w, 327);
        assert_eq!(m["c"].w, 326);
        assert_eq!(m["a"].x, 0);
        assert_eq!(m["b"].x, 327 + 10);
        assert_eq!(m["c"].x, 327 + 10 + 327 + 10);
    }

    #[test]
    fn nested_tree_matches_mockup_shape() {
        let t = Node::Stack {
            dir: Axis::Vertical,
            gap_px: 10,
            children: vec![
                (Sizing::Fixed(48), Node::Pane("header")),
                (
                    Sizing::Ratio(1.0),
                    Node::Stack {
                        dir: Axis::Horizontal,
                        gap_px: 10,
                        children: vec![
                            (Sizing::Ratio(1.2), Node::Pane("focus")),
                            (Sizing::Ratio(1.0), Node::Pane("events")),
                            (Sizing::Ratio(0.7), Node::Pane("pages")),
                        ],
                    },
                ),
            ],
        };
        let outer = Rect::new(0, 0, 1000, 400);
        let m = solve(&t, outer);
        assert_eq!(m["header"], Rect::new(0, 0, 1000, 48));
        assert_eq!(m["focus"].y, 58);
        assert_eq!(m["focus"].h, 342);
        // Three children sum to 1000 - 2*10 = 980 (allowing 1px rounding).
        let total = m["focus"].w + m["events"].w + m["pages"].w;
        assert!(total == 980, "got {}", total);
    }

    #[test]
    fn fixed_oversized_clamps_to_available() {
        let t = Node::Stack {
            dir: Axis::Horizontal,
            gap_px: 0,
            children: vec![
                (Sizing::Fixed(2000), Node::Pane("a")),
                (Sizing::Fixed(500), Node::Pane("b")),
            ],
        };
        let m = solve(&t, outer());
        assert_eq!(m["a"].w, 1000);
        assert_eq!(m["b"].w, 0);
    }

    #[test]
    fn empty_stack_yields_no_panes() {
        let t = Node::Stack {
            dir: Axis::Horizontal,
            gap_px: 0,
            children: vec![],
        };
        let m = solve(&t, outer());
        assert!(m.is_empty());
    }

    #[test]
    fn zero_outer_yields_zero_children() {
        let t = Node::Stack {
            dir: Axis::Horizontal,
            gap_px: 5,
            children: vec![
                (Sizing::Ratio(1.0), Node::Pane("a")),
                (Sizing::Ratio(1.0), Node::Pane("b")),
            ],
        };
        let m = solve(&t, Rect::ZERO);
        assert_eq!(m["a"].w, 0);
        assert_eq!(m["b"].w, 0);
    }
}
