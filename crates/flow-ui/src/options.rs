//! Host-selectable chrome. Disable panels to embed the canvas in your own editor shell.
#[derive(Clone, Copy, Debug)]
pub struct FlowOptions {
    pub show_toolbar: bool,
    pub show_palette: bool,
    pub show_inspector: bool,
    pub show_status: bool,
    pub show_minimap: bool,
    pub show_controls: bool,
}
impl Default for FlowOptions {
    fn default() -> Self {
        Self {
            show_toolbar: true,
            show_palette: true,
            show_inspector: true,
            show_status: true,
            show_minimap: true,
            show_controls: true,
        }
    }
}
impl FlowOptions {
    pub fn canvas_only() -> Self {
        Self {
            show_toolbar: false,
            show_palette: false,
            show_inspector: false,
            show_status: false,
            show_minimap: true,
            show_controls: true,
        }
    }
}
