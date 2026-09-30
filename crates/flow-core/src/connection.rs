//! Toolkit-independent drag-to-connect state. Coordinates passed to this module
//! are canvas-local screen pixels, so magnet radius is independent of zoom/DPI.
use crate::{Graph, GraphError, Point, PortDirection, PortRef, Viewport};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectionPair {
    pub source: PortRef,
    pub target: PortRef,
}

#[derive(Clone, Debug)]
pub struct ConnectionDrag {
    source: Option<PortRef>,
    target: Option<PortRef>,
    hovered: Option<PortRef>,
    pointer: Point,
    snap_radius: f32,
}
impl Default for ConnectionDrag {
    fn default() -> Self {
        Self {
            source: None,
            target: None,
            hovered: None,
            pointer: Point::ZERO,
            snap_radius: 20.,
        }
    }
}
impl ConnectionDrag {
    pub fn with_snap_radius(mut self, pixels: f32) -> Self {
        if pixels.is_finite() && pixels > 0. {
            self.snap_radius = pixels;
        }
        self
    }
    pub fn snap_radius(&self) -> f32 {
        self.snap_radius
    }
    pub fn active(&self) -> bool {
        self.source.is_some()
    }
    pub fn source(&self) -> Option<&PortRef> {
        self.source.as_ref()
    }
    /// Nearest compatible endpoint within the screen-pixel magnet radius.
    pub fn target(&self) -> Option<&PortRef> {
        self.target.as_ref()
    }
    /// Nearest socket, including incompatible sockets, for rejection feedback.
    pub fn hovered(&self) -> Option<&PortRef> {
        self.hovered.as_ref()
    }
    pub fn can_start(graph: &Graph, port: &PortRef) -> Result<(), GraphError> {
        let socket = graph
            .port(port)
            .ok_or_else(|| GraphError::MissingPort(port.clone()))?;
        if socket
            .max_connections
            .is_some_and(|limit| graph.connection_count(port) >= limit)
        {
            return Err(GraphError::InvalidConnection(
                "port connection capacity reached".into(),
            ));
        }
        Ok(())
    }
    /// Starting again first clears the old gesture, including on invalid input.
    pub fn begin(&mut self, graph: &Graph, port: PortRef, screen: Point) -> Result<(), GraphError> {
        self.cancel();
        if !screen.is_finite() {
            return Err(GraphError::InvalidGeometry("non-finite pointer".into()));
        }
        Self::can_start(graph, &port)?;
        self.source = Some(port);
        self.pointer = screen;
        Ok(())
    }
    pub fn cancel(&mut self) {
        self.source = None;
        self.target = None;
        self.hovered = None;
    }
    pub fn validate_target(
        &self,
        graph: &Graph,
        port: &PortRef,
    ) -> Result<ConnectionPair, GraphError> {
        let first = self
            .source
            .as_ref()
            .ok_or_else(|| GraphError::InvalidConnection("no active connection drag".into()))?;
        let direction = graph
            .port(first)
            .ok_or_else(|| GraphError::MissingPort(first.clone()))?
            .direction;
        let pair = match direction {
            PortDirection::Output => ConnectionPair {
                source: first.clone(),
                target: port.clone(),
            },
            PortDirection::Input => ConnectionPair {
                source: port.clone(),
                target: first.clone(),
            },
        };
        graph.validate_connection(&pair.source, &pair.target)?;
        Ok(pair)
    }
    /// Missing left-button state cancels immediately (e.g. release outside the window).
    pub fn move_to(
        &mut self,
        graph: &Graph,
        viewport: Viewport,
        screen: Point,
        left_pressed: bool,
    ) {
        if !left_pressed
            || !screen.is_finite()
            || !viewport.is_valid()
            || self
                .source
                .as_ref()
                .is_none_or(|source| graph.port(source).is_none())
        {
            self.cancel();
            return;
        }
        self.pointer = screen;
        self.target = None;
        self.hovered = None;
        let mut nearest = self.snap_radius;
        let mut nearest_compatible = self.snap_radius;
        for node in graph.nodes.values() {
            for port in &node.ports {
                let reference = PortRef::new(node.id.clone(), port.id.clone());
                if self.source.as_ref() == Some(&reference) {
                    continue;
                }
                let Some(position) = graph.port_position(&reference) else {
                    continue;
                };
                let distance = viewport.world_to_screen(position).distance(screen);
                if distance <= nearest {
                    nearest = distance;
                    self.hovered = Some(reference.clone());
                }
                if distance <= nearest_compatible && self.validate_target(graph, &reference).is_ok()
                {
                    nearest_compatible = distance;
                    self.target = Some(reference);
                }
            }
        }
    }
    /// Update from an exact host hit without doing any proximity search. A
    /// separate snapping plugin may replace `hovered` before calling this.
    pub fn move_to_exact(
        &mut self,
        graph: &Graph,
        screen: Point,
        hovered: Option<PortRef>,
        left_pressed: bool,
    ) {
        if !left_pressed
            || !screen.is_finite()
            || self
                .source
                .as_ref()
                .is_none_or(|source| graph.port(source).is_none())
        {
            self.cancel();
            return;
        }
        self.pointer = screen;
        self.target = hovered
            .as_ref()
            .filter(|port| self.validate_target(graph, port).is_ok())
            .cloned();
        self.hovered = hovered;
    }
    /// Consume the current exact/snapped hit and always clear pointer state.
    pub fn release_exact(&mut self, graph: &Graph) -> Result<Option<ConnectionPair>, GraphError> {
        let result = if let Some(target) = self.target.as_ref().or(self.hovered.as_ref()) {
            self.validate_target(graph, target).map(Some)
        } else {
            Ok(None)
        };
        self.cancel();
        result
    }
    /// Resolve the final release position, then unconditionally return to idle.
    /// Empty release returns `Ok(None)`; invalid socket release returns its reason.
    pub fn release(
        &mut self,
        graph: &Graph,
        viewport: Viewport,
        screen: Point,
    ) -> Result<Option<ConnectionPair>, GraphError> {
        self.move_to(graph, viewport, screen, true);
        self.release_exact(graph)
    }
    /// Preview endpoints in world coordinates, always ordered output → input.
    pub fn preview(&self, graph: &Graph, viewport: Viewport) -> Option<(Point, Point)> {
        let source = self.source.as_ref()?;
        let anchor = graph.port_position(source)?;
        let endpoint = self
            .target
            .as_ref()
            .and_then(|target| graph.port_position(target))
            .unwrap_or_else(|| viewport.screen_to_world(self.pointer));
        match graph.port(source)?.direction {
            PortDirection::Output => Some((anchor, endpoint)),
            PortDirection::Input => Some((endpoint, anchor)),
        }
    }
}
