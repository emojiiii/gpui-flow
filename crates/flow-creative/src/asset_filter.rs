use crate::{MASK_MEDIA_TYPE, ProjectAsset};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AssetFilter {
    #[default]
    All,
    Image,
    Video,
    Audio,
    Model,
    Mask,
}

impl AssetFilter {
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "全部",
            Self::Image => "图片",
            Self::Video => "视频",
            Self::Audio => "音频",
            Self::Model => "模型",
            Self::Mask => "Mask",
        }
    }

    /// Match the media kind and a trimmed, Unicode-lowercase name substring.
    /// The asset key and other manifest metadata are never searched or opened.
    pub fn matches(self, asset: &ProjectAsset, query: &str) -> bool {
        let kind_matches = if asset.media_type == MASK_MEDIA_TYPE {
            matches!(self, Self::All | Self::Mask)
        } else {
            match self {
                Self::All => true,
                Self::Image => asset.media_type.starts_with("image/"),
                Self::Video => asset.media_type.starts_with("video/"),
                Self::Audio => asset.media_type.starts_with("audio/"),
                Self::Model => asset.media_type == "model/gltf-binary",
                Self::Mask => false,
            }
        };
        if !kind_matches {
            return false;
        }
        let query = query.trim();
        query.is_empty() || asset.name.to_lowercase().contains(&query.to_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str, media_type: &str) -> ProjectAsset {
        ProjectAsset {
            key: "assets/a-012345.png".into(),
            name: name.into(),
            media_type: media_type.into(),
            byte_len: 42,
            width: 32,
            height: 24,
        }
    }

    #[test]
    fn unicode_name_query_is_trimmed_case_insensitive_and_combined_with_type() {
        let image = asset("ÉTÉ 森林 ÖLÇEK.png", "image/png");
        assert!(AssetFilter::Image.matches(&image, "\u{2003}été 森林 ölçek\n"));
        assert!(!AssetFilter::Video.matches(&image, "été"));
        assert!(!AssetFilter::Image.matches(&image, "冬天"));
        assert!(AssetFilter::Image.matches(&image, "\t\u{2003}"));
        assert_eq!(AssetFilter::default(), AssetFilter::All);
    }

    #[test]
    fn masks_and_ordinary_png_images_are_distinct() {
        let mask = asset("Selection.png", MASK_MEDIA_TYPE);
        let image = asset("Selection.png", "image/png");
        assert!(AssetFilter::All.matches(&mask, "selection"));
        assert!(AssetFilter::Mask.matches(&mask, "selection"));
        assert!(!AssetFilter::Image.matches(&mask, "selection"));
        assert!(AssetFilter::Image.matches(&image, "selection"));
        assert!(!AssetFilter::Mask.matches(&image, "selection"));
        assert!(!AssetFilter::Mask.matches(&mask, "other"));
    }

    #[test]
    fn media_types_are_classified_without_filename_or_container_guessing() {
        let filters = [
            AssetFilter::Image,
            AssetFilter::Video,
            AssetFilter::Audio,
            AssetFilter::Model,
            AssetFilter::Mask,
        ];
        for (media_type, expected) in [
            ("audio/mp4", Some(AssetFilter::Audio)),
            ("video/mp4", Some(AssetFilter::Video)),
            ("model/gltf-binary", Some(AssetFilter::Model)),
            ("model/gltf+json", None),
            ("application/octet-stream", None),
            ("", None),
        ] {
            let media = asset("misleading-image.png", media_type);
            assert!(AssetFilter::All.matches(&media, ""));
            for filter in filters {
                assert_eq!(
                    filter.matches(&media, ""),
                    Some(filter) == expected,
                    "{filter:?} must classify {media_type:?} by its declared MIME type"
                );
            }
        }
    }

    #[test]
    fn only_display_name_is_searched_and_query_has_no_path_semantics() {
        let mut image = asset("Visible landscape", "image/png");
        image.key = "../../private/hidden-secret.png".into();
        for query in ["hidden-secret", "../../private", "image/png", "42", "*"] {
            assert!(!AssetFilter::All.matches(&image, query));
        }
        assert!(AssetFilter::Image.matches(&image, "landscape"));
        image.name = "../Literal * Name.png".into();
        assert!(AssetFilter::Image.matches(&image, "../literal *"));
    }
}
