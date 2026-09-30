use log::{debug, error};
use std::{
    collections::HashSet,
    ffi::OsString,
    io::{BufRead, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        mpsc::{self, Receiver, Sender},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

/// Index of the colour matugen should extract from the image, `0` being the most
/// dominant one. Passing it is what stops matugen from showing its interactive
/// "Select the color you want to use as source color" prompt.
const SOURCE_COLOR_INDEX: &str = "0";

/// Material You scheme. The default `scheme-tonal-spot` washes the palette out
/// towards the wallpaper's own colours, which is what made waybar and the other
/// bars turn sky blue; `scheme-vibrant` keeps the hues bold instead.
const SCHEME: &str = "scheme-vibrant";

/// How often the worker checks whether a run finished or a newer wallpaper
/// arrived.
const WORKER_POLL: Duration = Duration::from_millis(100);

/// Tracks which images matugen already handled, so several monitors using the
/// same wallpaper only trigger one run. Clones share the state.
#[derive(Default, Clone)]
pub struct Matugen {
    /// `None` when matugen is disabled, so nothing is ever started.
    generated_images: Option<Arc<Mutex<HashSet<PathBuf>>>>,
}

impl Matugen {
    #[must_use]
    pub fn new(enabled: bool) -> Self {
        Self {
            generated_images: enabled.then(|| Arc::new(Mutex::new(HashSet::new()))),
        }
    }

    /// Runs `matugen image <image>` and reports whether it succeeded, unless
    /// matugen is disabled or the same image was already handled.
    pub fn generate(&self, image: &Path) -> bool {
        if !self.take_image(image) {
            return false;
        }
        run_matugen(image).unwrap_or_default()
    }

    /// Registers `image` as handled and reports whether matugen still has to run
    /// for it. Empty paths come from wallpapers saved before the folder was
    /// known, so they are skipped without running matugen.
    pub fn take_image(&self, image: &Path) -> bool {
        if image.as_os_str().is_empty() {
            debug!("Skipping matugen for a wallpaper without a path");
            return false;
        }
        if self.generated_images.is_none() {
            return false;
        }
        if let Err(e) = check_image(image) {
            error!("Skipping matugen for {image:?}, {e}");
            return false;
        }
        lock(self.generated_images.as_ref().expect("just checked")).insert(image.to_path_buf())
    }
}

/// Runs matugen in the background for a window: the wallpaper is applied right
/// away and a run that a newer wallpaper made pointless is dropped, so the theme
/// files always end up matching the last wallpaper that was picked.
#[derive(Clone)]
pub struct MatugenWorker {
    requests: Sender<PathBuf>,
}

impl MatugenWorker {
    #[must_use]
    pub fn spawn() -> Self {
        let (requests, receiver) = mpsc::channel::<PathBuf>();
        thread::spawn(move || run_requests(receiver));
        Self { requests }
    }

    /// Queues `image`. The worker only ever runs the newest wallpaper, so
    /// clicking through a gallery quickly does not leave matugen behind.
    pub fn submit(&self, image: &Path) {
        if let Err(e) = self.requests.send(image.to_path_buf()) {
            error!("The matugen worker is gone, {e}");
        }
    }
}

/// Themes `images`, one after the other. Used by the detached worker process, so
/// a one shot command can exit without waiting several seconds for matugen.
#[must_use]
pub fn theme_images(images: &[PathBuf]) -> bool {
    let matugen = Matugen::new(true);
    let mut succeeded = true;
    for image in images {
        succeeded &= matugen.generate(image);
    }
    succeeded
}

/// The detached worker: reads one image path per line from stdin, themes each
/// one and exits. `waytrogen` hands the paths over through a pipe and returns.
#[must_use]
pub fn run_theme_worker() -> bool {
    let stdin = std::io::stdin();
    let images: Vec<PathBuf> = stdin
        .lock()
        .lines()
        .map_while(std::result::Result::ok)
        .filter(|line| !line.trim().is_empty())
        .map(PathBuf::from)
        .collect();
    debug!("Matugen worker received {} image(s)", images.len());
    for image in &images {
        debug!("Matugen worker got {image:?}");
    }
    theme_images(&images)
}

/// Hands the newest of `images` to the detached worker. matugen writes a single
/// theme for the whole session, so theming several wallpapers of a multi monitor
/// command would only end up throwing all but the last one away.
pub fn theme_latest_in_background(images: &[PathBuf]) {
    if let Some(latest) = images.last() {
        theme_images_in_background(std::slice::from_ref(latest));
    }
}

/// Starts the detached worker for `images` without waiting for it, so a one shot
/// command returns immediately while matugen writes the theme files.
pub fn theme_images_in_background(images: &[PathBuf]) {
    if images.is_empty() {
        return;
    }
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => {
            error!("Failed to find the waytrogen binary for matugen, {e}");
            return;
        }
    };
    let child = Command::new(exe)
        .arg("--matugen-worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(e) => {
            error!("Failed to start the matugen worker, {e}");
            return;
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        for image in images {
            if let Err(e) = writeln!(stdin, "{}", image.display()) {
                error!("Failed to hand {image:?} to the matugen worker, {e}");
                return;
            }
        }
    }
    debug!("Started the matugen worker for {images:?}");
    // The worker keeps running after this process exits; its stdin closes when
    // the pipe above is dropped.
    drop(child);
}

/// Runs the queued images, killing the previous run as soon as a newer one
/// arrives so that two matugen processes never write the same theme file at once.
fn run_requests(receiver: Receiver<PathBuf>) {
    let mut running: Option<Child> = None;
    loop {
        match receiver.recv_timeout(WORKER_POLL) {
            Ok(mut image) => {
                // Anything queued while we were waiting is outdated.
                while let Ok(newer) = receiver.try_recv() {
                    image = newer;
                }
                stop(&mut running);
                debug!("Running matugen on {}", image.display());
                match matugen_command(&image)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .spawn()
                {
                    Ok(child) => running = Some(child),
                    Err(e) => error!("Failed to run matugen, {e}"),
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if running
                    .as_mut()
                    .is_some_and(|child| matches!(child.try_wait(), Ok(Some(_))))
                {
                    debug!("matugen finished successfully");
                    running = None;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    stop(&mut running);
}

/// Kills a running matugen, together with the app reloads it may have started.
fn stop(running: &mut Option<Child>) {
    let Some(mut child) = running.take() else {
        return;
    };
    debug!("Dropping the matugen run of a newer wallpaper");
    let _ = child.kill();
    let _ = child.wait();
}

/// Runs matugen and reports whether it succeeded, `None` when it could not run.
fn run_matugen(image: &Path) -> Option<bool> {
    if let Err(e) = check_image(image) {
        error!("Skipping matugen for {image:?}, {e}");
        return Some(false);
    }
    debug!(
        "Running matugen on {} ({} KiB)",
        image.display(),
        std::fs::metadata(image).map_or(0, |metadata| metadata.len()) / 1024
    );
    let mut command = matugen_command(image);
    debug!("Running {command:?}");
    match command
        // Never let matugen block waiting for the source colour picker.
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .status()
    {
        Ok(status) if status.success() => {
            debug!("matugen finished successfully");
            Some(true)
        }
        Ok(status) => {
            error!("matugen exited with {status}");
            Some(false)
        }
        Err(e) => {
            error!("Failed to run matugen, {e}");
            None
        }
    }
}

/// Checks matugen can be pointed at `image` and logs what it will find.
fn check_image(image: &Path) -> Result<(), String> {
    let metadata =
        std::fs::metadata(image).map_err(|e| format!("the image cannot be read, {e}"))?;
    if !metadata.is_file() {
        return Err("the path is not a file".to_owned());
    }
    if metadata.len() == 0 {
        return Err("the image is empty".to_owned());
    }
    Ok(())
}

/// Locks a mutex, recovering from a panic in another thread.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Builds the matugen invocation used to theme from `image`. Kept separate from
/// the IO of [`Matugen::generate`] so tests can inspect and extend it.
fn matugen_command(image: &Path) -> Command {
    let mut command = Command::new("matugen");
    command.args(matugen_args(image));
    command
}

/// The arguments matugen is themed with, kept separate from the process so tests
/// can check them without running it.
fn matugen_args(image: &Path) -> Vec<OsString> {
    vec![
        "image".into(),
        image.into(),
        "--source-color-index".into(),
        SOURCE_COLOR_INDEX.into(),
        "--type".into(),
        SCHEME.into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates a wallpaper matugen can actually be pointed at, and returns its
    /// path. The images are never decoded, so any readable file will do.
    fn wallpaper(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join("waytrogen-matugen-tests");
        std::fs::create_dir_all(&folder).expect("test folder");
        let path = folder.join(name);
        std::fs::write(&path, b"not a real image").expect("test wallpaper");
        path
    }

    /// The arguments matugen would be run with.
    fn args(image: &Path) -> Vec<String> {
        matugen_args(image)
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn command_themes_the_wallpaper_without_prompting() {
        let image = Path::new("/tmp/waytrogen/matugen/test.png");
        let args = args(image);
        assert_eq!(args[0], "image");
        assert_eq!(args[1], image.to_str().unwrap());
        // Without the index matugen opens its interactive colour picker and waits.
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--source-color-index", "0"]));
        // The default scheme washed the colours out towards the wallpaper's own.
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--type", "scheme-vibrant"]));
    }

    #[test]
    fn generating_only_runs_once_per_image() {
        let image = wallpaper("once.png");
        let matugen = Matugen::new(true);
        // Clones share the state, so several monitors cannot theme the same image
        // twice or out of order.
        let clone = matugen.clone();
        assert!(matugen.take_image(&image));
        assert!(!clone.take_image(&image));
        assert!(!matugen.take_image(&image));
    }

    #[test]
    fn different_images_are_all_queued() {
        let matugen = Matugen::new(true);
        let first = wallpaper("first.png");
        let second = wallpaper("second.png");
        assert!(matugen.take_image(&first));
        assert!(matugen.take_image(&second));
    }

    #[test]
    fn generating_is_skipped_when_disabled() {
        let image = wallpaper("disabled.png");
        assert!(!Matugen::new(false).take_image(&image));
    }

    #[test]
    fn wallpapers_without_a_path_are_skipped() {
        // Saved wallpapers can predate the folder they were picked from.
        let matugen = Matugen::new(true);
        assert!(!matugen.take_image(Path::new("")));
    }

    #[test]
    fn wallpapers_that_cannot_be_read_are_skipped() {
        let matugen = Matugen::new(true);
        assert!(!matugen.take_image(Path::new("/tmp/waytrogen/matugen/missing.png")));
        assert!(!matugen.take_image(Path::new("/tmp/waytrogen/matugen")));
        // An empty file would only make matugen fail after it started.
        let empty = std::env::temp_dir().join("waytrogen-matugen-tests/empty.png");
        std::fs::write(&empty, b"").expect("empty wallpaper");
        assert!(!matugen.take_image(&empty));
    }

    #[test]
    fn theming_no_images_does_nothing() {
        // A command that changed no wallpaper must not start a worker.
        theme_images_in_background(&[]);
        assert!(theme_images(&[]));
    }
}
