//! Convert config `LayoutNodeCfg` trees into `juballer_core::layout::Node` trees.

use crate::config::schema::{LayoutChildNode, LayoutNodeCfg, SizingCfg, StackInner};
use crate::{Error, Result};
use juballer_core::layout::{Axis, Node, PaneId, Sizing};
use std::collections::HashMap;

#[derive(Debug)]
pub struct LayoutConverted {
    pub root: Node,
    pub pane_names: Vec<String>,
}

/// Convert a `LayoutNodeCfg` into a core `Node`. PaneId is `&'static str`, so we leak
/// pane name strings to get static lifetimes. Acceptable for config-driven use: we
/// rebuild the layout on hot reload and the previous leaks remain reachable via the
/// pane_names vec (returned for subsequent teardown cycles).
pub fn convert(
    cfg: &LayoutNodeCfg,
    interner: &mut HashMap<String, &'static str>,
) -> Result<LayoutConverted> {
    let mut pane_names = Vec::new();
    let root = walk_node(cfg, interner, &mut pane_names)?;
    Ok(LayoutConverted { root, pane_names })
}

fn walk_node(
    cfg: &LayoutNodeCfg,
    interner: &mut HashMap<String, &'static str>,
    panes: &mut Vec<String>,
) -> Result<Node> {
    match cfg {
        LayoutNodeCfg::Pane { pane } => {
            panes.push(pane.clone());
            Ok(Node::Pane(intern(pane, interner)))
        }
        LayoutNodeCfg::Stack {
            dir, gap, children, ..
        } => {
            let axis = parse_axis(dir)?;
            let mut xs = Vec::with_capacity(children.len());
            for child in children {
                let sz = parse_sizing(&child.size);
                let node = walk_child(&child.node, interner, panes)?;
                xs.push((sz, node));
            }
            Ok(Node::Stack {
                dir: axis,
                gap_px: *gap,
                children: xs,
            })
        }
    }
}

fn walk_child(
    cfg: &LayoutChildNode,
    interner: &mut HashMap<String, &'static str>,
    panes: &mut Vec<String>,
) -> Result<Node> {
    match cfg {
        LayoutChildNode::Pane { pane } => {
            panes.push(pane.clone());
            Ok(Node::Pane(intern(pane, interner)))
        }
        LayoutChildNode::Stack { stack } => walk_stack(stack, interner, panes),
    }
}

fn walk_stack(
    s: &StackInner,
    interner: &mut HashMap<String, &'static str>,
    panes: &mut Vec<String>,
) -> Result<Node> {
    let axis = parse_axis(&s.dir)?;
    let mut xs = Vec::with_capacity(s.children.len());
    for child in &s.children {
        let sz = parse_sizing(&child.size);
        let node = walk_child(&child.node, interner, panes)?;
        xs.push((sz, node));
    }
    Ok(Node::Stack {
        dir: axis,
        gap_px: s.gap,
        children: xs,
    })
}

fn parse_axis(s: &str) -> Result<Axis> {
    match s {
        "horizontal" => Ok(Axis::Horizontal),
        "vertical" => Ok(Axis::Vertical),
        other => Err(Error::Config(format!("unknown axis: {other}"))),
    }
}

fn parse_sizing(s: &SizingCfg) -> Sizing {
    match s {
        SizingCfg::Fixed { fixed } => Sizing::Fixed(*fixed),
        SizingCfg::Ratio { ratio } => Sizing::Ratio(*ratio),
        SizingCfg::Auto { .. } => Sizing::Auto,
    }
}

fn intern(name: &str, interner: &mut HashMap<String, &'static str>) -> PaneId {
    if let Some(&s) = interner.get(name) {
        return s;
    }
    let leaked: &'static str = Box::leak(name.to_string().into_boxed_str());
    interner.insert(name.to_string(), leaked);
    leaked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::*;

    #[test]
    fn convert_simple_vertical() {
        let cfg = LayoutNodeCfg::Stack {
            kind: "stack".into(),
            dir: "vertical".into(),
            gap: 10,
            children: vec![
                LayoutChildCfg {
                    size: SizingCfg::Fixed { fixed: 48 },
                    node: LayoutChildNode::Pane {
                        pane: "header".into(),
                    },
                },
                LayoutChildCfg {
                    size: SizingCfg::Ratio { ratio: 1.0 },
                    node: LayoutChildNode::Pane {
                        pane: "body".into(),
                    },
                },
            ],
        };
        let mut interner = HashMap::new();
        let out = convert(&cfg, &mut interner).unwrap();
        assert_eq!(out.pane_names, vec!["header", "body"]);
        match out.root {
            Node::Stack { children, .. } => {
                assert_eq!(children.len(), 2);
                match &children[0].1 {
                    Node::Pane(p) => assert_eq!(*p, "header"),
                    _ => panic!("expected pane"),
                }
            }
            _ => panic!("expected stack"),
        }
    }

    #[test]
    fn bad_axis_errors() {
        let cfg = LayoutNodeCfg::Stack {
            kind: "stack".into(),
            dir: "diagonal".into(),
            gap: 0,
            children: vec![],
        };
        let mut interner = HashMap::new();
        let err = convert(&cfg, &mut interner).unwrap_err();
        match err {
            Error::Config(_) => {}
            other => panic!("wrong variant: {other:?}"),
        }
    }
}
