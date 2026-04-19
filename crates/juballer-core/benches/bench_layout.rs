use criterion::{criterion_group, criterion_main, Criterion};
use juballer_core::layout::{Axis, Node, Sizing::*};
use juballer_core::{layout, Rect};

fn bench_solve(c: &mut Criterion) {
    let tree = Node::Stack {
        dir: Axis::Vertical,
        gap_px: 10,
        children: vec![
            (Fixed(48), Node::Pane("header")),
            (
                Ratio(1.0),
                Node::Stack {
                    dir: Axis::Horizontal,
                    gap_px: 10,
                    children: vec![
                        (Ratio(1.2), Node::Pane("focus")),
                        (Ratio(1.0), Node::Pane("events")),
                        (Ratio(0.7), Node::Pane("pages")),
                    ],
                },
            ),
        ],
    };
    let outer = Rect::new(0, 0, 2560, 547);
    c.bench_function("solve mockup tree", |b| {
        b.iter(|| layout::solve(&tree, outer))
    });
}

criterion_group!(benches, bench_solve);
criterion_main!(benches);
