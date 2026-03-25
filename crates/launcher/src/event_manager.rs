use renderer::Scene;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ElementId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl UiRect {
    #[inline]
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x <= self.x + self.w && y <= self.y + self.h
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    PointerMove,
    PointerEnter,
    PointerLeave,
    PointerDown,
    PointerUp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    Other(u32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UiEvent {
    // Semantic UI events consumed by elements. These are produced by an input bridge
    // from backend/raw input (e.g., wl_pointer enter/motion/button).
    PointerMove { x: f32, y: f32 },
    PointerEnter { x: f32, y: f32 },
    PointerLeave { x: f32, y: f32 },
    PointerDown { x: f32, y: f32, button: MouseButton },
    PointerUp { x: f32, y: f32, button: MouseButton },
}

impl UiEvent {
    #[inline]
    pub fn kind(&self) -> EventKind {
        match self {
            UiEvent::PointerMove { .. } => EventKind::PointerMove,
            UiEvent::PointerEnter { .. } => EventKind::PointerEnter,
            UiEvent::PointerLeave { .. } => EventKind::PointerLeave,
            UiEvent::PointerDown { .. } => EventKind::PointerDown,
            UiEvent::PointerUp { .. } => EventKind::PointerUp,
        }
    }

    #[inline]
    pub fn position(&self) -> Option<(f32, f32)> {
        match self {
            UiEvent::PointerMove { x, y }
            | UiEvent::PointerEnter { x, y }
            | UiEvent::PointerLeave { x, y }
            | UiEvent::PointerDown { x, y, .. }
            | UiEvent::PointerUp { x, y, .. } => Some((*x, *y)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventMask(u32);

impl EventMask {
    // EventMask is a bitset describing which UiEvent kinds an element accepts.
    // Extend behavior by adding new bit constants (e.g., KEY_DOWN = 1 << 5),
    // mapping them in from_kind(), and returning the new bits from Element::event_mask().
    pub const NONE: Self = Self(0);
    pub const POINTER_MOVE: Self = Self(1 << 0);
    pub const POINTER_ENTER: Self = Self(1 << 1);
    pub const POINTER_LEAVE: Self = Self(1 << 2);
    pub const POINTER_DOWN: Self = Self(1 << 3);
    pub const POINTER_UP: Self = Self(1 << 4);
    pub const ALL_POINTER: Self = Self(
        Self::POINTER_MOVE.0
            | Self::POINTER_ENTER.0
            | Self::POINTER_LEAVE.0
            | Self::POINTER_DOWN.0
            | Self::POINTER_UP.0,
    );

    #[inline]
    pub fn from_kind(kind: EventKind) -> Self {
        match kind {
            EventKind::PointerMove => Self::POINTER_MOVE,
            EventKind::PointerEnter => Self::POINTER_ENTER,
            EventKind::PointerLeave => Self::POINTER_LEAVE,
            EventKind::PointerDown => Self::POINTER_DOWN,
            EventKind::PointerUp => Self::POINTER_UP,
        }
    }

    #[inline]
    pub fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }
}

impl core::ops::BitOr for EventMask {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventResult {
    Ignored,
    Handled,
    StopPropagation,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RedrawRequest {
    Full,
    Rect(UiRect),
}

#[derive(Debug, Default)]
pub struct EventCtx {
    redraws: Vec<RedrawRequest>,
    layout_requested: bool,
}

impl EventCtx {
    pub fn request_repaint(&mut self) {
        self.redraws.push(RedrawRequest::Full);
    }

    pub fn request_repaint_rect(&mut self, rect: UiRect) {
        self.redraws.push(RedrawRequest::Rect(rect));
    }

    pub fn request_layout(&mut self) {
        self.layout_requested = true;
    }

    fn into_frame_state(self) -> FrameState {
        FrameState {
            redraws: self.redraws,
            layout_requested: self.layout_requested,
        }
    }
}

pub struct PaintCtx<'a> {
    // PaintCtx is the rendering bridge from UI tree to renderer: each element writes
    // draw primitives into the shared Scene during UiTree::paint_all traversal.
    pub scene: &'a mut Scene,
}

pub trait Element {
    fn bounds(&self) -> UiRect;

    fn event_mask(&self) -> EventMask {
        EventMask::NONE
    }

    fn on_event(&mut self, _event: &UiEvent, _ctx: &mut EventCtx) -> EventResult {
        EventResult::Ignored
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>) {}
}

#[derive(Debug)]
struct Node<E> {
    id: ElementId,
    parent: Option<usize>,
    children: Vec<usize>,
    z_index: i32,
    element: E,
}

pub struct UiTree<E> {
    nodes: Vec<Node<E>>,
    roots: Vec<usize>,
    next_id: u64,
}

impl<E> Default for UiTree<E> {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            roots: Vec::new(),
            next_id: 1,
        }
    }
}

impl<E: Element> UiTree<E> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_root(&mut self, element: E, z_index: i32) -> ElementId {
        self.add_node(None, element, z_index)
    }

    pub fn add_child(&mut self, parent: ElementId, element: E, z_index: i32) -> Option<ElementId> {
        // Tree extension point: this is where you grow nested UI structure.
        // Add children here, then hit-testing/bubbling automatically include them.
        let parent_idx = self.index_of(parent)?;
        Some(self.add_node(Some(parent_idx), element, z_index))
    }

    pub fn element_mut(&mut self, id: ElementId) -> Option<&mut E> {
        let idx = self.index_of(id)?;
        Some(&mut self.nodes[idx].element)
    }

    pub fn element(&self, id: ElementId) -> Option<&E> {
        let idx = self.index_of(id)?;
        Some(&self.nodes[idx].element)
    }

    pub fn parent_of(&self, id: ElementId) -> Option<ElementId> {
        let idx = self.index_of(id)?;
        let parent_idx = self.nodes[idx].parent?;
        Some(self.nodes[parent_idx].id)
    }

    pub fn hit_test(&self, x: f32, y: f32) -> Option<ElementId> {
        // Hit-test entry point: traverses roots from highest z to lowest z,
        // so the top-most visible element wins.
        self.hit_test_in(&self.sorted_roots_desc(), x, y)
    }

    pub fn paint_all(&self, ctx: &mut PaintCtx<'_>) {
        // Paint pass traversal: roots and children are painted in ascending z order,
        // so later (higher z) elements can visually appear on top.
        for root in self.sorted_roots_asc() {
            self.paint_node(root, ctx);
        }
    }

    fn add_node(&mut self, parent: Option<usize>, element: E, z_index: i32) -> ElementId {
        let id = ElementId(self.next_id);
        self.next_id += 1;

        let idx = self.nodes.len();
        self.nodes.push(Node {
            id,
            parent,
            children: Vec::new(),
            z_index,
            element,
        });

        match parent {
            Some(parent_idx) => self.nodes[parent_idx].children.push(idx),
            None => self.roots.push(idx),
        }

        id
    }

    fn index_of(&self, id: ElementId) -> Option<usize> {
        self.nodes.iter().position(|n| n.id == id)
    }

    fn sorted_roots_desc(&self) -> Vec<usize> {
        let mut out = self.roots.clone();
        out.sort_by_key(|idx| self.nodes[*idx].z_index);
        out.reverse();
        out
    }

    fn sorted_roots_asc(&self) -> Vec<usize> {
        let mut out = self.roots.clone();
        out.sort_by_key(|idx| self.nodes[*idx].z_index);
        out
    }

    fn sorted_children_desc(&self, idx: usize) -> Vec<usize> {
        let mut out = self.nodes[idx].children.clone();
        out.sort_by_key(|child| self.nodes[*child].z_index);
        out.reverse();
        out
    }

    fn sorted_children_asc(&self, idx: usize) -> Vec<usize> {
        let mut out = self.nodes[idx].children.clone();
        out.sort_by_key(|child| self.nodes[*child].z_index);
        out
    }

    fn hit_test_in(&self, nodes: &[usize], x: f32, y: f32) -> Option<ElementId> {
        // Walk candidate nodes and return immediately on first hit.
        for idx in nodes {
            if let Some(hit) = self.hit_test_node(*idx, x, y) {
                return Some(hit);
            }
        }
        None
    }

    fn hit_test_node(&self, idx: usize, x: f32, y: f32) -> Option<ElementId> {
        // Depth-first in descending child z-order:
        // children are tested before parent so deeply nested top-most children win.
        for child in self.sorted_children_desc(idx) {
            if let Some(hit) = self.hit_test_node(child, x, y) {
                return Some(hit);
            }
        }

        let node = &self.nodes[idx];
        if node.element.bounds().contains(x, y) {
            Some(node.id)
        } else {
            None
        }
    }

    fn paint_node(&self, idx: usize, ctx: &mut PaintCtx<'_>) {
        let node = &self.nodes[idx];
        node.element.paint(ctx);
        for child in self.sorted_children_asc(idx) {
            self.paint_node(child, ctx);
        }
    }
}

#[derive(Debug, Default)]
pub struct FrameState {
    // Collected side effects of one or more dispatched events.
    // Launcher/runtime reads this and decides whether to trigger a redraw/layout pass.
    pub redraws: Vec<RedrawRequest>,
    pub layout_requested: bool,
}

#[derive(Debug, Default)]
pub struct EventManager {
    hovered: Option<ElementId>,
    pressed: Option<ElementId>,
    frame_state: FrameState,
}

impl EventManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn hovered(&self) -> Option<ElementId> {
        self.hovered
    }

    pub fn pressed(&self) -> Option<ElementId> {
        self.pressed
    }

    pub fn process_event<E: Element>(&mut self, tree: &mut UiTree<E>, event: UiEvent) {
        match event {
            UiEvent::PointerMove { x, y } | UiEvent::PointerEnter { x, y } => {
                self.update_hover(tree, x, y);
                self.dispatch_at(tree, &event, x, y, false);
            }
            UiEvent::PointerLeave { .. } => {
                self.clear_hover(tree, &event);
            }
            UiEvent::PointerDown { x, y, .. } => {
                let target = tree.hit_test(x, y);
                self.pressed = target;
                self.dispatch_target(tree, &event, target);
            }
            UiEvent::PointerUp { x, y, .. } => {
                let target = self.pressed.or_else(|| tree.hit_test(x, y));
                self.dispatch_target(tree, &event, target);
                self.pressed = None;
            }
        }
    }

    pub fn take_frame_state(&mut self) -> FrameState {
        core::mem::take(&mut self.frame_state)
    }

    fn update_hover<E: Element>(&mut self, tree: &mut UiTree<E>, x: f32, y: f32) {
        let next_hover = tree.hit_test(x, y);
        if self.hovered == next_hover {
            return;
        }

        if let Some(prev) = self.hovered {
            let leave_event = UiEvent::PointerLeave { x, y };
            self.dispatch_target(tree, &leave_event, Some(prev));
        }

        if let Some(next) = next_hover {
            let enter_event = UiEvent::PointerEnter { x, y };
            self.dispatch_target(tree, &enter_event, Some(next));
        }

        self.hovered = next_hover;
    }

    fn clear_hover<E: Element>(&mut self, tree: &mut UiTree<E>, leave_event: &UiEvent) {
        if let Some(prev) = self.hovered {
            self.dispatch_target(tree, leave_event, Some(prev));
        }
        self.hovered = None;
    }

    fn dispatch_at<E: Element>(
        &mut self,
        tree: &mut UiTree<E>,
        event: &UiEvent,
        x: f32,
        y: f32,
        fallback_to_hover: bool,
    ) {
        let target = tree
            .hit_test(x, y)
            .or_else(|| fallback_to_hover.then_some(self.hovered).flatten());
        self.dispatch_target(tree, event, target);
    }

    // Dispatch core:
    // 1) Starts from hit target.
    // 2) Traverses up parent chain (bubbling).
    // 3) Checks element.event_mask() against required EventKind mask.
    // 4) Calls on_event only for supporting elements.
    // 5) Aggregates redraw/layout requests into frame_state.
    fn dispatch_target<E: Element>(
        &mut self,
        tree: &mut UiTree<E>,
        event: &UiEvent,
        mut target: Option<ElementId>,
    ) {
        let required = EventMask::from_kind(event.kind());
        let mut ctx = EventCtx::default();

        while let Some(id) = target {
            let parent = tree.parent_of(id);

            let Some(element) = tree.element_mut(id) else {
                break;
            };

            // Exact support check: element receives this event only when its EventMask
            // includes the required bit for the current UiEvent kind.
            if element.event_mask().contains(required) {
                match element.on_event(event, &mut ctx) {
                    EventResult::Ignored => {}
                    EventResult::Handled => break,
                    EventResult::StopPropagation => break,
                }
            }

            target = parent;
        }

        let frame_updates = ctx.into_frame_state();
        self.frame_state.redraws.extend(frame_updates.redraws);
        self.frame_state.layout_requested |= frame_updates.layout_requested;
    }
}
