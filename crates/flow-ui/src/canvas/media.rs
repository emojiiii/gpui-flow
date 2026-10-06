//! Per-node media ownership. Imported paths are only opened when the host enables a backend.
use super::*;
use crate::media_backend::{MediaBackend, PlaybackOptions};
impl FlowCanvas {
    /// Enable an explicitly chosen backend. Switching it drops all old players.
    /// Hosts loading untrusted documents should validate allowed paths before enabling media.
    pub fn set_media_backend(&mut self, backend: Option<Arc<dyn MediaBackend>>) {
        self.media_players.clear();
        self.media_backend = backend;
    }
    pub(super) fn sync_media(&mut self, cx: &mut Context<Self>) {
        let sources: BTreeMap<_, _> = self
            .editor
            .graph()
            .nodes
            .values()
            .filter(|node| model::node_is_visible(self.editor.graph(), &node.id))
            .filter(|node| {
                matches!(node.kind.as_str(), "audio" | "video")
                    && self.registry.get(&node.kind).is_none()
            })
            .filter_map(|node| {
                node.data
                    .get("source")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|source| (node.id.clone(), source.to_owned()))
            })
            .collect();
        self.media_players
            .retain(|id, (source, _)| sources.get(id) == Some(source));
        let Some(backend) = self.media_backend.clone() else {
            self.media_players.clear();
            return;
        };
        let limit_message =
            "Canvas media player limit (8) reached; remove another source to activate";
        self.media_players
            .retain(|_, (_, player)| !matches!(player, Err(error) if error == limit_message));
        let mut active = self
            .media_players
            .values()
            .filter(|(_, player)| player.is_ok())
            .count();
        for (id, source) in sources {
            if self.media_players.contains_key(&id) {
                continue;
            }
            if active >= 8 {
                self.media_players
                    .insert(id, (source, Err(limit_message.into())));
                continue;
            }
            let player = backend
                .open(std::path::Path::new(&source), PlaybackOptions::default())
                .map(|controller| {
                    cx.new(|cx| {
                        MediaView::new(controller, cx)
                            .theme(self.theme)
                            .preview_height(90.)
                    })
                })
                .map_err(|error| error.to_string());
            if player.is_ok() {
                active += 1;
            }
            self.media_players.insert(id, (source, player));
        }
    }
}
