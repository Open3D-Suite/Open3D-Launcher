// SPDX-License-Identifier: GPL-3.0-or-later
//! The user's choices, saved between runs by the shell.

use std::collections::BTreeMap;
use std::path::Path;

use open3d_launcher_core::PackageKind;
use serde::{Deserialize, Serialize};

use crate::i18n::t;
use crate::open3d_settings;
use crate::theme::ThemeChoice;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViewMode {
    #[default]
    Grid,
    Table,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Filter {
    #[default]
    All,
    // These three no longer appear in the toolbar but must remain deserializable from old settings.
    Installed,
    Updates,
    NotInstalled,
    /// Apps the user chose to hide from the other lists.
    Hidden,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortKey {
    /// Suite order: classic apps, then new. Settings saved with the removed "Inspired by" sort load as this.
    #[default]
    #[serde(alias = "InspiredBy")]
    Suite,
    Name,
    Status,
    /// Installed version.
    Installed,
    /// Latest released version.
    Latest,
    /// Release date.
    Released,
    /// How often the app has been opened from here, most first.
    Frequent,
}

impl SortKey {
    /// The direction a first click sorts in: newest and highest first for dates and versions,
    /// A to Z for text.
    pub fn starts_descending(self) -> bool {
        matches!(self, Self::Installed | Self::Latest | Self::Released | Self::Frequent)
    }
}

/// How often to look for updates in the background.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckEvery {
    Hour,
    FourHours,
    Day,
    Never,
}

impl CheckEvery {
    /// The choices offered. `FourHours` and `Day` are kept so saved settings still load; they now
    /// mean "automatically" too.
    pub const ALL: [Self; 2] = [Self::Hour, Self::Never];

    pub fn label(self) -> String {
        match self {
            Self::Hour | Self::FourHours | Self::Day => t("Automatically").to_string(),
            Self::Never => t("Only when I ask").to_string(),
        }
    }

    /// How often to look. Checks ask Open3D's release feed (one cheap request; GitHub only as a
    /// fallback), so automatic checks run every twelve minutes to reduce GitHub API traffic.
    pub fn seconds(self) -> Option<i64> {
        match self {
            Self::Hour | Self::FourHours | Self::Day => Some(12 * 60),
            Self::Never => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub install_locations: open3d_launcher_core::protocol::InstallLocations,
    pub view: ViewMode,
    pub filter: Filter,
    pub sort: SortKey,
    pub sort_descending: bool,
    pub theme: ThemeChoice,
    /// Interface language: `auto` (follow Windows) or a language code from the registry (`en`,
    /// `es`). The language shared by every Open3D app is the source (`open3d_settings`): at start
    /// this takes its value ([`Self::take_shared_language`]), and a choice in Settings is written
    /// there, `auto` as no preference.
    pub language: String,
    /// The launcher's own `language` has been handed to the shared file (or there was nothing to
    /// hand over), so it is never carried over again.
    pub language_shared: bool,
    /// Closing the window keeps the launcher in the tray (menu bar) instead of quitting.
    pub close_to_tray: bool,
    /// macOS: showing the window brings it to the current desktop (Space) instead of switching to
    /// the desktop it was on.
    pub show_on_current_desktop: bool,
    pub check_every: CheckEvery,
    /// Automatically download and install updates when found.
    #[serde(default)]
    pub auto_update: bool,
    /// Update all apps together behind one permission prompt where the platform asks for one
    /// (Linux and Windows); off, each app's update asks on its own.
    #[serde(default)]
    pub batch_updates: bool,
    /// Show a system notification when an installed app has an update.
    pub notify: bool,
    pub prereleases: bool,
    /// Version track per app. Missing entries follow stable releases.
    pub versions: BTreeMap<String, open3d_launcher_core::protocol::VersionTrack>,
    pub launcher_version: open3d_launcher_core::protocol::VersionTrack,
    /// Linux: the package format to prefer for new installs (empty: the system's own).
    pub preferred_kinds: Vec<PackageKind>,
    /// Announcements and alerts from the release feed the user dismissed (by notice id).
    pub dismissed_notices: std::collections::BTreeSet<String>,
    /// Updates already announced, so each version is announced once: app id → version.
    pub notified: BTreeMap<String, String>,
    /// Unix seconds of the last finished check.
    pub last_check: Option<i64>,
    /// Hide community apps from the catalog without changing their installation or update settings.
    pub hide_community_apps: bool,
    /// Catalog ids of apps hidden from the main lists (`phototool`, …).
    pub hidden: std::collections::BTreeSet<String>,
    /// The one-time notice that closing the window keeps the launcher in the tray has been shown.
    pub tray_hint_shown: bool,
    /// How many times each app was opened from the launcher (catalog id → count), for "Frequently used".
    pub launches: BTreeMap<String, u32>,
    /// Catalog ids pinned to the top of the grid/table, in the order the user chose.
    pub pinned: Vec<String>,
    /// Extra command-line flags each app is opened with (catalog id → the text as typed, split
    /// by [`args::split`](open3d_launcher_core::args::split) at launch).
    pub launch_flags: BTreeMap<String, String>,
}

impl Settings {
    /// Machine-readable code for "follow the system language".
    pub const LANGUAGE_AUTO: &str = "auto";

    /// Clear saved catalog filters whose controls were removed from the toolbar.
    pub fn migrate(&mut self) {
        if !matches!(self.filter, Filter::All | Filter::Hidden) {
            self.filter = Filter::All;
        }
    }

    /// Whether `id` is in the pinned list.
    pub fn is_pinned(&self, id: &str) -> bool {
        self.pinned.iter().any(|p| p == id)
    }

    /// Pin `id` at the end of the pinned list (no-op if already pinned).
    pub fn pin_app(&mut self, id: &str) {
        if id.is_empty() || self.is_pinned(id) {
            return;
        }
        self.pinned.push(id.to_string());
    }

    /// Remove `id` from the pinned list.
    pub fn unpin_app(&mut self, id: &str) {
        self.pinned.retain(|p| p != id);
    }

    /// The index of the next pinned id above (`delta = -1`) or below (`delta = 1`) `id` that passes
    /// `available`. Skips stale catalog ids left in `pinned` so Move up/down still change the
    /// visible order.
    pub fn pinned_neighbor(&self, id: &str, delta: isize, available: impl Fn(&str) -> bool) -> Option<usize> {
        let mut j = self.pinned.iter().position(|p| p == id)?;
        loop {
            j = j.checked_add_signed(delta)?;
            if available(self.pinned.get(j)?) {
                return Some(j);
            }
        }
    }

    /// Move `from_id` to insertion slot `to_index` (`0..=len`, before remove). Used by drag-and-drop.
    pub fn reorder_pinned_to(&mut self, from_id: &str, mut to_index: usize) {
        let Some(from) = self.pinned.iter().position(|p| p == from_id) else {
            return;
        };
        to_index = to_index.min(self.pinned.len());
        // Already in this slot (including the no-op "insert immediately after self").
        if to_index == from || to_index == from + 1 {
            return;
        }
        let item = self.pinned.remove(from);
        if from < to_index {
            to_index -= 1;
        }
        self.pinned.insert(to_index, item);
    }

    /// Display name for a language `code`: the language's name in itself; "Auto" for `auto` and
    /// unknown codes, like [`Lang::from_pref`](crate::i18n::Lang::from_pref).
    pub fn language_label(code: &str) -> String {
        crate::i18n::Lang::from_code(code).map_or_else(|| t("Auto").to_string(), |lang| lang.name().to_string())
    }

    /// Takes the language shared by every Open3D app in at start, from `settings.json` in `dir`
    /// (D-051). Its preference becomes `language`. With none, a language the person chose here
    /// before is written there once (D-048), unless the write fails, and then it is tried again at
    /// the next start; otherwise `language` is `auto`, and Windows decides (D-052). Nothing is
    /// written when there is nothing to carry over.
    pub fn take_shared_language(&mut self, dir: Option<&Path>) {
        if let Some(shared) = dir.and_then(open3d_settings::read_language) {
            self.language = shared.code().to_string();
            self.language_shared = true;
            return;
        }
        if !self.language_shared
            && let Some(own) = open3d_settings::Language::from_code(&self.language)
        {
            match dir.map(|dir| open3d_settings::write_language(dir, Some(own))) {
                Some(Ok(())) => {
                    log::info!("the launcher's language, {}, is now the one every Open3D app shares", own.code());
                    self.language_shared = true;
                }
                Some(Err(e)) => log::warn!("couldn't carry the launcher's language over to the other Open3D apps: {e}"),
                None => log::warn!("no folder for the language shared by the Open3D apps"),
            }
            return;
        }
        self.language_shared = true;
        self.language = Self::LANGUAGE_AUTO.to_string();
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            install_locations: Default::default(),
            view: ViewMode::Grid,
            filter: Filter::All,
            sort: SortKey::Suite,
            sort_descending: false,
            theme: ThemeChoice::System,
            language: "auto".to_string(),
            language_shared: false,
            close_to_tray: true,
            show_on_current_desktop: true,
            check_every: CheckEvery::Hour,
            auto_update: false,
            batch_updates: false,
            notify: true,
            prereleases: false,
            versions: BTreeMap::new(),
            launcher_version: Default::default(),
            preferred_kinds: Vec::new(),
            dismissed_notices: std::collections::BTreeSet::new(),
            notified: BTreeMap::new(),
            last_check: None,
            hide_community_apps: false,
            hidden: std::collections::BTreeSet::new(),
            tray_hint_shown: false,
            launches: BTreeMap::new(),
            pinned: Vec::new(),
            launch_flags: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_appends_once_and_unpin_removes() {
        let mut s = Settings::default();
        s.pin_app("phototool");
        s.pin_app("phototool");
        s.pin_app("filmtool");
        assert_eq!(s.pinned, vec!["phototool", "filmtool"]);
        s.unpin_app("phototool");
        assert_eq!(s.pinned, vec!["filmtool"]);
        s.unpin_app("missing");
        assert_eq!(s.pinned, vec!["filmtool"]);
    }

    #[test]
    fn reorder_pinned_to_uses_insertion_slot() {
        let mut s = Settings { pinned: vec!["a".into(), "b".into(), "c".into()], ..Settings::default() };
        // Insert c before a (slot 0).
        s.reorder_pinned_to("c", 0);
        assert_eq!(s.pinned, vec!["c", "a", "b"]);
        // Insert c after b (slot 3).
        s.reorder_pinned_to("c", 3);
        assert_eq!(s.pinned, vec!["a", "b", "c"]);
        // Insert a before c (slot 2) → [b, a, c].
        s.reorder_pinned_to("a", 2);
        assert_eq!(s.pinned, vec!["b", "a", "c"]);
        // Insert a after itself (slot 2) is a no-op, and so is an unknown id; a slot past the end
        // means the end.
        s.reorder_pinned_to("a", 2);
        s.reorder_pinned_to("missing", 0);
        assert_eq!(s.pinned, vec!["b", "a", "c"]);
        s.reorder_pinned_to("b", 99);
        assert_eq!(s.pinned, vec!["a", "c", "b"]);
    }

    #[test]
    fn old_settings_json_loads_with_empty_pinned() {
        let s: Settings = serde_json::from_str(r#"{"view":"Grid"}"#).expect("legacy settings");
        assert!(s.pinned.is_empty());
    }

    #[test]
    fn pinned_neighbor_steps_within_bounds() {
        let s = Settings { pinned: vec!["a".into(), "b".into(), "c".into()], ..Settings::default() };
        let any = |_: &str| true;
        assert_eq!(s.pinned_neighbor("b", -1, any), Some(0));
        assert_eq!(s.pinned_neighbor("b", 1, any), Some(2));
        assert_eq!(s.pinned_neighbor("a", -1, any), None);
        assert_eq!(s.pinned_neighbor("c", 1, any), None);
        assert_eq!(s.pinned_neighbor("missing", 1, any), None);
    }

    /// A folder of its own for the shared `settings.json`, never the real `%APPDATA%\Open3D`.
    struct Shared(std::path::PathBuf);

    impl Shared {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("open3d-launcher-shared-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            Self(dir)
        }
        fn holding(name: &str, json: &str) -> Self {
            let shared = Self::new(name);
            std::fs::create_dir_all(&shared.0).unwrap();
            std::fs::write(shared.0.join(open3d_settings::FILE_NAME), json).unwrap();
            shared
        }
        fn file(&self) -> Option<String> {
            std::fs::read_to_string(self.0.join(open3d_settings::FILE_NAME)).ok()
        }
    }

    impl Drop for Shared {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn with_language(language: &str, language_shared: bool) -> Settings {
        Settings { language: language.into(), language_shared, ..Settings::default() }
    }

    #[test]
    fn the_shared_language_wins_over_the_launchers_own() {
        let shared = Shared::holding("wins", r#"{"language":"es","theme":"dark"}"#);
        for own in ["auto", "en", "es"] {
            let mut settings = with_language(own, false);
            settings.take_shared_language(Some(&shared.0));
            assert_eq!((settings.language.as_str(), settings.language_shared), ("es", true), "{own}");
        }
        assert_eq!(shared.file().as_deref(), Some(r#"{"language":"es","theme":"dark"}"#), "reading writes nothing");
    }

    #[test]
    fn a_language_chosen_here_is_carried_over_once() {
        let shared = Shared::new("carried");
        let mut settings = with_language("es", false);
        settings.take_shared_language(Some(&shared.0));
        assert_eq!((settings.language.as_str(), settings.language_shared), ("es", true));
        assert_eq!(open3d_settings::read_language(&shared.0), Some(open3d_settings::Language::Spanish));
        // Taken out later (Auto, here or in another app): the launcher's copy is not put back.
        assert_eq!(open3d_settings::write_language(&shared.0, None), Ok(()));
        settings.take_shared_language(Some(&shared.0));
        assert_eq!((settings.language.as_str(), settings.language_shared), ("auto", true));
        assert_eq!(open3d_settings::read_language(&shared.0), None);
    }

    #[test]
    fn auto_carries_nothing_over_and_writes_nothing() {
        let shared = Shared::new("auto");
        let mut settings = with_language("auto", false);
        settings.take_shared_language(Some(&shared.0));
        assert_eq!((settings.language.as_str(), settings.language_shared), ("auto", true));
        assert!(!shared.0.exists());
    }

    #[test]
    fn a_carry_over_that_fails_is_tried_again_at_the_next_start() {
        let shared = Shared::new("blocked");
        // The folder is a file: nothing can be written in it.
        std::fs::write(&shared.0, b"in the way").unwrap();
        let mut settings = with_language("en", false);
        settings.take_shared_language(Some(&shared.0));
        assert_eq!((settings.language.as_str(), settings.language_shared), ("en", false));
        std::fs::remove_file(&shared.0).unwrap();
        settings.take_shared_language(Some(&shared.0));
        assert_eq!((settings.language.as_str(), settings.language_shared), ("en", true));
        assert_eq!(open3d_settings::read_language(&shared.0), Some(open3d_settings::Language::English));
    }

    #[test]
    fn old_settings_have_not_handed_their_language_over() {
        let s: Settings = serde_json::from_str(r#"{"language":"es"}"#).expect("settings of an older launcher");
        assert_eq!((s.language.as_str(), s.language_shared), ("es", false));
    }

    #[test]
    fn pinned_neighbor_skips_unavailable_ids() {
        let s = Settings { pinned: vec!["a".into(), "gone".into(), "b".into()], ..Settings::default() };
        let live = |id: &str| id != "gone";
        assert_eq!(s.pinned_neighbor("a", 1, live), Some(2));
        assert_eq!(s.pinned_neighbor("b", -1, live), Some(0));
    }
}
