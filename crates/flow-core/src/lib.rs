//! UI-independent building blocks for node-based editors.
//!
//! [`Graph`] is a serializable graph with typed ports and connection validation.
//! [`Editor`] adds selection, clipboard operations, and atomic undoable changes.
//! [`Viewport`] uses canvas-local screen coordinates, so host toolkits only need
//! to subtract the canvas origin before passing pointer coordinates here.
//!
//! All document mutations performed through `Editor` are validated. Public model
//! fields intentionally permit custom renderers and importers; call
//! [`Graph::validate`] after constructing or modifying a graph directly.
//!
//! ```
//! use gpui_flow_core::{DataType, Editor, Graph, Node, Point, Port};
//!
//! let mut graph = Graph::new();
//! graph.add_node(Node::new("source", "Source", Point::ZERO)
//!     .with_port(Port::output("value", "Value", DataType::Number)))?;
//! let mut editor = Editor::new(graph)?;
//! editor.selection.select_node("source", false);
//! editor.begin_transaction("Drag source")?;
//! editor.move_selection(Point::new(10.0, 20.0))?;
//! editor.move_selection(Point::new(5.0, 0.0))?;
//! editor.commit_transaction()?;
//! assert!(editor.undo());
//! assert_eq!(editor.graph().nodes[&"source".into()].position, Point::ZERO);
//! # Ok::<(), gpui_flow_core::GraphError>(())
//! ```

mod editor;
mod geometry;
mod model;

pub use editor::*;
pub use geometry::*;
pub use model::*;

mod persistence;
pub use persistence::*;

mod connection;
pub use connection::*;

mod presentation;
pub use presentation::*;
