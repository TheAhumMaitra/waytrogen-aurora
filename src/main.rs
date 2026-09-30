use clap::Parser;
use gtk::{gio::Settings, glib, prelude::*};
use log::error;
use std::{thread::sleep, time::Duration};
use waytrogen::{
    cli::{
        cycle_next_wallpaper, delete_image_cache, launch_application, print_app_version,
        print_wallpaper_state, resolve_open_folder, restore_wallpapers, set_random_wallpapers, Cli,
        Command,
    },
    common::APP_ID,
    dotfile::{self, get_config_file},
    fs::mixture_folders,
    main_window::{clear_mixture_folders, set_mixture_folders},
};

fn main() -> glib::ExitCode {
    let mut args = Cli::parse();

    stderrlog::new()
        .module(module_path!())
        .verbosity(args.log_level as usize)
        .init()
        .unwrap();

    // Detached matugen worker: it only themes what the parent handed over, so it
    // skips the whole startup and never opens a window. It runs after the logger
    // so matugen failures are still reported.
    if args.matugen_worker {
        return if waytrogen::matugen::run_theme_worker() {
            glib::ExitCode::SUCCESS
        } else {
            glib::ExitCode::FAILURE
        };
    }

    let config_file = match get_config_file() {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to get config file: {e}");
            return glib::ExitCode::FAILURE;
        }
    };

    match config_file.write_to_gsettings() {
        Ok(_) => {}
        Err(e) => {
            error!("Failed to write gsettings from configuration file: {e}");
            return glib::ExitCode::FAILURE;
        }
    }

    if args.external_script.is_none() && !config_file.executable_script.is_empty() {
        args.external_script = Some(config_file.executable_script);
    }

    // Stored before dispatching so that the window, `--next` and `--random` all
    // use the same folders.
    if matches!(args.command, Some(Command::Mixture)) {
        let settings = Settings::new(APP_ID);
        if set_mixture_folders(&settings, &mixture_folders()).is_empty() {
            error!("No wallpaper folders found for the mixture");
        }
    }

    if args.restore {
        sleep(Duration::from_millis(args.startup_delay));
        restore_wallpapers(&args)
    } else if args.list_current_wallpapers {
        print_wallpaper_state()
    } else if args.random {
        sleep(Duration::from_millis(args.startup_delay));
        set_random_wallpapers(&args)
    } else if args.version {
        print_app_version()
    } else if args.next.is_some() {
        sleep(Duration::from_millis(args.startup_delay));
        cycle_next_wallpaper(&args)
    } else if args.delete_cache {
        delete_image_cache()
    } else {
        match resolve_open_folder(&args) {
            Ok(Some(folder)) => {
                let settings = Settings::new(APP_ID);
                // `open` names one folder, so it also leaves the mixture.
                clear_mixture_folders(&settings);
                if let Err(e) =
                    settings.set_string("wallpaper-folder", folder.to_string_lossy().as_ref())
                {
                    error!("Failed to set wallpaper folder, {e}");
                    return glib::ExitCode::FAILURE;
                }
            }
            Ok(None) => {}
            Err(e) => {
                error!("{e}");
                return glib::ExitCode::FAILURE;
            }
        }
        let _ = launch_application(args);

        let config_file = match dotfile::ConfigFile::from_gsettings() {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to get config file: {e}");
                return glib::ExitCode::FAILURE;
            }
        };
        match config_file.write_to_config_file() {
            Ok(_) => glib::ExitCode::SUCCESS,
            Err(_) => glib::ExitCode::FAILURE,
        }
    }
}
