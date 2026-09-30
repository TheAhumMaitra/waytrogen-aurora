use log::{debug, error};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// Index of the colour matugen should extract from the image, `0` being the most
/// dominant one. Passing it is what stops matugen from showing its interactive
/// "Select the color you want to use as source color" prompt.
const SOURCE_COLOR_INDEX: &str = "0";

/// Highest index matugen accepts for `--source-color-index`.
#[cfg(test)]
const SOURCE_COLOR_INDEX_MAX: u8 = 4;

#[derive(Default)]
pub struct Matugen {
    enabled: bool,
    generated_images: HashSet<PathBuf>,
}

impl Matugen {
    #[must_use]
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            generated_images: HashSet::new(),
        }
    }

    /// Runs `matugen image <image> --source-color-index 0` unless matugen is
    /// disabled or the same image has already been generated with this instance.
    /// Returns whether matugen succeeded.
    pub fn generate(&mut self, image: &Path) -> bool {
        if !self.take_image(image) {
            return false;
        }
        debug!("Running matugen on {}", image.display());
        match matugen_command(image)
            // Never let matugen block waiting for the source colour picker.
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .status()
        {
            Ok(status) if status.success() => {
                debug!("matugen finished successfully");
                true
            }
            Ok(status) => {
                error!("matugen exited with {status}");
                false
            }
            Err(e) => {
                error!("Failed to run matugen, {e}");
                false
            }
        }
    }

    /// Registers `image` as handled and reports whether matugen still has to run
    /// for it. Empty paths come from wallpapers saved before the folder was known,
    /// so they are skipped without running matugen.
    fn take_image(&mut self, image: &Path) -> bool {
        if image.as_os_str().is_empty() {
            debug!("Skipping matugen for a wallpaper without a path");
            return false;
        }
        self.enabled && self.generated_images.insert(image.to_path_buf())
    }
}

/// Builds the matugen invocation used to theme from `image`. Kept separate from
/// the IO of [`Matugen::generate`] so tests can inspect and extend it.
fn matugen_command(image: &Path) -> Command {
    let mut command = Command::new("matugen");
    command
        .arg("image")
        .arg(image)
        .arg("--source-color-index")
        .arg(SOURCE_COLOR_INDEX);
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Creates a self cleaning temporary directory.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("waytrogen-{name}-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).expect("Failed to create temporary directory");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Writes a solid colour image matugen can read.
    fn write_test_image(path: &Path) {
        image::RgbImage::from_pixel(64, 64, image::Rgb([26, 188, 156]))
            .save(path)
            .expect("Failed to write test image");
    }

    fn args_of(command: &Command) -> Vec<String> {
        command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    fn matugen_is_installed() -> bool {
        Command::new("matugen")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
    }

    /// Runs matugen on a freshly written image and returns its parsed `--json hex`
    /// output, or `None` when matugen is not installed.
    ///
    /// `--dry-run` still renders templates, so the child's config directories are
    /// pointed at a temporary directory to keep the real theme files untouched.
    fn run_matugen(image: &Path, home: &Path) -> Option<serde_json::Value> {
        if !matugen_is_installed() {
            eprintln!("Skipping, matugen is not installed");
            return None;
        }
        let output = matugen_command(image)
            .args(["--dry-run", "--json", "hex"])
            .env("HOME", home)
            .env("XDG_CONFIG_HOME", home.join("config"))
            .env("XDG_CACHE_HOME", home.join("cache"))
            // A prompt would read stdin, and with none available matugen would
            // fail instead of exiting successfully.
            .stdin(Stdio::null())
            .output()
            .expect("Failed to run matugen");
        assert!(
            output.status.success(),
            "matugen exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        Some(serde_json::from_slice(&output.stdout).expect("matugen did not return JSON"))
    }

    /// Pulls a colour out of matugen's JSON, which is either a plain hex string or
    /// an object holding a `dark`, `default` and `light` variant.
    fn color_of(json: &serde_json::Value, name: &str) -> Option<String> {
        let color = json.get("colors")?.get(name)?;
        match color {
            serde_json::Value::String(hex) => Some(hex.clone()),
            serde_json::Value::Object(variants) => variants
                .get("dark")
                .or_else(|| variants.get("default"))
                .and_then(|variant| variant.get("color"))
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned),
            _ => None,
        }
    }

    #[test]
    fn command_picks_the_dominant_colour_without_prompting() {
        let args = args_of(&matugen_command(Path::new("/tmp/wall.png")));
        assert_eq!(
            args,
            vec!["image", "/tmp/wall.png", "--source-color-index", "0"]
        );
    }

    #[test]
    fn command_always_passes_a_source_colour_index() {
        let index: u8 = SOURCE_COLOR_INDEX
            .parse()
            .expect("Source colour index should be a number");
        assert!(index <= SOURCE_COLOR_INDEX_MAX);
        assert!(matugen_command(Path::new("wall.png"))
            .get_args()
            .collect::<Vec<_>>()
            .windows(2)
            .any(|pair| pair[0] == "--source-color-index" && pair[1] == SOURCE_COLOR_INDEX));
    }

    #[test]
    fn command_keeps_the_image_as_its_own_argument() {
        let args = args_of(&matugen_command(Path::new("/tmp/a folder/wall.png")));
        assert_eq!(args[0], "image");
        assert_eq!(args[1], "/tmp/a folder/wall.png");
    }

    #[test]
    fn a_disabled_matugen_never_takes_an_image() {
        let mut matugen = Matugen::new(false);
        assert!(!matugen.take_image(Path::new("/tmp/wall.png")));
        assert!(!matugen.take_image(Path::new("/tmp/wall.png")));
    }

    #[test]
    fn an_image_is_only_taken_once() {
        let mut matugen = Matugen::new(true);
        assert!(matugen.take_image(Path::new("/tmp/wall.png")));
        assert!(!matugen.take_image(Path::new("/tmp/wall.png")));
        assert!(matugen.take_image(Path::new("/tmp/other.png")));
        assert!(!matugen.take_image(Path::new("/tmp/other.png")));
    }

    #[test]
    fn generate_does_nothing_when_disabled() {
        assert!(!Matugen::new(false).generate(Path::new("/tmp/wall.png")));
    }

    #[test]
    fn generate_fails_loudly_on_an_unreadable_image() {
        let dir = TempDir::new("matugen-missing");
        let missing = dir.path().join("does-not-exist.png");
        assert!(!Matugen::new(true).generate(&missing));
    }

    #[test]
    fn matugen_generates_colors_without_user_input() {
        let dir = TempDir::new("matugen-colors");
        let image = dir.path().join("wall.png");
        write_test_image(&image);
        let Some(json) = run_matugen(&image, dir.path()) else {
            return;
        };
        assert_eq!(
            color_of(&json, "source_color").as_deref(),
            Some("#1abc9c"),
            "matugen did not extract the dominant colour"
        );
        for name in ["primary", "secondary", "tertiary", "background", "surface"] {
            assert!(
                color_of(&json, name).is_some(),
                "matugen did not generate {name}"
            );
        }
    }

    #[test]
    fn matugen_leaves_the_users_config_alone() {
        let dir = TempDir::new("matugen-isolation");
        let image = dir.path().join("wall.png");
        write_test_image(&image);
        if run_matugen(&image, dir.path()).is_none() {
            return;
        }
        assert!(
            !dir.path().join("config/matugen/config.toml").exists(),
            "matugen created a config inside the temporary directory, so it is not isolated"
        );
    }
}
