//! Mechanical forwarding: every platform behavior stays with GPUI's backend.
use super::*;
use anyhow::Result;
use futures::channel::oneshot;
use gpui::*;
use smallvec::SmallVec;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};
macro_rules! forward {
    () => {};
    (fn $name:ident(&self $(, $arg:ident: $ty:ty)* $(,)?) -> $ret:ty; $($rest:tt)*) => {
        fn $name(&self, $($arg: $ty),*) -> $ret { self.inner.$name($($arg),*) }
        forward!($($rest)*);
    };
    (fn $name:ident(&mut self $(, $arg:ident: $ty:ty)* $(,)?) -> $ret:ty; $($rest:tt)*) => {
        fn $name(&mut self, $($arg: $ty),*) -> $ret { self.inner.$name($($arg),*) }
        forward!($($rest)*);
    };
}
impl Platform for GpuPlatform {
    forward! {
        fn background_executor(&self) -> BackgroundExecutor;
        fn foreground_executor(&self) -> ForegroundExecutor;
        fn text_system(&self) -> Arc<dyn PlatformTextSystem>;
        fn run(&self, on_finish_launching: Box<dyn 'static + FnOnce()>) -> ();
        fn quit(&self) -> ();
        fn restart(&self, binary_path: Option<PathBuf>, arguments: Vec<OsString>) -> ();
        fn activate(&self, ignoring_other_apps: bool) -> ();
        fn hide(&self) -> ();
        fn hide_other_apps(&self) -> ();
        fn unhide_other_apps(&self) -> ();
        fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>>;
        fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>>;
        fn active_window(&self) -> Option<AnyWindowHandle>;
        fn window_stack(&self) -> Option<Vec<AnyWindowHandle>>;
        fn is_screen_capture_supported(&self) -> bool;
        fn screen_capture_sources( &self, ) -> oneshot::Receiver<anyhow::Result<Vec<Rc<dyn ScreenCaptureSource>>>>;
        fn window_appearance(&self) -> WindowAppearance;
        fn set_window_appearance(&self, _appearance: Option<WindowAppearance>) -> ();
        fn button_layout(&self) -> Option<WindowButtonLayout>;
        fn open_url(&self, url: &str) -> ();
        fn on_open_urls(&self, callback: Box<dyn FnMut(Vec<String>)>) -> ();
        fn register_url_scheme(&self, url: &str) -> Task<Result<()>>;
        fn prompt_for_paths( &self, options: PathPromptOptions, ) -> oneshot::Receiver<Result<Option<Vec<PathBuf>>>>;
        fn prompt_for_new_path( &self, directory: &Path, suggested_name: Option<&str>, ) -> oneshot::Receiver<Result<Option<PathBuf>>>;
        fn can_select_mixed_files_and_dirs(&self) -> bool;
        fn reveal_path(&self, path: &Path) -> ();
        fn open_with_system(&self, path: &Path) -> ();
        fn on_quit(&self, callback: Box<dyn FnMut() -> bool>) -> ();
        fn on_reopen(&self, callback: Box<dyn FnMut()>) -> ();
        fn on_system_sleep(&self, callback: Box<dyn FnMut()>) -> ();
        fn on_system_wake(&self, callback: Box<dyn FnMut()>) -> ();
        fn on_app_lifecycle(&self, _callback: Box<dyn FnMut(AppLifecyclePhase)>) -> ();
        fn on_memory_warning(&self, _callback: Box<dyn FnMut()>) -> ();
        fn gestures(&self) -> Option<Rc<dyn PlatformGestures>>;
        fn set_menus(&self, menus: Vec<Menu>, keymap: &Keymap) -> ();
        fn get_menus(&self) -> Option<Vec<OwnedMenu>>;
        fn set_dock_menu(&self, menu: Vec<MenuItem>, keymap: &Keymap) -> ();
        fn perform_dock_menu_action(&self, _action: usize) -> ();
        fn add_recent_document(&self, _path: &Path) -> ();
        fn update_jump_list( &self, _menus: Vec<MenuItem>, _entries: Vec<SmallVec<[PathBuf; 2]>>, ) -> Task<Vec<SmallVec<[PathBuf; 2]>>>;
        fn on_app_menu_action(&self, callback: Box<dyn FnMut(&dyn Action)>) -> ();
        fn on_will_open_app_menu(&self, callback: Box<dyn FnMut()>) -> ();
        fn on_validate_app_menu_command(&self, callback: Box<dyn FnMut(&dyn Action) -> bool>) -> ();
        fn thermal_state(&self) -> ThermalState;
        fn on_thermal_state_change(&self, callback: Box<dyn FnMut()>) -> ();
        fn prevent_idle_sleep(&self, reason: &str) -> Task<Result<ActivityGuard>>;
        fn set_app_identity(&self, identifier: &str, name: &str) -> ();
        fn show_system_notification(&self, notification: SystemNotification) -> ();
        fn dismiss_system_notification(&self, tag: &str) -> ();
        fn on_system_notification_response( &self, callback: Box<dyn FnMut(SystemNotificationResponse)>, ) -> ();
        fn compositor_name(&self) -> &'static str;
        fn app_path(&self) -> Result<PathBuf>;
        fn path_for_auxiliary_executable(&self, name: &str) -> Result<PathBuf>;
        fn set_cursor_style(&self, style: CursorStyle) -> ();
        fn hide_cursor_until_mouse_moves(&self) -> ();
        fn is_cursor_visible(&self) -> bool;
        fn should_auto_hide_scrollbars(&self) -> bool;
        fn read_from_clipboard(&self) -> Option<ClipboardItem>;
        fn write_to_clipboard(&self, item: ClipboardItem) -> ();
        fn read_from_clipboard_async(&self) -> Task<Result<Option<ClipboardItem>, ClipboardReadError>>;
        fn read_from_primary(&self) -> Option<ClipboardItem>;
        fn write_to_primary(&self, item: ClipboardItem) -> ();
        fn write_credentials(&self, url: &str, username: &str, password: &[u8]) -> Task<Result<()>>;
        fn read_credentials(&self, url: &str) -> Task<Result<Option<(String, Vec<u8>)>>>;
        fn delete_credentials(&self, url: &str) -> Task<Result<()>>;
        fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout>;
        fn keyboard_mapper(&self) -> Rc<dyn PlatformKeyboardMapper>;
        fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut()>) -> ();
    }
    fn open_window(
        &self,
        handle: AnyWindowHandle,
        options: WindowParams,
    ) -> Result<Box<dyn PlatformWindow>> {
        let inner = self.inner.open_window(handle, options)?;
        Ok(Box::new(GpuWindow::new(inner, self.host.clone(), handle)))
    }
}
impl PlatformWindow for GpuWindow {
    forward! {
        fn bounds(&self) -> Bounds<Pixels>;
        fn is_maximized(&self) -> bool;
        fn window_bounds(&self) -> WindowBounds;
        fn content_size(&self) -> Size<Pixels>;
        fn visual_viewport_bounds(&self) -> Bounds<Pixels>;
        fn on_visual_viewport_changed(&self, _callback: Box<dyn FnMut()>) -> ();
        fn prepare_frame(&self) -> bool;
        fn resize(&mut self, size: Size<Pixels>) -> ();
        fn scale_factor(&self) -> f32;
        fn appearance(&self) -> WindowAppearance;
        fn display(&self) -> Option<Rc<dyn PlatformDisplay>>;
        fn mouse_position(&self) -> Point<Pixels>;
        fn modifiers(&self) -> Modifiers;
        fn capslock(&self) -> Capslock;
        fn set_input_handler(&mut self, input_handler: PlatformInputHandler) -> ();
        fn take_input_handler(&mut self) -> Option<PlatformInputHandler>;
        fn set_text_input_configuration(&mut self, _configuration: TextInputConfiguration) -> ();
        fn prompt( &self, level: PromptLevel, msg: &str, detail: Option<&str>, answers: &[PromptButton], ) -> Option<oneshot::Receiver<usize>>;
        fn activate(&self) -> ();
        fn request_attention(&self) -> ();
        fn is_active(&self) -> bool;
        fn visibility(&self) -> WindowVisibility;
        fn is_hovered(&self) -> bool;
        fn background_appearance(&self) -> WindowBackgroundAppearance;
        fn set_title(&mut self, title: &str) -> ();
        fn set_background_appearance(&self, background_appearance: WindowBackgroundAppearance) -> ();
        fn minimize(&self) -> ();
        fn zoom(&self) -> ();
        fn toggle_fullscreen(&self) -> ();
        fn is_fullscreen(&self) -> bool;
        fn frame_waker(&self) -> Option<Rc<dyn Fn()>>;
        fn on_request_frame(&self, callback: Box<dyn FnMut(RequestFrameOptions)>) -> ();
        fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult>) -> ();
        fn on_active_status_change(&self, callback: Box<dyn FnMut(bool)>) -> ();
        fn on_visibility_change(&self, callback: Box<dyn FnMut(WindowVisibility)>) -> ();
        fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool)>) -> ();
        fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32)>) -> ();
        fn on_moved(&self, callback: Box<dyn FnMut()>) -> ();
        fn on_should_close(&self, callback: Box<dyn FnMut() -> bool>) -> ();
        fn on_hit_test_window_control(&self, callback: Box<dyn FnMut() -> Option<WindowControlArea>>) -> ();
        fn on_close(&self, callback: Box<dyn FnOnce()>) -> ();
        fn on_appearance_changed(&self, callback: Box<dyn FnMut()>) -> ();
        fn on_button_layout_changed(&self, _callback: Box<dyn FnMut()>) -> ();
        fn schedule_frame(&self) -> ();
        fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas>;
        fn is_subpixel_rendering_supported(&self) -> bool;
        fn get_title(&self) -> String;
        fn tabbed_windows(&self) -> Option<Vec<SystemWindowTab>>;
        fn tab_bar_visible(&self) -> bool;
        fn set_edited(&mut self, _edited: bool) -> ();
        fn set_document_path(&self, _path: Option<&std::path::Path>) -> ();
        fn toggle_simple_fullscreen(&self) -> ();
        fn is_simple_fullscreen(&self) -> bool;
        fn show_character_palette(&self) -> ();
        fn titlebar_double_click(&self, _is_resizable: bool, _is_minimizable: bool) -> ();
        fn on_move_tab_to_new_window(&self, _callback: Box<dyn FnMut()>) -> ();
        fn on_merge_all_windows(&self, _callback: Box<dyn FnMut()>) -> ();
        fn on_select_previous_tab(&self, _callback: Box<dyn FnMut()>) -> ();
        fn on_select_next_tab(&self, _callback: Box<dyn FnMut()>) -> ();
        fn on_toggle_tab_bar(&self, _callback: Box<dyn FnMut()>) -> ();
        fn merge_all_windows(&self) -> ();
        fn move_tab_to_new_window(&self) -> ();
        fn toggle_window_tab_overview(&self) -> ();
        fn set_tabbing_identifier(&self, _identifier: Option<String>) -> ();
        fn native_window_state(&self) -> Option<Vec<u8>>;
        fn restore_native_window_state(&self, _state: &[u8]) -> ();
        fn inner_window_bounds(&self) -> WindowBounds;
        fn request_decorations(&self, _decorations: WindowDecorations) -> ();
        fn show_window_menu(&self, _position: Point<Pixels>) -> ();
        fn start_window_move(&self) -> ();
        fn can_start_external_drag(&self) -> bool;
        fn start_external_drag(&self, _payload: &ExternalDragPayload) -> bool;
        fn start_window_resize(&self, _edge: ResizeEdge) -> ();
        fn set_exclusive_zone(&self, _zone: Pixels) -> ();
        fn set_exclusive_edge(&self, _edge: layer_shell::Anchor) -> ();
        fn set_input_region(&self, _region: Option<&[Bounds<Pixels>]>) -> ();
        fn window_decorations(&self) -> Decorations;
        fn set_app_id(&mut self, _app_id: &str) -> ();
        fn map_window(&mut self) -> anyhow::Result<()>;
        fn window_controls(&self) -> WindowControls;
        fn set_client_inset(&self, _inset: Pixels) -> ();
        fn gpu_specs(&self) -> Option<GpuSpecs>;
        fn update_ime_position(&self, _bounds: Bounds<Pixels>) -> ();
        fn insets(&self) -> WindowInsets;
        fn on_insets_changed(&self, _callback: Box<dyn FnMut(WindowInsets)>) -> ();
        fn set_back_handler(&self, _callback: Box<dyn FnMut()>) -> ();
        fn set_back_enabled(&self, _enabled: bool) -> ();
        fn show_soft_keyboard(&self) -> ();
        fn hide_soft_keyboard(&self) -> ();
        fn text_input_state_changed(&self, _change: TextInputStateChange) -> ();
        fn play_system_bell(&self) -> ();
        fn a11y_init(&self, _callbacks: A11yCallbacks) -> ();
        fn a11y_tree_update(&self, _tree_update: accesskit::TreeUpdate) -> ();
        fn a11y_update_window_bounds(&self) -> ();
    }
    fn draw(&self, scene: &gpui::Scene) {
        self.draw_direct(scene);
    }
    fn draw_composed(&self, scene: ComposedScene<'_>) {
        self.draw_flow(scene);
    }
    fn enable_window_composition(&self) -> Result<()> {
        Ok(())
    }
    fn create_native_surface(&self) -> Result<Rc<dyn PlatformSurfaceAttachment>> {
        anyhow::bail!("Flow GPU windows support inline GPUI content only")
    }
    fn set_composition_order(&self, surfaces: &[PlatformCompositionSurface]) -> Result<()> {
        self.accept_order(surfaces)
    }
}
impl raw_window_handle::HasWindowHandle for GpuWindow {
    fn window_handle(
        &self,
    ) -> std::result::Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError>
    {
        self.inner.window_handle()
    }
}
impl raw_window_handle::HasDisplayHandle for GpuWindow {
    fn display_handle(
        &self,
    ) -> std::result::Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError>
    {
        self.inner.display_handle()
    }
}
