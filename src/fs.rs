use crate::{common::sort_by_sort_dropdown_string, wallpaper_changers::WallpaperChangers};
use std::path::{Path, PathBuf};

/// Every wallpaper inside `paths`, recursively, deduplicated across the folders
/// and sorted as a single list. Folders that do not exist are skipped.
#[must_use]
pub fn get_image_files(
    paths: &[PathBuf],
    sort_dropdown: &str,
    invert_sort_switch_state: bool,
) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = paths
        .iter()
        .flat_map(|path| {
            walkdir::WalkDir::new(path)
                .follow_links(true)
                .follow_root_links(true)
                .into_iter()
        })
        .filter_map(std::result::Result::ok)
        .filter(|f| f.file_type().is_file())
        .map(|d| d.path().to_path_buf())
        .filter(|p| {
            WallpaperChangers::all_accepted_formats().iter().any(|f| {
                f == p
                    .extension()
                    .unwrap_or_default()
                    .to_str()
                    .unwrap_or_default()
            })
        })
        .collect();
    // Mixtures can contain folders inside folders, so the same wallpaper may be
    // reached more than once.
    files.sort();
    files.dedup();
    sort_by_sort_dropdown_string(&mut files, sort_dropdown, invert_sort_switch_state);
    files
}

/// The themes folder wallust themes live in, `~/.config/themes`.
#[must_use]
pub fn themes_dir() -> PathBuf {
    config_dir().join("themes")
}

/// The user's pictures folder, read from `user-dirs.dirs`.
#[must_use]
pub fn pictures_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    pictures_dir_in(&PathBuf::from(&home), &config_dir())
}

/// The folders the `mixture` subcommand mixes together: every subfolder of every
/// theme plus `Pictures/Wallpapers`. Folders that do not exist are left out.
#[must_use]
pub fn mixture_folders() -> Vec<PathBuf> {
    mixture_folders_in(&themes_dir(), &pictures_dir())
}

/// The folders the `mixture` subcommand mixes together, given the themes folder
/// and the pictures folder.
fn mixture_folders_in(themes: &Path, pictures: &Path) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = subfolders(themes)
        .iter()
        .flat_map(|theme| subfolders(theme))
        .collect();

    let wallpapers = pictures.join("Wallpapers");
    if wallpapers.is_dir() {
        folders.push(wallpapers);
    }
    folders
}

/// Resolves `XDG_PICTURES_DIR` from `user-dirs.dirs`, which keeps `$HOME`
/// unexpanded, and falls back to `HOME/Pictures` when it is unset or unreadable.
fn pictures_dir_in(home: &Path, config: &Path) -> PathBuf {
    let fallback = || home.join("Pictures");
    let Ok(dirs) = std::fs::read_to_string(config.join("user-dirs.dirs")) else {
        return fallback();
    };
    dirs.lines()
        .find_map(|line| line.trim().strip_prefix("XDG_PICTURES_DIR="))
        .map(|value| {
            value
                .trim()
                .trim_matches('"')
                .replace("$HOME", &home.to_string_lossy())
        })
        .filter(|value| !value.is_empty())
        .map_or_else(fallback, PathBuf::from)
}

fn config_dir() -> PathBuf {
    xdg::BaseDirectories::with_prefix("").map_or_else(
        |_| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config"),
        |dirs| dirs.get_config_home(),
    )
}

/// Immediate subdirectories of `path`, sorted by name. Hidden folders such as
/// `.git` are skipped.
fn subfolders(path: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(path) else {
        return Vec::new();
    };
    let mut folders: Vec<PathBuf> = entries
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|entry| {
            entry.is_dir()
                && entry
                    .file_name()
                    .is_some_and(|name| !name.to_string_lossy().starts_with('.'))
        })
        .collect();
    folders.sort();
    folders
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_common::SORT_DROPDOWN_STRINGS;

    /// A temporary directory that removes itself again.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("waytrogen-{name}-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).expect("Failed to create temporary directory");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        /// Creates a directory inside of it and returns its path.
        fn dir(&self, name: &str) -> PathBuf {
            let path = self.0.join(name);
            std::fs::create_dir_all(&path).expect("Failed to create directory");
            path
        }

        fn file(&self, name: &str) -> PathBuf {
            let path = self.0.join(name);
            std::fs::write(&path, b"wallpaper").expect("Failed to write file");
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn image_files_are_collected_from_every_folder() {
        let dir = TempDir::new("fs-multi-root");
        let first = dir.dir("first");
        let second = dir.dir("second");
        dir.dir("second/nested");
        let a = dir.file("first/a.png");
        let b = dir.file("second/nested/b.jpg");
        // mpvpaper can play videos, but text files are not wallpapers.
        dir.file("second/notes.rst");

        let files = get_image_files(
            &[first, second, dir.path().join("missing")],
            SORT_DROPDOWN_STRINGS[1],
            false,
        );

        assert_eq!(files.len(), 2, "{files:?}");
        assert!(files.contains(&a) && files.contains(&b), "{files:?}");
    }

    #[test]
    fn image_files_reached_twice_are_listed_once() {
        let dir = TempDir::new("fs-dedupe");
        let theme = dir.dir("themes/Dracula/backgrounds");
        let root = dir.dir("themes");
        let wallpaper = dir.file("themes/Dracula/backgrounds/wall.png");

        let files = get_image_files(&[theme, root], SORT_DROPDOWN_STRINGS[1], false);

        assert_eq!(files, vec![wallpaper]);
    }

    #[test]
    fn mixture_uses_every_theme_subfolder_and_the_wallpapers_folder() {
        let dir = TempDir::new("fs-mixture");
        let themes = dir.dir("themes");
        let dracula = themes.join("Dracula");
        let catppuccin = themes.join("Catppuccin");
        std::fs::create_dir_all(dracula.join("backgrounds/nested")).expect("Failed to create dir");
        std::fs::create_dir_all(dracula.join("waybar")).expect("Failed to create dir");
        std::fs::create_dir_all(catppuccin.join("rofi")).expect("Failed to create dir");
        let pictures = dir.dir("Pictures/Wallpapers");

        let folders = mixture_folders_in(&themes, &dir.path().join("Pictures"));

        assert_eq!(
            folders,
            vec![
                catppuccin.join("rofi"),
                dracula.join("backgrounds"),
                dracula.join("waybar"),
                pictures,
            ]
        );
    }

    #[test]
    fn mixture_skips_hidden_theme_folders() {
        let dir = TempDir::new("fs-mixture-hidden");
        let themes = dir.dir("themes/Dracula");
        let _backgrounds = dir.dir("themes/Dracula/backgrounds");
        dir.dir("themes/Dracula/.git");

        let folders = mixture_folders_in(&dir.path().join("themes"), &dir.path().join("Pictures"));

        assert_eq!(folders, vec![themes.join("backgrounds")]);
    }

    #[test]
    fn mixture_skips_missing_folders() {
        let dir = TempDir::new("fs-mixture-missing");
        let folders = mixture_folders_in(&dir.path().join("themes"), &dir.path().join("Pictures"));
        assert!(folders.is_empty(), "{folders:?}");
    }

    #[test]
    fn pictures_dir_follows_user_dirs() {
        let dir = TempDir::new("fs-user-dirs");
        let config = dir.dir("config");
        std::fs::write(
            config.join("user-dirs.dirs"),
            "# generated\nXDG_DOWNLOAD_DIR=\"$HOME/Downloads\"\nXDG_PICTURES_DIR=\"$HOME/Bilder\"\n",
        )
        .expect("Failed to write user-dirs.dirs");

        assert_eq!(
            pictures_dir_in(dir.path(), &config),
            dir.path().join("Bilder")
        );
    }

    #[test]
    fn pictures_dir_falls_back_to_home() {
        let dir = TempDir::new("fs-user-dirs-missing");
        let config = dir.dir("config");

        assert_eq!(
            pictures_dir_in(dir.path(), &config),
            dir.path().join("Pictures")
        );
    }
}
