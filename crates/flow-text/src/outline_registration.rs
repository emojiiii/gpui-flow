//! Startup-only font binding for the audited, Cargo.lock-pinned Linux backend.
//!
//! A caller-provided `Platform` cannot prove its implementation or ownership.
//! Only the dedicated factory below can create bindings. No application, platform,
//! callback, executor or binding escapes before the entire batch is verified.
use crate::{
    GpuiTextRasterizer,
    static_outlines::{PreparedFont, StaticStyle},
};
use gpui::{Application, Font, FontId, TextSystem};
use std::{fmt, sync::Arc};

/// One immutable, host-authorized face and the exact style the host will request.
/// Prepare all fonts with `PreparedFont::prepare` before starting the application.
pub struct StartupOutlineFont {
    pub font: Arc<PreparedFont>,
    pub style: StaticStyle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistrationPhase {
    Probe,
    Register,
    Resolve,
    ConstructApplication,
    VerifyApplication,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutlineBindingError {
    /// Generic platforms, other operating systems and builds without an explicit
    /// `static-outline-x11` or `static-outline-wayland` feature are unsupported.
    Unsupported,
    EmptyBatch,
    Limit,
    DuplicateAlias,
    StyleMismatch,
    AliasCollision,
    DuplicateFontId,
    Native {
        phase: RegistrationPhase,
        message: String,
    },
    NativePanicked,
    ApplicationMismatch,
    WrongTextSystem,
    UnboundFont,
    StaleSnapshot,
}
impl fmt::Display for OutlineBindingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "static outline binding: {self:?}")
    }
}
impl std::error::Error for OutlineBindingError {}

// All fields and constructors are private. The prepared object retains the exact
// registration bytes/hash/index/style; the owner Arc cannot be inferred from IDs.
struct Binding {
    id: FontId,
    descriptor: Font,
    font: Arc<PreparedFont>,
    registered_epoch: u64,
}
pub(crate) struct OutlineBindings {
    owner: Arc<TextSystem>,
    fonts: Vec<Binding>,
}
impl OutlineBindings {
    pub(crate) fn descriptors(&self) -> Vec<Font> {
        self.fonts
            .iter()
            .map(|binding| binding.descriptor.clone())
            .collect()
    }

    pub(crate) fn lookup(
        &self,
        owner: &Arc<TextSystem>,
        id: FontId,
        style: StaticStyle,
        epoch: u64,
    ) -> Result<&PreparedFont, OutlineBindingError> {
        // This identity check MUST precede the integer FontId lookup.
        if !Arc::ptr_eq(&self.owner, owner) {
            return Err(OutlineBindingError::WrongTextSystem);
        }
        lookup_binding(&self.fonts, id, style, epoch)
    }
}

fn lookup_binding(
    fonts: &[Binding],
    id: FontId,
    style: StaticStyle,
    epoch: u64,
) -> Result<&PreparedFont, OutlineBindingError> {
    let binding = fonts
        .iter()
        .find(|binding| binding.id == id)
        .ok_or(OutlineBindingError::UnboundFont)?;
    if epoch < binding.registered_epoch {
        return Err(OutlineBindingError::StaleSnapshot);
    }
    binding
        .font
        .require_style(style)
        .map_err(|_| OutlineBindingError::StyleMismatch)?;
    Ok(&binding.font)
}

/// Create a new application and raster bridge with an all-or-nothing static-font
/// batch. Call once on the main thread, before any application is exposed/run.
///
/// The factory calls the pinned `gpui_platform::current_platform(false)` itself.
/// It never accepts an existing App, Platform or native text-system handle. On
/// any error or unwind, the entire private construction is dropped; retry by
/// preparing fresh fonts and starting a new construction, never reusing a failed
/// native family key. This does not implement stroke rasterization or Canvas text.
pub fn application_with_static_outlines(
    fonts: Vec<StartupOutlineFont>,
) -> Result<(Application, Arc<GpuiTextRasterizer>), OutlineBindingError> {
    #[cfg(all(
        target_os = "linux",
        any(feature = "static-outline-x11", feature = "static-outline-wayland")
    ))]
    {
        use std::panic::{AssertUnwindSafe, catch_unwind};
        catch_unwind(AssertUnwindSafe(|| {
            // Validate the complete immutable batch before constructing a backend.
            let fonts = transaction::admit(fonts)?;
            let platform = gpui_platform::current_platform(false);
            let raster = platform.text_system();
            let backend = native::NativeBackend {
                platform: Some(platform),
                raster: raster.clone(),
            };
            let (app, bindings) = transaction::register(backend, fonts)?;
            let shaping = app.text_system();
            let outlines = OutlineBindings {
                owner: shaping.clone(),
                fonts: bindings,
            };
            let bridge = GpuiTextRasterizer::from_outline_registration(shaping, raster, outlines);
            Ok((app, Arc::new(bridge)))
        }))
        .map_err(|_| OutlineBindingError::NativePanicked)?
    }
    #[cfg(not(all(
        target_os = "linux",
        any(feature = "static-outline-x11", feature = "static-outline-wayland")
    )))]
    {
        let _ = fonts;
        Err(OutlineBindingError::Unsupported)
    }
}

#[cfg(any(
    test,
    all(
        target_os = "linux",
        any(feature = "static-outline-x11", feature = "static-outline-wayland")
    )
))]
mod transaction {
    use super::*;
    use gpui::{FontFallbacks, FontStyle, FontWeight};
    use std::{
        collections::HashSet,
        panic::{AssertUnwindSafe, catch_unwind},
    };

    // Exact *outer* error emitted by both audited pinned matching implementations.
    pub(super) const ABSENT: &str =
        "requested font family contains no font matching the other parameters";
    const MAX_FONTS: usize = 32;
    const MAX_BATCH_BYTES: usize = 64 * 1024 * 1024;

    pub(super) trait Backend {
        type Application;
        fn font_id(&mut self, descriptor: &Font) -> Result<FontId, String>;
        fn add_font(&mut self, bytes: &[u8]) -> Result<(), String>;
        fn application(&mut self) -> Result<Self::Application, String>;
        fn app_font_id(
            &mut self,
            app: &Self::Application,
            descriptor: &Font,
        ) -> Result<FontId, String>;
    }

    pub(super) fn admit(
        fonts: Vec<StartupOutlineFont>,
    ) -> Result<Vec<StartupOutlineFont>, OutlineBindingError> {
        if fonts.is_empty() {
            return Err(OutlineBindingError::EmptyBatch);
        }
        if fonts.len() > MAX_FONTS {
            return Err(OutlineBindingError::Limit);
        }
        let mut aliases = HashSet::new();
        let mut bytes = 0usize;
        for request in &fonts {
            request
                .font
                .require_style(request.style)
                .map_err(|_| OutlineBindingError::StyleMismatch)?;
            if !aliases.insert(request.font.private_alias()) {
                return Err(OutlineBindingError::DuplicateAlias);
            }
            bytes = bytes
                .checked_add(request.font.bytes().len())
                .ok_or(OutlineBindingError::Limit)?;
            if bytes > MAX_BATCH_BYTES {
                return Err(OutlineBindingError::Limit);
            }
        }
        Ok(fonts)
    }

    fn descriptor(request: &StartupOutlineFont) -> Font {
        let mut descriptor = gpui::font(request.font.private_alias().to_owned());
        descriptor.weight = FontWeight(f32::from(request.style.weight));
        descriptor.style = if request.style.italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        };
        descriptor
    }
    fn native(phase: RegistrationPhase, message: String) -> OutlineBindingError {
        OutlineBindingError::Native { phase, message }
    }

    // The backend is owned, never borrowed from a host; no failed partial state can
    // escape. No public callbacks or executor tasks are accepted by this machine.
    pub(super) fn register<B: Backend>(
        mut backend: B,
        fonts: Vec<StartupOutlineFont>,
    ) -> Result<(B::Application, Vec<Binding>), OutlineBindingError> {
        catch_unwind(AssertUnwindSafe(|| {
            let mut pending = Vec::with_capacity(fonts.len());
            for request in fonts {
                let mut descriptor = descriptor(&request);
                // Sacrifice only None. Cosmic caches even failed family queries,
                // and its family key does not contain weight/style.
                match backend.font_id(&descriptor) {
                    Ok(_) => return Err(OutlineBindingError::AliasCollision),
                    Err(error) if error == ABSENT => {}
                    Err(error) => return Err(native(RegistrationPhase::Probe, error)),
                }
                descriptor.fallbacks = Some(FontFallbacks::default());
                pending.push((request.font, descriptor));
            }
            let mut bindings = Vec::with_capacity(pending.len());
            let mut ids = HashSet::new();
            for (font, descriptor) in pending {
                // Exactly one admitted single-face buffer per registration call.
                backend
                    .add_font(font.bytes())
                    .map_err(|e| native(RegistrationPhase::Register, e))?;
                // Ok from add_fonts is not evidence of a load. The previously
                // unused Some(empty) key MUST directly resolve before App fallback.
                let id = backend
                    .font_id(&descriptor)
                    .map_err(|e| native(RegistrationPhase::Resolve, e))?;
                if !ids.insert(id) {
                    return Err(OutlineBindingError::DuplicateFontId);
                }
                bindings.push(Binding {
                    id,
                    descriptor,
                    font,
                    registered_epoch: 0,
                });
            }
            let app = backend
                .application()
                .map_err(|e| native(RegistrationPhase::ConstructApplication, e))?;
            for binding in &bindings {
                let app_id = backend
                    .app_font_id(&app, &binding.descriptor)
                    .map_err(|e| native(RegistrationPhase::VerifyApplication, e))?;
                if app_id != binding.id {
                    return Err(OutlineBindingError::ApplicationMismatch);
                }
            }
            Ok((app, bindings))
        }))
        .map_err(|_| OutlineBindingError::NativePanicked)?
    }
}

#[cfg(all(
    target_os = "linux",
    any(feature = "static-outline-x11", feature = "static-outline-wayland")
))]
mod native {
    use super::*;
    use gpui::{Platform, PlatformTextSystem};
    use std::{borrow::Cow, rc::Rc};
    pub(super) struct NativeBackend {
        pub(super) platform: Option<Rc<dyn Platform>>,
        pub(super) raster: Arc<dyn PlatformTextSystem>,
    }
    impl transaction::Backend for NativeBackend {
        type Application = Application;
        fn font_id(&mut self, descriptor: &Font) -> Result<FontId, String> {
            self.raster.font_id(descriptor).map_err(|e| e.to_string())
        }
        fn add_font(&mut self, bytes: &[u8]) -> Result<(), String> {
            self.raster
                .add_fonts(vec![Cow::Owned(bytes.to_vec())])
                .map_err(|e| e.to_string())
        }
        fn application(&mut self) -> Result<Application, String> {
            Ok(Application::with_platform(
                self.platform
                    .take()
                    .expect("private platform consumed once"),
            ))
        }
        fn app_font_id(&mut self, app: &Application, descriptor: &Font) -> Result<FontId, String> {
            Ok(app.text_system().resolve_font(descriptor))
        }
    }
}

#[cfg(test)]
mod tests;
