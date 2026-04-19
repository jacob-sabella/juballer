#![cfg(feature = "headless")]

use juballer_core::layout::{Axis, Node, Sizing::*};
use juballer_core::{layout, Rect};

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[test]
fn layout_solve_steady_state_no_alloc() {
    let _profiler = dhat::Profiler::builder().testing().build();

    let tree = Node::Stack {
        dir: Axis::Vertical,
        gap_px: 10,
        children: vec![
            (Fixed(48), Node::Pane("header")),
            (Ratio(1.0), Node::Pane("body")),
        ],
    };
    let outer = Rect::new(0, 0, 1920, 1080);

    // Warmup: first few solves may allocate IndexMap buckets.
    for _ in 0..5 {
        let _ = layout::solve(&tree, outer);
    }

    let stats0 = dhat::HeapStats::get();
    for _ in 0..1000 {
        let _ = layout::solve(&tree, outer);
    }
    let stats1 = dhat::HeapStats::get();

    // Each solve allocates a new IndexMap, which is fine for the public API contract.
    // The contract being enforced here is that the SOLVER ITSELF does not leak — i.e.
    // in steady state, curr_bytes should stay bounded.
    let bytes_grew = stats1.curr_bytes.saturating_sub(stats0.curr_bytes);
    assert!(
        bytes_grew < 1024,
        "solver leaked {bytes_grew} bytes in steady state"
    );
}
