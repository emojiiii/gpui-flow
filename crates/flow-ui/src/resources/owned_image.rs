use super::*;
use gpui_flow_media::{
    DecodedImage, ImageLoad, ImageLoadLimits, ImageLoadState, LoadTicket, MediaError,
    OwnedImageLoader, ResourceLoadPool,
};
use std::{cell::RefCell, path::PathBuf, sync::Weak};

struct Shared {
    loader: OwnedImageLoader,
    rendered: RefCell<BTreeMap<LoadTicket, Weak<RenderImage>>>,
}
/// Clone one instance across SceneViews to share decode jobs and CPU images.
/// Uses an explicitly supplied local worker; it never falls back to an unowned
/// GPUI fetch when a source is unsupported, unauthorized or over budget.
#[derive(Clone)]
pub struct SceneOwnedImageLoader(Rc<Shared>);
impl SceneOwnedImageLoader {
    pub fn new(
        pool: ResourceLoadPool,
        worker: PathBuf,
        limits: ImageLoadLimits,
    ) -> Result<Self, MediaError> {
        Ok(Self(Rc::new(Shared {
            loader: OwnedImageLoader::new(pool, worker, limits)?,
            rendered: RefCell::new(BTreeMap::new()),
        })))
    }
    pub fn pool(&self) -> &ResourceLoadPool {
        self.0.loader.pool()
    }
    pub fn invalidate(&self) {
        self.0.loader.invalidate();
    }
    fn collect(&self) {
        self.0
            .rendered
            .borrow_mut()
            .retain(|_, image| image.strong_count() > 0);
    }
    fn image(
        &self,
        ticket: LoadTicket,
        decoded: Arc<DecodedImage>,
    ) -> Result<Arc<RenderImage>, MediaError> {
        if let Some(image) = self
            .0
            .rendered
            .borrow()
            .get(&ticket)
            .and_then(Weak::upgrade)
        {
            return Ok(image);
        }
        let mut frames = Vec::with_capacity(decoded.frames.len());
        for f in &decoded.frames {
            let mut bytes = f.rgba.clone();
            for pixel in bytes.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
            let buffer = image::RgbaImage::from_raw(f.width, f.height, bytes)
                .ok_or_else(|| MediaError::InvalidMedia("invalid owned image frame".into()))?;
            frames.push(image::Frame::from_parts(
                buffer,
                0,
                0,
                image::Delay::from_numer_denom_ms(f.delay_numerator_ms, f.delay_denominator),
            ));
        }
        let image = Arc::new(RenderImage::new(frames));
        self.0.loader.pool().retain_until(&image, decoded);
        self.0
            .rendered
            .borrow_mut()
            .insert(ticket, Arc::downgrade(&image));
        Ok(image)
    }
}
struct Entry {
    job: ImageLoad,
    _wake: gpui::Task<()>,
}
#[derive(Default)]
pub(super) struct OwnedImages {
    pub loader: Option<SceneOwnedImageLoader>,
    entries: BTreeMap<ResourceTicket, Entry>,
}
impl OwnedImages {
    pub fn event(&self, ticket: ResourceTicket) -> Option<gpui_flow_media::ResourceLoadEvent> {
        self.entries
            .get(&ticket)
            .map(|entry| entry.job.resource_event())
    }
    pub fn clear(&mut self) {
        self.entries.clear();
        if let Some(loader) = &self.loader {
            loader.collect();
        }
    }
    pub fn retain(&mut self, live: impl Fn(ResourceTicket) -> bool) {
        self.entries.retain(|ticket, _| live(*ticket));
        if let Some(loader) = &self.loader {
            loader.collect();
        }
    }
    pub fn prepare(
        &mut self,
        ticket: ResourceTicket,
        resource: &Resource,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<Option<Arc<RenderImage>>, MediaError> {
        let loader = self.loader.as_ref().expect("owned loading configured");
        loader.collect();
        if let std::collections::btree_map::Entry::Vacant(entry) = self.entries.entry(ticket) {
            let Resource::Path(path) = resource else {
                return Err(MediaError::InvalidSource(
                    "owned image loader accepts authorized local files only".into(),
                ));
            };
            let job = loader.0.loader.load(path)?;
            let future = job.completion();
            let wake = window.spawn(cx, async move |cx| {
                let _ = future.await;
                let _ = cx.update(|window, _| window.refresh());
            });
            entry.insert(Entry { job, _wake: wake });
        }
        let job = &self.entries.get(&ticket).expect("inserted image job").job;
        match job.state() {
            ImageLoadState::Pending { .. } => Ok(None),
            ImageLoadState::Failed(error) => Err(error),
            ImageLoadState::Loaded(image) => loader.image(job.ticket(), image).map(Some),
        }
    }
}
