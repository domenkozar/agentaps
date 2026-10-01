//! Serializable desktop pane layout, independent of GPUI and agent processes.
use serde::{Deserialize, Serialize};

pub(crate) const MIN_WIDTH: f32 = 320.;
pub(crate) const MIN_HEIGHT: f32 = 280.;
pub(crate) const DIVIDER: f32 = 6.;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Direction {
    Right,
    Down,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Node {
    Pane {
        id: u64,
        session: Option<u64>,
    },
    Split {
        id: u64,
        direction: Direction,
        ratio: f32,
        first: Box<Node>,
        second: Box<Node>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Layout {
    pub(crate) root: Node,
    pub(crate) focused: u64,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            root: Node::Pane {
                id: 1,
                session: None,
            },
            focused: 1,
        }
    }
}

impl Node {
    pub(crate) fn panes(&self) -> Vec<(u64, Option<u64>)> {
        match self {
            Self::Pane { id, session } => vec![(*id, *session)],
            Self::Split { first, second, .. } => {
                first.panes().into_iter().chain(second.panes()).collect()
            }
        }
    }

    pub(crate) fn find_mut(&mut self, target: u64) -> Option<&mut Self> {
        let id = match self {
            Self::Pane { id, .. } | Self::Split { id, .. } => *id,
        };
        if id == target {
            return Some(self);
        }
        match self {
            Self::Split { first, second, .. } => {
                first.find_mut(target).or_else(|| second.find_mut(target))
            }
            _ => None,
        }
    }

    pub(crate) fn minimum_size(&self) -> (f32, f32) {
        match self {
            Self::Pane { .. } => (MIN_WIDTH, MIN_HEIGHT),
            Self::Split {
                direction,
                first,
                second,
                ..
            } => {
                let (aw, ah) = first.minimum_size();
                let (bw, bh) = second.minimum_size();
                match direction {
                    Direction::Right => (aw + bw + DIVIDER, ah.max(bh)),
                    Direction::Down => (aw.max(bw), ah + bh + DIVIDER),
                }
            }
        }
    }

    pub(crate) fn sizes(&self, width: f32, height: f32) -> Option<(f32, f32)> {
        let Self::Split {
            direction,
            ratio,
            first,
            second,
            ..
        } = self
        else {
            return None;
        };
        let a = first.minimum_size();
        let b = second.minimum_size();
        let (total, min_a, min_b) = match direction {
            Direction::Right => (width - DIVIDER, a.0, b.0),
            Direction::Down => (height - DIVIDER, a.1, b.1),
        };
        let total = total.max(min_a + min_b);
        let first = (total * ratio).clamp(min_a, total - min_b);
        Some((first, total - first))
    }

    fn remove(&mut self, target: u64) -> Option<u64> {
        let Self::Split { first, second, .. } = self else {
            return None;
        };
        if matches!(first.as_ref(), Self::Pane { id, .. } if *id == target) {
            let sibling = second.as_ref().clone();
            let focus = sibling.panes()[0].0;
            *self = sibling;
            return Some(focus);
        }
        if matches!(second.as_ref(), Self::Pane { id, .. } if *id == target) {
            let sibling = first.as_ref().clone();
            let focus = sibling.panes().last().unwrap().0;
            *self = sibling;
            return Some(focus);
        }
        first.remove(target).or_else(|| second.remove(target))
    }
}

impl Layout {
    pub(crate) fn split(
        &mut self,
        target: u64,
        direction: Direction,
        split_id: u64,
        pane_id: u64,
    ) -> bool {
        let Some(node @ Node::Pane { .. }) = self.root.find_mut(target) else {
            return false;
        };
        *node = Node::Split {
            id: split_id,
            direction,
            ratio: 0.5,
            first: Box::new(node.clone()),
            second: Box::new(Node::Pane {
                id: pane_id,
                session: None,
            }),
        };
        self.focused = pane_id;
        true
    }

    pub(crate) fn close(&mut self, target: u64) -> bool {
        let Some(neighbor) = self.root.remove(target) else {
            return false;
        };
        if self.focused == target {
            self.focused = neighbor;
        }
        true
    }

    /// Reassign node IDs and discard stale or duplicate sessions from saved layouts.
    pub(crate) fn normalize(&mut self, valid_sessions: &[u64]) -> u64 {
        fn visit(
            node: &mut Node,
            next: &mut u64,
            valid: &[u64],
            seen: &mut std::collections::HashSet<u64>,
            focused: u64,
            restored_focus: &mut Option<u64>,
        ) {
            let id = *next;
            *next += 1;
            match node {
                Node::Pane { id: old, session } => {
                    if *old == focused && restored_focus.is_none() {
                        *restored_focus = Some(id);
                    }
                    *old = id;
                    if session
                        .is_some_and(|session| !valid.contains(&session) || !seen.insert(session))
                    {
                        *session = None;
                    }
                }
                Node::Split {
                    id: old,
                    ratio,
                    first,
                    second,
                    ..
                } => {
                    *old = id;
                    *ratio = if ratio.is_finite() {
                        ratio.clamp(0.05, 0.95)
                    } else {
                        0.5
                    };
                    visit(first, next, valid, seen, focused, restored_focus);
                    visit(second, next, valid, seen, focused, restored_focus);
                }
            }
        }
        let mut next = 1;
        let mut focused = None;
        visit(
            &mut self.root,
            &mut next,
            valid_sessions,
            &mut Default::default(),
            self.focused,
            &mut focused,
        );
        self.focused = focused.unwrap_or_else(|| self.root.panes()[0].0);
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_splits_close_without_losing_sessions() {
        let mut layout = Layout::default();
        if let Node::Pane { session, .. } = &mut layout.root {
            *session = Some(100);
        }
        assert!(layout.split(1, Direction::Right, 2, 3));
        assert!(layout.split(3, Direction::Down, 4, 5));
        assert_eq!(layout.root.minimum_size(), (646., 566.));
        assert_eq!(layout.focused, 5);
        assert!(layout.close(5));
        assert_eq!(layout.focused, 3);
        assert!(layout.close(3));
        assert_eq!(
            layout.root,
            Node::Pane {
                id: 1,
                session: Some(100)
            }
        );
        assert_eq!(layout.focused, 1);
        assert!(!layout.close(1));
    }

    #[test]
    fn restoration_normalizes_missing_sessions_duplicate_ids_and_focus() {
        let mut layout = Layout {
            focused: 99,
            root: Node::Split {
                id: 1,
                direction: Direction::Right,
                ratio: f32::NAN,
                first: Box::new(Node::Pane {
                    id: 1,
                    session: Some(7),
                }),
                second: Box::new(Node::Split {
                    id: 1,
                    direction: Direction::Down,
                    ratio: 2.,
                    first: Box::new(Node::Pane {
                        id: 1,
                        session: Some(7),
                    }),
                    second: Box::new(Node::Pane {
                        id: 1,
                        session: Some(8),
                    }),
                }),
            },
        };
        assert_eq!(layout.normalize(&[7]), 6);
        assert_eq!(layout.focused, 2);
        assert_eq!(layout.root.panes(), [(2, Some(7)), (4, None), (5, None)]);
        let Node::Split { ratio, second, .. } = &layout.root else {
            panic!()
        };
        assert_eq!(*ratio, 0.5);
        let Node::Split { ratio, .. } = second.as_ref() else {
            panic!()
        };
        assert_eq!(*ratio, 0.95);
        let restored: Layout =
            serde_json::from_slice(&serde_json::to_vec(&layout).unwrap()).unwrap();
        assert_eq!(restored, layout);
    }

    #[test]
    fn resizing_preserves_subtree_minimums_and_uses_scrollable_dimensions() {
        let mut layout = Layout::default();
        layout.split(1, Direction::Right, 2, 3);
        layout.split(3, Direction::Right, 4, 5);
        let Node::Split { ratio, .. } = &mut layout.root else {
            panic!()
        };
        *ratio = 0.99;
        assert_eq!(layout.root.minimum_size(), (972., 280.));
        assert_eq!(layout.root.sizes(1200., 800.), Some((548., 646.)));
        assert_eq!(layout.root.sizes(400., 200.), Some((320., 646.)));
    }
}
