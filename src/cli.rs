use crate::{
    common::{
        parse_executable_script, sort_by_sort_dropdown_string, Wallpaper, APP_ID, APP_VERSION,
        CACHE_FILE_NAME, CONFIG_APP_NAME, GETTEXT_DOMAIN,
    },
    main_window::{build_ui, stored_mixture_folders},
    matugen::{theme_latest_in_background, Matugen},
    ui_common::{gschema_string_to_string, string_to_gschema_string, SORT_DROPDOWN_STRINGS},
    wallpaper_changers::{WallpaperChanger, WallpaperChangers},
};
use clap::{Parser, Subcommand};
use gettextrs::{bind_textdomain_codeset, bindtextdomain, getters, gettext, textdomain};
use gtk::{gio::Settings, glib, prelude::*, Application};
use log::debug;
use rand::Rng;
use std::{
    env::current_exe,
    fs::{remove_dir_all, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use log::{error, warn};

#[must_use]
pub fn restore_wallpapers(args: &Cli) -> glib::ExitCode {
    let settings = Settings::new(APP_ID);
    WallpaperChangers::killall_changers();
    let previous_wallpapers = serde_json::from_str::<Vec<Wallpaper>>(&gschema_string_to_string(
        settings.string("saved-wallpapers").as_ref(),
    ))
    .unwrap();
    let matugen = Matugen::new(args.matugen);
    let mut themed: Vec<PathBuf> = Vec::new();
    for wallpaper in previous_wallpapers {
        debug!("Restoring: {:?}", wallpaper);
        if matugen.take_image(Path::new(&wallpaper.clone().path)) {
            themed.push(PathBuf::from(wallpaper.clone().path));
        }
        wallpaper.clone().changer.change(
            PathBuf::from(wallpaper.clone().path),
            wallpaper.clone().monitor,
        );
        match wallpaper.clone().changer {
            WallpaperChangers::Hyprpaper(_) => {
                thread::sleep(Duration::from_millis(1000));
            }
            WallpaperChangers::Swaybg(_, _)
            | WallpaperChangers::MpvPaper(_, _, _)
            | WallpaperChangers::Awww(_, _, _, _, _, _, _, _, _, _, _)
            | WallpaperChangers::GSlapper(_, _, _, _) => {}
        }
    }
    theme_latest_in_background(&themed);
    glib::ExitCode::SUCCESS
}

#[must_use]
pub fn print_wallpaper_state() -> glib::ExitCode {
    let settings = Settings::new(APP_ID);
    println!(
        "{}",
        gschema_string_to_string(&settings.string("saved-wallpapers"))
    );
    glib::ExitCode::SUCCESS
}

fn get_previous_wallpapers(settings: &Settings) -> Vec<Wallpaper> {
    let previous_wallpapers = serde_json::from_str::<Vec<Wallpaper>>(&gschema_string_to_string(
        settings.string("saved-wallpapers").as_ref(),
    ))
    .unwrap();
    previous_wallpapers
}

fn get_previous_supported_wallpapers(settings: &Settings) -> Vec<PathBuf> {
    let previous_wallpapers = get_previous_wallpapers(settings);
    let mixture = stored_mixture_folders(settings);
    let paths = if mixture.is_empty() {
        vec![Path::new(&previous_wallpapers[0].clone().path)
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .to_path_buf()]
    } else {
        mixture
    };
    paths
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
            previous_wallpapers
                .iter()
                .map(|w| w.changer.clone())
                .all(|c| {
                    c.accepted_formats().iter().any(|f| {
                        f == p
                            .extension()
                            .unwrap_or_default()
                            .to_str()
                            .unwrap_or_default()
                    })
                })
        })
        .collect::<Vec<_>>()
}

#[must_use]
pub fn set_random_wallpapers(args: &Cli) -> glib::ExitCode {
    let settings = Settings::new(APP_ID);
    let mut previous_wallpapers = get_previous_wallpapers(&settings);
    let files = get_previous_supported_wallpapers(&settings);
    WallpaperChangers::killall_changers();
    let matugen = Matugen::new(args.matugen);
    let mut themed: Vec<PathBuf> = Vec::new();
    for w in &mut previous_wallpapers {
        let mut rng = rand::thread_rng();
        let index = rng.gen_range(0..files.len());
        log::debug!("{index}");
        if matugen.take_image(&files[index]) {
            themed.push(files[index].clone());
        }
        w.changer
            .clone()
            .change(files[index].clone(), w.monitor.clone());
        w.path = files[index].clone().to_str().unwrap_or_default().to_owned();
    }
    match settings.set_string(
        "saved-wallpapers",
        &string_to_gschema_string(&serde_json::to_string(&previous_wallpapers).unwrap_or_default()),
    ) {
        Ok(_) => {}
        Err(e) => {
            error!("{} {e}", gettext("Unable to save \"next\" wallpapers"));
        }
    }
    theme_latest_in_background(&themed);
    glib::ExitCode::SUCCESS
}

#[must_use]
pub fn print_app_version() -> glib::ExitCode {
    println!("{APP_VERSION}");
    glib::ExitCode::SUCCESS
}

#[must_use]
pub fn cycle_next_wallpaper(args: &Cli) -> glib::ExitCode {
    let settings = Settings::new(APP_ID);
    let mut previous_wallpapers = get_previous_wallpapers(&settings);
    let sort_dropdown_string = SORT_DROPDOWN_STRINGS[settings.uint("sort-by") as usize];
    let mut files = get_previous_supported_wallpapers(&settings);
    let invert_sort_state = settings.boolean("invert-sort");
    sort_by_sort_dropdown_string(&mut files, sort_dropdown_string, invert_sort_state);
    let matugen = Matugen::new(args.matugen);
    let mut themed: Vec<PathBuf> = Vec::new();
    if args.next.clone().unwrap_or_default() == "All" {
        for previous_wallpaper in &mut previous_wallpapers {
            let wallpaper_index = files.iter().position(|p| {
                p.clone()
                    == previous_wallpaper
                        .path
                        .parse::<PathBuf>()
                        .unwrap_or_default()
            });
            try_set_next_wallpaper(
                &files,
                wallpaper_index,
                previous_wallpaper,
                &matugen,
                &mut themed,
            );
        }
    } else {
        let previous_wallpaper = previous_wallpapers
            .iter()
            .find(|w| *w.monitor == args.next.clone().unwrap_or_default());
        if previous_wallpaper.is_none() {
            error!(
                "Display \"{}\" does not exist.",
                args.next.clone().unwrap_or_default()
            );
            return glib::ExitCode::FAILURE;
        }
        let mut previous_wallpaper = previous_wallpaper.unwrap().clone();
        try_set_next_wallpaper(
            &files,
            files.iter().position(|f| {
                *f == previous_wallpaper
                    .path
                    .parse::<PathBuf>()
                    .unwrap_or_default()
            }),
            &mut previous_wallpaper,
            &matugen,
            &mut themed,
        );
        let index = previous_wallpapers
            .iter()
            .position(|w| w.monitor == previous_wallpaper.monitor)
            .unwrap();
        previous_wallpapers[index] = previous_wallpaper;
    }
    match settings.set_string(
        "saved-wallpapers",
        &string_to_gschema_string(&serde_json::to_string(&previous_wallpapers).unwrap_or_default()),
    ) {
        Ok(_) => {}
        Err(e) => {
            error!("{} {e}", gettext("Unable to save \"next\" wallpapers"));
        }
    }
    theme_latest_in_background(&themed);
    glib::ExitCode::SUCCESS
}

fn try_set_next_wallpaper(
    files: &[PathBuf],
    position: Option<usize>,
    previous_wallpaper: &mut Wallpaper,
    matugen: &Matugen,
    themed: &mut Vec<PathBuf>,
) {
    if let Some(i) = position {
        let path = &files[(i + 1) % files.len()];
        if matugen.take_image(path) {
            themed.push(path.clone());
        }
        previous_wallpaper
            .changer
            .clone()
            .change(path.clone(), previous_wallpaper.monitor.clone());
        previous_wallpaper.path = path.to_str().unwrap_or_default().to_owned();
    } else {
        warn!(
            "Wallpaper {} could not be found. Using first wallpaper",
            previous_wallpaper
                .path
                .parse::<PathBuf>()
                .unwrap_or_default()
                .display()
        );
        match files.first() {
            Some(p) => {
                if matugen.take_image(p) {
                    themed.push(p.clone());
                }
                previous_wallpaper
                    .changer
                    .clone()
                    .change(p.clone(), previous_wallpaper.monitor.clone());
                previous_wallpaper.path = p.to_str().unwrap_or_default().to_owned();
            }
            None => {
                error!("Wallpaper directory is empty. Please set a wallpaper folder before using --next.");
            }
        }
    }
}

pub fn delete_image_cache() -> glib::ExitCode {
    let xdg_dirs = xdg::BaseDirectories::with_prefix(CONFIG_APP_NAME);
    if xdg_dirs.is_err() {
        error!(
            "Failed to get XDG base dirrectory, {}",
            xdg_dirs.err().unwrap()
        );
        return glib::ExitCode::FAILURE;
    }
    let xdg_dirs = xdg_dirs.unwrap();
    let cache_path = xdg_dirs.place_cache_file(CACHE_FILE_NAME);
    if cache_path.is_err() {
        error!("Failed to get cache path, {}", cache_path.err().unwrap());
        return glib::ExitCode::FAILURE;
    }

    match remove_dir_all(xdg_dirs.get_cache_home()) {
        Ok(_) => glib::ExitCode::SUCCESS,
        Err(e) => {
            error!("Failed to delete cache {e}");
            glib::ExitCode::FAILURE
        }
    }
}

/// Resolves the folder given to `waytrogen open <PATH>` into an absolute path.
/// `Ok(None)` means no folder was requested, `Err` explains why it is unusable.
pub fn resolve_open_folder(args: &Cli) -> Result<Option<PathBuf>, String> {
    let Some(Command::Open { path }) = args.command.clone() else {
        return Ok(None);
    };
    match path.canonicalize() {
        Ok(folder) if folder.is_dir() => {
            debug!("Opening wallpaper folder {}", folder.display());
            Ok(Some(folder))
        }
        Ok(folder) => Err(format!("{} is not a directory", folder.display())),
        Err(e) => Err(format!("Failed to open {}: {e}", path.display())),
    }
}

#[must_use]
pub fn launch_application(args: Cli) -> glib::ExitCode {
    let app = Application::builder().application_id(APP_ID).build();
    textdomain("waytrogen").unwrap();
    bind_textdomain_codeset("waytrogen", "UTF-8").unwrap();
    let os_id = get_os_id().unwrap().unwrap_or_default();
    let domain_directory = match os_id.as_str() {
        "nixos" => {
            #[cfg(feature = "nixos")]
            // the path is known at compile time when using nix to build waytrogen
            {
                let path = env!("OUT_PATH").parse::<PathBuf>().unwrap();
                path.join("share").join("locale")
            }

            #[cfg(not(feature = "nixos"))]
            {
                let exe_path = current_exe().unwrap();
                exe_path
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .join("share")
                    .join("locale")
            }
        }
        _ => getters::domain_directory(GETTEXT_DOMAIN).unwrap(),
    };
    bindtextdomain(GETTEXT_DOMAIN, domain_directory).unwrap();

    app.connect_activate(move |app| {
        build_ui(app, &args);
    });

    let empty: Vec<String> = vec![];
    // Run the application
    app.run_with_args(&empty)
}

/// os id is the ID="nixos" parameter in `/etc/os-release`
/// If ID parameter is not found this returns None
fn get_os_id() -> anyhow::Result<Option<String>> {
    let file = File::open("/etc/os-release")?;
    let reader = BufReader::new(file);

    for line in reader.lines() {
        let line = line?;
        if let Some(s) = line.strip_prefix("ID=") {
            let id = s.trim_matches('"');
            return Ok(Some(id.to_string()));
        }
    }
    Ok(None)
}

/// Subcommands of Waytrogen. Every other option is global, so it may be passed
/// before or after the subcommand.
#[derive(Subcommand, Clone)]
pub enum Command {
    /// Launch Waytrogen with a given wallpaper folder, e.g.
    /// `waytrogen open ~/Pictures/wallpapers`.
    Open {
        /// Path to the wallpaper folder.
        path: PathBuf,
    },
    /// Mix every theme folder in `~/.config/themes` together with
    /// `Pictures/Wallpapers`, then use them for the window, `--next` and
    /// `--random`. Picking a folder in the window leaves the mixture.
    Mixture,
}

#[derive(Parser, Clone)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
    #[arg(short, long, global = true)]
    /// Restore previously set wallpapers.
    pub restore: bool,
    #[arg(long, global = true, default_value_t = 0)]
    /// How many error, warning, info, debug or trace logs will be shown. 0 for error, 1 for warning, 2 for info, 3 for debug, 4 or higher for trace.
    pub log_level: u8,
    #[arg(short, long, global = true, default_value_t = false)]
    /// Get the current wallpaper settings in JSON format.
    pub list_current_wallpapers: bool,
    #[arg(short, long, global = true, value_parser = parse_executable_script)]
    /// Path to external script.
    pub external_script: Option<String>,
    #[arg(long, global = true)]
    /// Set random wallpapers based on last set changer.
    pub random: bool,
    #[arg(short, long, global = true)]
    /// Get application version.
    pub version: bool,
    #[arg(short, long, global = true)]
    /// Cycle wallaper(s) the next on based on the previously set wallpaper(s) and sort settings on a given monitor. "All" cycles wallpapers on all monitors.
    pub next: Option<String>,
    #[arg(short, long, global = true, default_value_t = 0)]
    /// Startup delay to allow monitors to initialize.
    pub startup_delay: u64,
    #[arg(short, long, global = true)]
    /// Delete image cache.
    pub delete_cache: bool,
    #[arg(short = 'b', long, global = true)]
    /// Hide bottom bar
    pub hide_bottom_bar: Option<bool>,
    #[arg(long, global = true)]
    /// Run `matugen image <wallpaper>` before applying the wallpaper(s). Launches the app when used on its own.
    pub matugen: bool,
    /// Internal: themes the images read from stdin, one path per line. Started by
    /// waytrogen itself so it can exit before matugen finishes.
    #[arg(long, hide = true)]
    pub matugen_worker: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Cli {
        let mut argv = vec!["waytrogen"];
        argv.extend_from_slice(args);
        Cli::try_parse_from(argv).expect("Arguments should be valid")
    }

    #[test]
    fn open_takes_a_folder() {
        let args = parse(&["open", "/tmp/wallpapers"]);
        let Some(Command::Open { path }) = args.command else {
            panic!("open should be parsed as a subcommand");
        };
        assert_eq!(path, PathBuf::from("/tmp/wallpapers"));
    }

    #[test]
    fn open_requires_a_folder() {
        assert!(Cli::try_parse_from(["waytrogen", "open"]).is_err());
    }

    #[test]
    fn open_and_matugen_work_together() {
        for args in [
            ["open", "/tmp/wallpapers", "--matugen"],
            ["--matugen", "open", "/tmp/wallpapers"],
        ] {
            let cli = parse(&args);
            assert!(cli.matugen, "{args:?} should enable matugen");
            assert!(
                matches!(cli.command, Some(Command::Open { .. })),
                "{args:?} should open a folder"
            );
        }
    }

    #[test]
    fn options_without_a_subcommand_still_work() {
        assert!(parse(&["--restore"]).restore);
        assert!(parse(&["--random"]).random);
        assert!(parse(&["-r"]).restore);
        assert_eq!(parse(&["--next", "All"]).next.as_deref(), Some("All"));
        assert!(parse(&["--version"]).version);
        assert!(parse(&["--matugen"]).matugen);
        assert!(!parse(&[]).matugen);
        assert!(parse(&["open", "/tmp/wallpapers"]).command.is_some());
    }

    #[test]
    fn resolving_a_folder_requires_an_existing_directory() {
        let missing = parse(&["open", "/tmp/waytrogen-does-not-exist"]);
        assert!(resolve_open_folder(&missing).is_err());

        let file = std::env::temp_dir().join("waytrogen-not-a-directory");
        std::fs::write(&file, "").expect("Failed to write temporary file");
        let args = Cli {
            command: Some(Command::Open { path: file.clone() }),
            ..parse(&[])
        };
        let result = resolve_open_folder(&args);
        let _ = std::fs::remove_file(&file);
        assert!(result.is_err());
    }

    #[test]
    fn resolving_a_folder_makes_it_absolute() {
        let relative = Cli {
            command: Some(Command::Open {
                path: PathBuf::from("./src"),
            }),
            ..parse(&[])
        };
        let folder = resolve_open_folder(&relative)
            .expect("src should resolve")
            .expect("a folder was given");
        assert!(folder.is_absolute(), "{folder:?} should be absolute");
        assert!(folder.is_dir());
    }

    #[test]
    fn no_folder_is_resolved_without_the_subcommand() {
        assert_eq!(resolve_open_folder(&parse(&["--matugen"])), Ok(None));
    }

    #[test]
    fn mixture_takes_no_arguments() {
        for args in [
            &["mixture"][..],
            &["--matugen", "mixture"][..],
            &["mixture", "--matugen"][..],
        ] {
            let cli = parse(args);
            assert!(
                matches!(cli.command, Some(Command::Mixture)),
                "{args:?} should be a mixture"
            );
        }
        assert!(Cli::try_parse_from(["waytrogen", "mixture", "/tmp"]).is_err());
    }

    #[test]
    fn mixture_works_with_the_wallpaper_cycling_options() {
        for args in [
            &["mixture", "--next", "All"][..],
            &["--next", "All", "mixture"][..],
            &["mixture", "--random"][..],
        ] {
            let cli = parse(args);
            assert!(
                matches!(cli.command, Some(Command::Mixture)),
                "{args:?} should be a mixture"
            );
        }
        assert_eq!(
            parse(&["mixture", "--next", "All"]).next.as_deref(),
            Some("All")
        );
        assert!(parse(&["mixture", "--random"]).random);
    }
}
