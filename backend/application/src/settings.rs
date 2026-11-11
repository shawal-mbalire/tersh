//! The settings use case: what the UI shows of the user's configuration, and
//! noticing when anything it depends on changes (config, themes, stylesheet,
//! plugins, installed fonts) so a running terminal can reload in place.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use serde::Serialize;
use tersh_domain::Config;

use crate::ports::{ConfigStore, Fingerprint, Watchable};

/// Everything the UI gets from the user's settings.
#[derive(Debug, Clone, Serialize)]
pub struct Settings {
    pub config: Config,
    /// The user stylesheet.
    pub style: Option<String>,
    pub config_path: PathBuf,
    /// Theme names usable in `theme = "..."`.
    pub themes: Vec<String>,
    /// Changes when any plugin file changes, so the UI knows to reload plugins.
    pub plugins_revision: String,
    /// Changes when fonts are installed or removed, so the UI re-measures the grid.
    pub fonts_revision: String,
}

impl Settings {
    pub fn new(store: &dyn ConfigStore, fonts: &dyn Watchable, config: Config) -> Self {
        Self {
            config,
            style: store.style(),
            config_path: store.config_path(),
            themes: store.themes(),
            plugins_revision: revision(&store.plugin_files()),
            fonts_revision: revision(&fonts.fingerprint()),
        }
    }
}

fn revision(fingerprint: &Fingerprint) -> String {
    let mut hasher = DefaultHasher::new();
    fingerprint.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Detects changes to the settings, plugin and font files between polls.
pub struct ChangeDetector {
    last: Fingerprint,
}

impl ChangeDetector {
    pub fn new(store: &dyn ConfigStore, fonts: &dyn Watchable) -> Self {
        Self { last: Self::snapshot(store, fonts) }
    }

    fn snapshot(store: &dyn ConfigStore, fonts: &dyn Watchable) -> Fingerprint {
        let mut all = store.settings_files();
        all.extend(store.plugin_files());
        all.extend(fonts.fingerprint());
        all
    }

    /// Whether anything changed since the previous call.
    pub fn changed(&mut self, store: &dyn ConfigStore, fonts: &dyn Watchable) -> bool {
        let current = Self::snapshot(store, fonts);
        if current == self.last {
            return false;
        }
        self.last = current;
        true
    }
}
