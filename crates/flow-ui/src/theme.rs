//! Replaceable, deliberately small theme surface.
#[derive(Clone, Copy, Debug)]
pub struct FlowTheme {
    pub background: u32,
    pub panel: u32,
    pub card: u32,
    pub border: u32,
    pub foreground: u32,
    pub muted: u32,
    pub accent: u32,
    pub grid: u32,
}
impl Default for FlowTheme {
    fn default() -> Self {
        Self {
            background: 0x0b0f17,
            panel: 0x111827,
            card: 0x172131,
            border: 0x2a374b,
            foreground: 0xe6edf7,
            muted: 0x8292ac,
            accent: 0x63e2bf,
            grid: 0x253247,
        }
    }
}
