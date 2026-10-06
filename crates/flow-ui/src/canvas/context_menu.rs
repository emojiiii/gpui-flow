use super::*;
use gpui_flow_interaction::{CanvasCommand, Capability, HitTarget, InteractionError};

/// An exact visible hit and its canvas-relative screen position, before pan/zoom inversion.
#[derive(Clone, Debug, PartialEq)]
pub struct CanvasContextMenuRequest {
    pub hit: HitTarget,
    pub position: model::Point,
}

/// Runs after the pointer event releases the canvas borrow, so hosts may update it safely.
pub type CanvasContextMenuHandler = Arc<dyn Fn(CanvasContextMenuRequest, &mut Window, &mut App)>;

impl FlowCanvas {
    /// Opt into right-click menus in the selection tool. `None` restores right-button pan.
    /// Space and the pan tool always retain their existing pan behavior.
    pub fn set_context_menu_handler(&mut self, handler: Option<CanvasContextMenuHandler>) {
        self.context_menu_handler = handler;
    }

    /// Whether this canvas has a nonempty copied fragment.
    /// Edit policy, destination and host validation are checked when pasting.
    pub fn can_paste_selection(&self) -> bool {
        self.clipboard
            .as_ref()
            .is_some_and(|clipboard| !clipboard.is_empty())
    }

    /// Paste this canvas's private Ctrl+C fragment with its node/group union at `point`.
    /// `point` is in world coordinates. Missing/empty fragments and non-finite points
    /// return an error without changing the editor or copied fragment.
    pub fn paste_copied_selection_at(
        &mut self,
        point: model::Point,
        cx: &mut Context<Self>,
    ) -> Result<(), InteractionError> {
        let command = if self.is_editable() {
            copied_selection_paste_command(
                &self.editor,
                self.clipboard.as_ref(),
                point,
                self.document_validator.as_ref(),
            )
        } else {
            Err(InteractionError::CapabilityDenied(Capability::Edit))
        };
        match command {
            Ok(command) => self.execute_command(command, cx),
            Err(error) => {
                self.message = error.to_string();
                cx.notify();
                Err(error)
            }
        }
    }
}

fn copied_selection_paste_command(
    editor: &Editor,
    clipboard: Option<&Clipboard>,
    point: model::Point,
    validator: Option<&FlowDocumentValidator>,
) -> Result<CanvasCommand, InteractionError> {
    if !point.is_finite() {
        return Err(InteractionError::InvalidInput(
            "Paste destination must be a finite world point".into(),
        ));
    }
    let clipboard = clipboard
        .filter(|clipboard| !clipboard.is_empty())
        .ok_or_else(|| {
            InteractionError::InvalidInput(
                "Copy a node or group in this canvas before pasting".into(),
            )
        })?;
    clipboard.validate()?;
    let bounds = clipboard
        .nodes
        .iter()
        .map(Node::bounds)
        .chain(clipboard.groups.iter().map(|group| group.bounds))
        .reduce(model::Rect::union)
        .expect("nonempty clipboard has a node or group");
    let offset = point - bounds.origin;
    if !offset.is_finite() {
        return Err(InteractionError::InvalidInput(
            "Paste destination is too far from the copied fragment".into(),
        ));
    }
    if validator.is_some() {
        let mut preview = Editor::from_document(editor.document())?;
        preview.paste(clipboard, offset)?;
        persistence::validate_host_document(&preview.document(), validator)
            .map_err(InteractionError::InvalidInput)?;
    }
    Ok(CanvasCommand::Paste {
        clipboard: clipboard.clone(),
        offset,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;

    fn fragment() -> Clipboard {
        Clipboard {
            nodes: vec![
                Node::new("a", "A", model::Point::new(20., 10.))
                    .with_size(model::Size::new(30., 20.)),
                Node::new("b", "B", model::Point::new(-40., 50.))
                    .with_size(model::Size::new(10., 10.)),
            ],
            groups: vec![
                model::Group::new(
                    "frame",
                    "Frame",
                    model::Rect::new(model::Point::new(-10., -30.), model::Size::new(90., 100.)),
                )
                .with_nodes([NodeId::from("a")]),
            ],
            ..Clipboard::default()
        }
    }

    #[test]
    fn paste_at_positions_node_group_union_and_preserves_copy_and_history() {
        let clipboard = fragment();
        let original = clipboard.clone();
        let mut editor = Editor::default();
        let command = copied_selection_paste_command(
            &editor,
            Some(&clipboard),
            model::Point::new(100., 200.),
            None,
        )
        .unwrap();
        assert_eq!(editor.document(), model::FlowDocument::default());
        let mut host = gpui_flow_interaction::PluginHost::with_defaults();
        host.execute(&mut editor, command).unwrap();
        let graph = editor.graph();
        let a = graph.nodes.values().find(|node| node.title == "A").unwrap();
        let b = graph.nodes.values().find(|node| node.title == "B").unwrap();
        assert_eq!(a.position, model::Point::new(160., 240.));
        assert_eq!(b.position, model::Point::new(100., 280.));
        assert_eq!(
            graph.groups.values().next().unwrap().bounds,
            model::Rect::new(model::Point::new(130., 200.), model::Size::new(90., 100.))
        );
        assert_eq!(
            graph.bounds(),
            Some(model::Rect::new(
                model::Point::new(100., 200.),
                model::Size::new(120., 100.)
            ))
        );
        assert_eq!(clipboard, original);
        let pasted = editor.document();
        host.execute(&mut editor, CanvasCommand::Undo).unwrap();
        assert_eq!(editor.document(), model::FlowDocument::default());
        host.execute(&mut editor, CanvasCommand::Redo).unwrap();
        assert_eq!(editor.document(), pasted);
    }

    #[test]
    fn empty_frame_can_be_pasted_without_nodes() {
        let clipboard = Clipboard {
            groups: vec![model::Group::new(
                "frame",
                "Frame",
                model::Rect::new(model::Point::new(80., 60.), model::Size::new(20., 40.)),
            )],
            ..Clipboard::default()
        };
        let command = copied_selection_paste_command(
            &Editor::default(),
            Some(&clipboard),
            model::Point::new(-10., -20.),
            None,
        )
        .unwrap();
        assert!(matches!(
            command,
            CanvasCommand::Paste { offset, .. } if offset == model::Point::new(-90., -80.)
        ));
    }

    #[test]
    fn invalid_destination_or_fragment_is_rejected_without_mutating_input() {
        let editor = Editor::default();
        let clipboard = fragment();
        let original = clipboard.clone();
        for point in [
            model::Point::new(f32::NAN, 0.),
            model::Point::new(0., f32::INFINITY),
            model::Point::new(f32::NEG_INFINITY, 0.),
        ] {
            assert!(
                copied_selection_paste_command(&editor, Some(&clipboard), point, None).is_err()
            );
        }
        for clipboard in [None, Some(&Clipboard::default())] {
            let error =
                copied_selection_paste_command(&editor, clipboard, model::Point::ZERO, None)
                    .unwrap_err();
            assert!(error.to_string().contains("Copy a node or group"));
        }
        let mut invalid = clipboard.clone();
        invalid.groups[0].node_ids.insert(NodeId::from("missing"));
        assert!(
            copied_selection_paste_command(&editor, Some(&invalid), model::Point::ZERO, None)
                .is_err()
        );
        let distant = Clipboard {
            nodes: vec![Node::new("far", "Far", model::Point::new(-f32::MAX, 0.))],
            ..Clipboard::default()
        };
        assert!(
            copied_selection_paste_command(
                &editor,
                Some(&distant),
                model::Point::new(f32::MAX, 0.),
                None
            )
            .is_err()
        );
        assert_eq!(editor.document(), model::FlowDocument::default());
        assert_eq!(clipboard, original);
    }

    #[test]
    fn paste_preview_obeys_host_contract_without_mutating_the_editor() {
        let editor = Editor::default();
        let clipboard = fragment();
        let reject: FlowDocumentValidator = Arc::new(|document| {
            assert_eq!(document.graph.nodes.len(), 2);
            assert_eq!(document.graph.bounds().unwrap().origin, model::Point::ZERO);
            Err("This project does not accept these copied objects".into())
        });
        let error = copied_selection_paste_command(
            &editor,
            Some(&clipboard),
            model::Point::ZERO,
            Some(&reject),
        )
        .unwrap_err();
        assert_eq!(
            error,
            InteractionError::InvalidInput(
                "This project does not accept these copied objects".into()
            )
        );
        assert_eq!(editor.document(), model::FlowDocument::default());
        assert_eq!(clipboard, fragment());
    }
}
