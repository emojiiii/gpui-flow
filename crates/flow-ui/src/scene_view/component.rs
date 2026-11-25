use super::{LeaferCompatibilityProfile, SceneView};
use gpui::{AppContext, Context, Entity};
use gpui_flow_engine::{ElementHandle, EngineError, SceneEngine};
use gpui_flow_interaction::{
    SelectionMode,
    scene::{SceneCommand, SceneInteractionError},
};
use gpui_flow_scene::{DisplayId, DisplayObject, Scene, SceneError, SceneNode};

impl SceneView {
    /// Create an independently owned scene component for a GPUI layout.
    pub fn mount(scene: Scene, cx: &mut impl AppContext) -> Result<Entity<Self>, SceneError> {
        let engine = SceneEngine::new(scene)?;
        Ok(cx.new(|cx| Self::from_engine(engine, cx)))
    }

    /// Mount with the supported Leafer pointer and editor defaults. Scene data
    /// and its authored layout and paint settings are preserved.
    pub fn mount_with_profile(
        scene: Scene,
        profile: LeaferCompatibilityProfile,
        cx: &mut impl AppContext,
    ) -> Result<Entity<Self>, SceneError> {
        let engine = SceneEngine::new(scene)?;
        Ok(cx.new(|cx| Self::from_engine_with_profile(engine, profile, cx)))
    }

    pub fn scene(&self) -> &Scene {
        self.editor.scene()
    }

    /// Look up an attached element by ID in this scene tree.
    pub fn find_id(&self, id: &DisplayId) -> Option<ElementHandle> {
        self.scene().get(id)?;
        self.editor.engine.handle(id)
    }

    /// Read an attached element owned by this view.
    pub fn object(&self, handle: ElementHandle) -> Result<&DisplayObject, EngineError> {
        Ok(&self.attached_node(handle)?.object)
    }

    /// The ordered direct children of a container, or the roots of this view.
    pub fn children(
        &self,
        parent: Option<ElementHandle>,
    ) -> Result<Vec<ElementHandle>, EngineError> {
        let ids = match parent {
            Some(parent) => self.attached_node(parent)?.children.as_slice(),
            None => self.scene().roots(),
        };
        Ok(ids
            .iter()
            .map(|id| {
                self.editor
                    .engine
                    .handle(id)
                    .expect("attached child has a handle")
            })
            .collect())
    }

    /// The parent of an attached element. A scene root has no parent.
    pub fn parent(&self, handle: ElementHandle) -> Result<Option<ElementHandle>, EngineError> {
        Ok(self.attached_node(handle)?.parent.as_ref().map(|id| {
            self.editor
                .engine
                .handle(id)
                .expect("attached parent has a handle")
        }))
    }

    fn attached_node(&self, handle: ElementHandle) -> Result<&SceneNode, EngineError> {
        let id = self.editor.engine.attached_id(handle)?;
        Ok(self
            .scene()
            .get(&id)
            .expect("attached handle belongs to the scene"))
    }

    /// Append an object to the root list as one undoable edit.
    pub fn add(
        &mut self,
        object: DisplayObject,
        cx: &mut Context<Self>,
    ) -> Result<ElementHandle, SceneInteractionError> {
        self.add_under(None, object, cx)
    }

    /// Append an object under an attached container of this view.
    pub fn add_child(
        &mut self,
        parent: ElementHandle,
        object: DisplayObject,
        cx: &mut Context<Self>,
    ) -> Result<ElementHandle, SceneInteractionError> {
        let parent_id = self
            .editor
            .engine
            .attached_id(parent)
            .map_err(|error| SceneInteractionError::InvalidInput(error.to_string()))?;
        self.add_under(Some(parent_id), object, cx)
    }

    fn add_under(
        &mut self,
        parent: Option<DisplayId>,
        object: DisplayObject,
        cx: &mut Context<Self>,
    ) -> Result<ElementHandle, SceneInteractionError> {
        let id = object.id.clone();
        self.execute_checked_command(
            SceneCommand::AddObject { object, parent: parent.clone(), index: None },
            |command| {
                if !matches!(command, SceneCommand::AddObject { object, parent: actual_parent, index: None }
                    if object.id == id && *actual_parent == parent)
                {
                    return Err(SceneInteractionError::InvalidInput(
                        "middleware cannot change the child ID or its attachment".into(),
                    ));
                }
                Ok(())
            },
            cx,
        )?;
        Ok(self
            .find_id(&id)
            .expect("successful add attaches this object"))
    }

    /// Remove an attached subtree as one undoable edit.
    pub fn remove(
        &mut self,
        handle: ElementHandle,
        cx: &mut Context<Self>,
    ) -> Result<(), SceneInteractionError> {
        let id = self
            .editor
            .engine
            .attached_id(handle)
            .map_err(|error| SceneInteractionError::InvalidInput(error.to_string()))?;
        self.execute_checked_command(
            SceneCommand::RemoveObject { id: id.clone() },
            |command| {
                if !matches!(command, SceneCommand::RemoveObject { id: target } if *target == id) {
                    return Err(SceneInteractionError::InvalidInput(
                        "middleware cannot change the removed object".into(),
                    ));
                }
                Ok(())
            },
            cx,
        )
    }

    /// Replace the selection with one attached element of this view.
    pub fn select(
        &mut self,
        handle: ElementHandle,
        cx: &mut Context<Self>,
    ) -> Result<(), SceneInteractionError> {
        let id = self
            .editor
            .engine
            .attached_id(handle)
            .map_err(|error| SceneInteractionError::InvalidInput(error.to_string()))?;
        self.execute_checked_command(
            SceneCommand::Select { id: id.clone(), mode: SelectionMode::Replace },
            |command| {
                if !matches!(command, SceneCommand::Select { id: target, mode: SelectionMode::Replace } if *target == id) {
                    return Err(SceneInteractionError::InvalidInput(
                        "middleware cannot change the selected object or selection mode".into(),
                    ));
                }
                Ok(())
            },
            cx,
        )
    }

    pub fn selected_elements(&self) -> impl Iterator<Item = ElementHandle> + '_ {
        self.editor
            .selection
            .ids
            .iter()
            .filter_map(|id| self.find_id(id))
    }

    pub fn can_undo(&self) -> bool {
        self.editor.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.editor.can_redo()
    }
}
