//! Layout primitive for the top region: tiny tree of Stack/Pane nodes.

pub type PaneId = &'static str;

#[derive(Debug, Clone)]
pub enum Node {
    Stack {
        dir: Axis,
        gap_px: u16,
        children: Vec<(Sizing, Node)>,
    },
    Pane(PaneId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Sizing {
    Fixed(u16),
    Ratio(f32),
    Auto,
}

mod solve;
pub use solve::solve;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_tree_compiles() {
        let _t = Node::Stack {
            dir: Axis::Vertical,
            gap_px: 10,
            children: vec![
                (Sizing::Fixed(48), Node::Pane("header")),
                (Sizing::Ratio(1.0), Node::Pane("body")),
            ],
        };
    }
}
