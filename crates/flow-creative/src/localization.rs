use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum CreativeLanguage {
    SimplifiedChinese,
    English,
}

// The workbench runs in one process. Background tasks format messages in the
// same language as the desktop views and built-in node registry.
static ENGLISH: AtomicBool = AtomicBool::new(false);

pub(crate) fn language() -> CreativeLanguage {
    if ENGLISH.load(Ordering::Relaxed) {
        CreativeLanguage::English
    } else {
        CreativeLanguage::SimplifiedChinese
    }
}

pub(crate) fn tr(chinese: &'static str, english: &'static str) -> &'static str {
    match language() {
        CreativeLanguage::SimplifiedChinese => chinese,
        CreativeLanguage::English => english,
    }
}

#[cfg(feature = "desktop")]
mod desktop {
    use super::{CreativeLanguage, ENGLISH, Ordering};
    use gpui::{App, Global};
    use std::{
        fs::{self, File, OpenOptions},
        io::{self, Read, Write},
        path::{Path, PathBuf},
    };

    pub(crate) struct LanguagePreference(Option<PathBuf>);
    impl Global for LanguagePreference {}

    impl CreativeLanguage {
        pub(crate) fn label(self) -> &'static str {
            match self {
                Self::SimplifiedChinese => "简体中文",
                Self::English => "English",
            }
        }

        fn code(self) -> &'static str {
            match self {
                Self::SimplifiedChinese => "zh-CN",
                Self::English => "en",
            }
        }
    }

    /// Load the desktop app's language before creating its window.
    pub fn initialize_creative_language(cx: &mut App) {
        let path = match crate::ProjectStore::default_root() {
            Ok(root) => Some(root.with_file_name("language.txt")),
            Err(error) => {
                eprintln!("Cannot locate Flow Creative language preference: {error}");
                None
            }
        };
        let selected = match path.as_deref().map(load).transpose() {
            Ok(Some(selected)) => selected,
            Ok(None) => CreativeLanguage::SimplifiedChinese,
            Err(error) => {
                eprintln!("Cannot read Flow Creative language preference: {error}");
                CreativeLanguage::SimplifiedChinese
            }
        };
        activate(selected);
        cx.set_global(LanguagePreference(path));
    }

    fn load(path: &Path) -> io::Result<CreativeLanguage> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(CreativeLanguage::SimplifiedChinese);
            }
            Err(error) => return Err(error),
        };
        let mut value = String::new();
        file.take(32).read_to_string(&mut value)?;
        match value.trim() {
            "zh-CN" => Ok(CreativeLanguage::SimplifiedChinese),
            "en" => Ok(CreativeLanguage::English),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "expected en or zh-CN in language.txt",
            )),
        }
    }

    pub(crate) fn select_language(selected: CreativeLanguage, cx: &App) -> io::Result<()> {
        if let Some(path) = &cx.global::<LanguagePreference>().0 {
            let directory = path.parent().expect("language preference has a parent");
            fs::create_dir_all(directory)?;
            let temporary =
                directory.join(format!(".{}.tmp", crate::project::unique_name("language")));
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            let result = (|| {
                file.write_all(selected.code().as_bytes())?;
                file.sync_all()?;
                drop(file);
                fs::rename(&temporary, path)
            })();
            if let Err(error) = result {
                let _ = fs::remove_file(temporary);
                return Err(error);
            }
        }
        activate(selected);
        Ok(())
    }

    fn activate(selected: CreativeLanguage) {
        ENGLISH.store(selected == CreativeLanguage::English, Ordering::Relaxed);
        gpui_kit::component::set_locale(selected.code());
    }
}

#[cfg(feature = "desktop")]
pub use desktop::initialize_creative_language;
#[cfg(feature = "desktop")]
pub(crate) use desktop::{LanguagePreference, select_language};
