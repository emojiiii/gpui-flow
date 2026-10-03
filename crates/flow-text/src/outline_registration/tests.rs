use super::transaction::{ABSENT, Backend, admit, register};
use super::*;
use gpui::{FontFallbacks, FontFeatures, FontWeight};
use std::{collections::HashMap, sync::Mutex};

#[derive(Default, Debug)]
struct Trace {
    descriptors: Vec<Font>,
    added_bytes: Vec<Vec<u8>>,
    backend_drops: usize,
    apps_created: usize,
    app_drops: usize,
}
#[derive(Clone, Copy, Default)]
enum Fault {
    #[default]
    None,
    IgnoreAdd(usize),
    Error(RegistrationPhase),
    ProbeMessage(&'static str),
    Panic(RegistrationPhase),
    WrongAppId,
    DuplicateId,
}
struct ModelApplication(Arc<Mutex<Trace>>);
impl Drop for ModelApplication {
    fn drop(&mut self) {
        self.0.lock().unwrap().app_drops += 1;
    }
}
#[derive(Hash, PartialEq, Eq)]
struct FamilyKey(String, FontFeatures, Option<FontFallbacks>);
impl From<&Font> for FamilyKey {
    fn from(font: &Font) -> Self {
        Self(
            font.family.to_string(),
            font.features.clone(),
            font.fallbacks.clone(),
        )
    }
}
struct ModelBackend {
    trace: Arc<Mutex<Trace>>,
    expected: Vec<Arc<PreparedFont>>,
    database: HashMap<String, FontId>,
    // Like the pinned native cache: both absent and present results are sticky,
    // keys include features/fallback option, and exclude weight and style.
    cache: HashMap<FamilyKey, Option<FontId>>,
    fault: Fault,
    add_calls: usize,
}
impl ModelBackend {
    fn new(fonts: &[StartupOutlineFont], fault: Fault) -> Self {
        Self {
            trace: Arc::default(),
            expected: fonts.iter().map(|font| font.font.clone()).collect(),
            database: HashMap::new(),
            cache: HashMap::new(),
            fault,
            add_calls: 0,
        }
    }
    fn fail(&self, phase: RegistrationPhase) -> Result<(), String> {
        match self.fault {
            Fault::ProbeMessage(message) if phase == RegistrationPhase::Probe => {
                Err(message.into())
            }
            Fault::Error(at) if at == phase => Err("loader unavailable".into()),
            Fault::Panic(at) if at == phase => panic!("modeled native panic"),
            _ => Ok(()),
        }
    }
}
impl Drop for ModelBackend {
    fn drop(&mut self) {
        self.trace.lock().unwrap().backend_drops += 1;
    }
}
impl Backend for ModelBackend {
    type Application = ModelApplication;
    fn font_id(&mut self, descriptor: &Font) -> Result<FontId, String> {
        self.trace
            .lock()
            .unwrap()
            .descriptors
            .push(descriptor.clone());
        self.fail(if descriptor.fallbacks.is_none() {
            RegistrationPhase::Probe
        } else {
            RegistrationPhase::Resolve
        })?;
        let id = *self
            .cache
            .entry(descriptor.into())
            .or_insert_with(|| self.database.get(descriptor.family.as_ref()).copied());
        id.ok_or_else(|| ABSENT.into())
    }
    fn add_font(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.fail(RegistrationPhase::Register)?;
        let expected = &self.expected[self.add_calls];
        assert_eq!(
            bytes,
            expected.bytes().as_ref(),
            "exact prepared buffer must be registered"
        );
        self.trace.lock().unwrap().added_bytes.push(bytes.to_vec());
        let ignore = matches!(self.fault, Fault::IgnoreAdd(call) if call == self.add_calls);
        if !ignore {
            let id = if matches!(self.fault, Fault::DuplicateId) {
                7
            } else {
                7 + self.add_calls
            };
            self.database
                .insert(expected.private_alias().into(), FontId(id));
        }
        self.add_calls += 1;
        Ok(())
    }
    fn application(&mut self) -> Result<ModelApplication, String> {
        self.fail(RegistrationPhase::ConstructApplication)?;
        self.trace.lock().unwrap().apps_created += 1;
        Ok(ModelApplication(self.trace.clone()))
    }
    fn app_font_id(
        &mut self,
        _app: &ModelApplication,
        descriptor: &Font,
    ) -> Result<FontId, String> {
        self.fail(RegistrationPhase::VerifyApplication)?;
        if matches!(self.fault, Fault::WrongAppId) {
            return Ok(FontId(999));
        }
        self.font_id(descriptor)
    }
}

fn requests(count: usize) -> Vec<StartupOutlineFont> {
    (0..count)
        .map(|index| {
            let bytes: &[u8] = if index % 2 == 0 {
                include_bytes!("../../tests/fixtures/outline-quadratic.ttf")
            } else {
                include_bytes!("../../tests/fixtures/outline-cubic.otf")
            };
            let font = Arc::new(PreparedFont::prepare(bytes).unwrap());
            let style = font.identity().style;
            StartupOutlineFont { font, style }
        })
        .collect()
}
fn run(
    backend: ModelBackend,
    requests: Vec<StartupOutlineFont>,
) -> Result<(ModelApplication, Vec<Binding>), OutlineBindingError> {
    register(backend, admit(requests)?)
}
fn failed<T>(result: Result<T, OutlineBindingError>) -> OutlineBindingError {
    match result {
        Ok(_) => panic!("unexpected successful publication"),
        Err(error) => error,
    }
}
fn owner() -> Arc<TextSystem> {
    Arc::new(TextSystem::new(Arc::new(gpui::NoopTextSystem::new())))
}

#[test]
fn single_and_multiple_faces_bind_exact_bytes_style_and_descriptor() {
    for count in [1, 2] {
        let requests = requests(count);
        let fonts: Vec<_> = requests
            .iter()
            .map(|request| request.font.clone())
            .collect();
        let backend = ModelBackend::new(&requests, Fault::None);
        let trace = backend.trace.clone();
        let (app, bindings) = run(backend, requests).unwrap();
        assert_eq!(bindings.len(), count);
        for (index, binding) in bindings.iter().enumerate() {
            assert!(Arc::ptr_eq(&binding.font, &fonts[index]));
            assert_eq!(binding.font.identity().face_index, 0);
            assert_eq!(
                binding.descriptor.family.as_ref(),
                fonts[index].private_alias()
            );
            assert_eq!(binding.descriptor.features, FontFeatures::default());
            assert_eq!(binding.descriptor.fallbacks, Some(FontFallbacks::default()));
            assert_eq!(
                binding.descriptor.weight,
                FontWeight(f32::from(fonts[index].identity().style.weight))
            );
            assert_eq!(binding.registered_epoch, 0);
        }
        let trace_guard = trace.lock().unwrap();
        assert_eq!(trace_guard.added_bytes.len(), count);
        assert_eq!(trace_guard.backend_drops, 1);
        assert_eq!(trace_guard.apps_created, 1);
        assert_eq!(trace_guard.app_drops, 0);
        // Every sacrificial lookup precedes registration/live/App lookups.
        assert!(
            trace_guard.descriptors[..count]
                .iter()
                .all(|font| font.fallbacks.is_none())
        );
        assert!(
            trace_guard.descriptors[count..]
                .iter()
                .all(|font| font.fallbacks.is_some())
        );
        drop(trace_guard);
        drop(app);
        assert_eq!(trace.lock().unwrap().app_drops, 1);
    }
}

#[test]
fn admission_rejects_duplicates_exact_style_mismatch_and_limits_before_native_work() {
    let mut requests = requests(1);
    requests.push(StartupOutlineFont {
        font: requests[0].font.clone(),
        style: requests[0].style,
    });
    assert_eq!(failed(admit(requests)), OutlineBindingError::DuplicateAlias);
    for change_italic in [false, true] {
        let mut requests = self::requests(1);
        if change_italic {
            requests[0].style.italic = !requests[0].style.italic;
        } else {
            requests[0].style.weight += 1;
        }
        assert_eq!(failed(admit(requests)), OutlineBindingError::StyleMismatch);
    }
    assert_eq!(failed(admit(Vec::new())), OutlineBindingError::EmptyBatch);
    let font = self::requests(1).remove(0).font;
    let oversized = (0..33)
        .map(|_| StartupOutlineFont {
            style: font.identity().style,
            font: font.clone(),
        })
        .collect();
    assert_eq!(failed(admit(oversized)), OutlineBindingError::Limit);
}

#[test]
fn existing_family_or_secondary_alias_collision_aborts_before_add() {
    // Native load_family checks every face.families entry, not only its first.
    for secondary in [false, true] {
        let requests = requests(2);
        let mut backend = ModelBackend::new(&requests, Fault::None);
        if secondary {
            backend
                .database
                .insert("unrelated primary family".into(), FontId(21));
        }
        backend
            .database
            .insert(requests[1].font.private_alias().into(), FontId(21));
        let trace = backend.trace.clone();
        assert_eq!(
            failed(run(backend, requests)),
            OutlineBindingError::AliasCollision
        );
        let trace = trace.lock().unwrap();
        assert!(trace.added_bytes.is_empty());
        assert_eq!(trace.apps_created, 0);
        assert_eq!(trace.backend_drops, 1);
    }
}

#[test]
fn only_exact_outer_absence_error_is_accepted() {
    for message in [
        "loader unavailable",
        "context: requested font family contains no font matching the other parameters",
        "requested font family contains no font matching the other parameters: caused by corrupt font",
    ] {
        let requests = requests(1);
        let backend = ModelBackend::new(&requests, Fault::ProbeMessage(message));
        let trace = backend.trace.clone();
        assert_eq!(
            failed(run(backend, requests)),
            OutlineBindingError::Native {
                phase: RegistrationPhase::Probe,
                message: message.into(),
            }
        );
        assert!(trace.lock().unwrap().added_bytes.is_empty());
        assert_eq!(trace.lock().unwrap().apps_created, 0);
    }
}

#[test]
fn none_probe_does_not_poison_some_empty_but_weight_changes_cannot_repair_a_key() {
    let requests = requests(1);
    let mut backend = ModelBackend::new(&requests, Fault::None);
    let mut probe = gpui::font(requests[0].font.private_alias().to_owned());
    assert_eq!(backend.font_id(&probe).unwrap_err(), ABSENT);
    backend.add_font(requests[0].font.bytes()).unwrap();
    probe.weight = FontWeight::BOLD;
    assert_eq!(backend.font_id(&probe).unwrap_err(), ABSENT);
    probe.fallbacks = Some(FontFallbacks::default());
    assert_eq!(backend.font_id(&probe).unwrap(), FontId(7));

    let requests = self::requests(1);
    let mut backend = ModelBackend::new(&requests, Fault::None);
    let mut live = gpui::font(requests[0].font.private_alias().to_owned());
    live.fallbacks = Some(FontFallbacks::default());
    assert_eq!(backend.font_id(&live).unwrap_err(), ABSENT);
    backend.add_font(requests[0].font.bytes()).unwrap();
    live.weight = FontWeight::BOLD;
    live.style = gpui::FontStyle::Italic;
    assert_eq!(backend.font_id(&live).unwrap_err(), ABSENT);
}

#[test]
fn silently_ignored_or_corrupt_load_rolls_back_entire_batch_without_retry() {
    for fail_at in [0, 1] {
        let requests = requests(2);
        let backend = ModelBackend::new(&requests, Fault::IgnoreAdd(fail_at));
        let trace = backend.trace.clone();
        assert!(matches!(
            failed(run(backend, requests)),
            OutlineBindingError::Native {
                phase: RegistrationPhase::Resolve,
                ..
            }
        ));
        let trace = trace.lock().unwrap();
        assert_eq!(trace.added_bytes.len(), fail_at + 1);
        assert_eq!(trace.apps_created, 0);
        assert_eq!(trace.backend_drops, 1);
        // Failed live keys are never retried, with the same or a modified style.
        assert_eq!(
            trace
                .descriptors
                .iter()
                .filter(|font| font.fallbacks.is_some())
                .count(),
            fail_at + 1
        );
    }
}

#[test]
fn native_errors_and_panics_at_every_phase_publish_nothing_and_drop_private_state() {
    for phase in [
        RegistrationPhase::Probe,
        RegistrationPhase::Register,
        RegistrationPhase::Resolve,
        RegistrationPhase::ConstructApplication,
        RegistrationPhase::VerifyApplication,
    ] {
        for panic in [false, true] {
            let requests = requests(2);
            let backend = ModelBackend::new(
                &requests,
                if panic {
                    Fault::Panic(phase)
                } else {
                    Fault::Error(phase)
                },
            );
            let trace = backend.trace.clone();
            let error = failed(run(backend, requests));
            if panic {
                assert_eq!(error, OutlineBindingError::NativePanicked);
            } else {
                assert!(
                    matches!(error, OutlineBindingError::Native { phase: at, .. } if at == phase)
                );
            }
            let trace = trace.lock().unwrap();
            assert_eq!(trace.backend_drops, 1);
            assert_eq!(trace.app_drops, trace.apps_created);
        }
    }
}

#[test]
fn app_resolution_mismatch_and_reused_native_id_abort_batch() {
    for (fault, expected) in [
        (Fault::WrongAppId, OutlineBindingError::ApplicationMismatch),
        (Fault::DuplicateId, OutlineBindingError::DuplicateFontId),
    ] {
        let requests = requests(2);
        let backend = ModelBackend::new(&requests, fault);
        let trace = backend.trace.clone();
        assert_eq!(failed(run(backend, requests)), expected);
        let trace = trace.lock().unwrap();
        assert_eq!(trace.app_drops, trace.apps_created);
        assert_eq!(trace.backend_drops, 1);
    }
}

#[test]
fn same_numerical_id_in_wrong_app_is_rejected_before_lookup() {
    let requests = requests(1);
    let backend = ModelBackend::new(&requests, Fault::None);
    let (_app, fonts) = run(backend, requests).unwrap();
    let own = owner();
    let other = owner();
    let id = fonts[0].id;
    let style = fonts[0].font.identity().style;
    let bindings = OutlineBindings {
        owner: own.clone(),
        fonts,
    };
    assert_eq!(
        failed(bindings.lookup(&other, id, style, 0)),
        OutlineBindingError::WrongTextSystem
    );
    assert_eq!(
        failed(bindings.lookup(&other, FontId(999), style, 0)),
        OutlineBindingError::WrongTextSystem
    );
    assert!(bindings.lookup(&own, id, style, 0).is_ok());
    assert_eq!(
        failed(bindings.lookup(&own, FontId(999), style, 0)),
        OutlineBindingError::UnboundFont
    );
    assert_eq!(
        failed(bindings.lookup(
            &own,
            id,
            StaticStyle {
                weight: style.weight + 1,
                ..style
            },
            0
        )),
        OutlineBindingError::StyleMismatch
    );
}

#[test]
fn unrelated_later_registration_preserves_append_only_binding_and_epoch() {
    let requests = requests(1);
    let backend = ModelBackend::new(&requests, Fault::None);
    let (_app, fonts) = run(backend, requests).unwrap();
    let own = owner();
    let bytes = fonts[0].font.bytes().clone();
    let identity = fonts[0].font.identity().clone();
    let bindings = OutlineBindings {
        owner: own.clone(),
        fonts,
    };
    // Noop models an unrelated public add_fonts call; exact native append-only
    // behavior remains a source-reviewed assumption until the native fixture.
    own.add_fonts(vec![std::borrow::Cow::Owned(vec![0; 8])])
        .unwrap();
    for epoch in [0, 1, 100] {
        let bound = bindings
            .lookup(&own, FontId(7), identity.style, epoch)
            .unwrap();
        assert!(Arc::ptr_eq(bound.bytes(), &bytes));
        assert_eq!(bound.identity(), &identity);
    }
    assert_eq!(bindings.descriptors().len(), 1);
}

#[cfg(not(all(
    target_os = "linux",
    any(feature = "static-outline-x11", feature = "static-outline-wayland")
)))]
#[test]
fn non_linux_or_no_explicit_backend_is_unsupported_without_native_construction() {
    assert_eq!(
        failed(application_with_static_outlines(requests(1))),
        OutlineBindingError::Unsupported
    );
}

#[test]
fn bound_bridge_checks_current_epoch_and_cancellation_before_outlines() {
    let requests = requests(1);
    let backend = ModelBackend::new(&requests, Fault::None);
    let (_app, fonts) = run(backend, requests).unwrap();
    let raster: Arc<dyn gpui::PlatformTextSystem> = Arc::new(gpui::NoopTextSystem::new());
    let system = Arc::new(TextSystem::new(raster.clone()));
    let id = fonts[0].id;
    let style = fonts[0].font.identity().style;
    let bindings = OutlineBindings {
        owner: system.clone(),
        fonts,
    };
    let bridge = GpuiTextRasterizer::from_outline_registration(system.clone(), raster, bindings);
    let cancel = std::sync::atomic::AtomicBool::new(false);
    assert!(
        bridge
            .static_outline_font(&system, id, style, 0, &cancel)
            .is_ok()
    );
    bridge.invalidate();
    assert_eq!(
        failed(bridge.static_outline_font(&system, id, style, 0, &cancel)),
        OutlineBindingError::StaleSnapshot
    );
    assert!(
        bridge
            .static_outline_font(&system, id, style, 1, &cancel)
            .is_ok()
    );
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(
        failed(bridge.static_outline_font(&system, id, style, 1, &cancel)),
        OutlineBindingError::StaleSnapshot
    );
    assert_eq!(bridge.static_outline_descriptors().unwrap().len(), 1);
}
