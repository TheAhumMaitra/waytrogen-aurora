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

/// How often the worker checks whether a run finished or a newer wallpaper
/// arrived.
const WORKER_POLL: Duration = Duration::from_millis(100);

/// Tracks which images wallust already handled, so several monitors using the
/// same wallpaper only trigger one run. Clones share the state.
#[derive(Default, Clone)]
pub struct Wallust {
    /// `None` when wallust is disabled, so nothing is ever started.
    generated_images: Option<Arc<Mutex<HashSet<PathBuf>>>>,
}

impl Wallust {
    #[must_use]
    pub fn new(enabled: bool) -> Self {
        Self {
            generated_images: enabled.then(|| Arc::new(Mutex::new(HashSet::new()))),
        }
    }

    /// Runs `wallust run <image>` and reports whether it succeeded, unless
    /// wallust is disabled or the same image was already handled.
    pub fn generate(&self, image: &Path) -> bool {
        if !self.take_image(image) {
            return false;
        }
        run_wallust(image).unwrap_or_default()
    }

    /// Registers `image` as handled and reports whether wallust still has to run
    /// for it. Empty paths come from wallpapers saved before the folder was
    /// known, so they are skipped without running wallust.
    pub fn take_image(&self, image: &Path) -> bool {
        if image.as_os_str().is_empty() {
            debug!("Skipping wallust for a wallpaper without a path");
            return false;
        }
        if self.generated_images.is_none() {
            return false;
        }
        if let Err(e) = check_image(image) {
            error!("Skipping wallust for {image:?}, {e}");
            return false;
        }
        lock(self.generated_images.as_ref().expect("just checked")).insert(image.to_path_buf())
    }
}

/// Runs wallust in the background for a window: the wallpaper is applied right
/// away and a run that a newer wallpaper made pointless is dropped, so the theme
/// files always end up matching the last wallpaper that was picked.
#[derive(Clone)]
pub struct WallustWorker {
    requests: Sender<PathBuf>,
}

impl WallustWorker {
    #[must_use]
    pub fn spawn() -> Self {
        let (requests, receiver) = mpsc::channel::<PathBuf>();
        thread::spawn(move || run_requests(receiver));
        Self { requests }
    }

    /// Queues `image`. The worker only ever runs the newest wallpaper, so
    /// clicking through a gallery quickly does not leave wallust behind.
    pub fn submit(&self, image: &Path) {
        if let Err(e) = self.requests.send(image.to_path_buf()) {
            error!("The wallust worker is gone, {e}");
        }
    }
}

/// Themes `images`, one after the other. Used by the detached worker process, so
/// a one shot command can exit without waiting several seconds for wallust.
#[must_use]
pub fn theme_images(images: &[PathBuf]) -> bool {
    let wallust = Wallust::new(true);
    let mut succeeded = true;
    for image in images {
        succeeded &= wallust.generate(image);
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
    debug!("Wallust worker received {} image(s)", images.len());
    for image in &images {
        debug!("Wallust worker got {image:?}");
    }
    theme_images(&images)
}

/// Hands the newest of `images` to the detached worker. wallust writes a single
/// theme for the whole session, so theming several wallpapers of a multi monitor
/// command would only end up throwing all but the last one away.
pub fn theme_latest_in_background(images: &[PathBuf]) {
    if let Some(latest) = images.last() {
        theme_images_in_background(std::slice::from_ref(latest));
    }
}

/// Starts the detached worker for `images` without waiting for it, so a one shot
/// command returns immediately while wallust writes the theme files.
pub fn theme_images_in_background(images: &[PathBuf]) {
    if images.is_empty() {
        return;
    }
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => {
            error!("Failed to find the waytrogen binary for wallust, {e}");
            return;
        }
    };
    let child = Command::new(exe)
        .arg("--wallust-worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(e) => {
            error!("Failed to start the wallust worker, {e}");
            return;
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        for image in images {
            if let Err(e) = writeln!(stdin, "{}", image.display()) {
                error!("Failed to hand {image:?} to the wallust worker, {e}");
                return;
            }
        }
    }
    debug!("Started the wallust worker for {images:?}");
    // The worker keeps running after this process exits; its stdin closes when
    // the pipe above is dropped.
    drop(child);
}

/// Runs the queued images, killing the previous run as soon as a newer one
/// arrives so that two wallust processes never write the same theme file at once.
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
                debug!("Running wallust on {}", image.display());
                match wallust_command(&image)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .spawn()
                {
                    Ok(child) => running = Some(child),
                    Err(e) => error!("Failed to run wallust, {e}"),
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if running
                    .as_mut()
                    .is_some_and(|child| matches!(child.try_wait(), Ok(Some(_))))
                {
                    debug!("wallust finished successfully");
                    running = None;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    stop(&mut running);
}

/// Kills a running wallust, together with the app reloads it may have started.
fn stop(running: &mut Option<Child>) {
    let Some(mut child) = running.take() else {
        return;
    };
    debug!("Dropping the wallust run of a newer wallpaper");
    let _ = child.kill();
    let _ = child.wait();
}

/// Runs wallust and reports whether it succeeded, `None` when it could not run.
fn run_wallust(image: &Path) -> Option<bool> {
    if let Err(e) = check_image(image) {
        error!("Skipping wallust for {image:?}, {e}");
        return Some(false);
    }
    debug!(
        "Running wallust on {} ({} KiB)",
        image.display(),
        std::fs::metadata(image).map_or(0, |metadata| metadata.len()) / 1024
    );
    let mut command = wallust_command(image);
    debug!("Running {command:?}");
    match command
        // wallust is non interactive on a file, but closing stdin keeps a
        // terminal from being handed to a child waytrogen does not read.
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .status()
    {
        Ok(status) if status.success() => {
            debug!("wallust finished successfully");
            Some(true)
        }
        Ok(status) => {
            error!("wallust exited with {status}");
            Some(false)
        }
        Err(e) => {
            error!("Failed to run wallust, {e}");
            None
        }
    }
}

/// Checks wallust can be pointed at `image` and logs what it will find.
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

/// Builds the wallust invocation used to theme from `image`. Kept separate from
/// the IO of [`Wallust::generate`] so tests can inspect and extend it.
fn wallust_command(image: &Path) -> Command {
    let mut command = Command::new("wallust");
    command.args(wallust_args(image));
    command
}

/// The arguments wallust is themed with, kept separate from the process so tests
/// can check them without running it.
fn wallust_args(image: &Path) -> Vec<OsString> {
    vec!["run".into(), image.into()]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates a wallpaper wallust can actually be pointed at, and returns its
    /// path. The images are never decoded, so any readable file will do.
    fn wallpaper(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join("waytrogen-wallust-tests");
        std::fs::create_dir_all(&folder).expect("test folder");
        let path = folder.join(name);
        std::fs::write(&path, b"not a real image").expect("test wallpaper");
        path
    }

    /// The arguments wallust would be run with.
    fn args(image: &Path) -> Vec<String> {
        wallust_args(image)
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn command_themes_the_wallpaper() {
        let image = Path::new("/tmp/waytrogen/wallust/test.png");
        let args = args(image);
        // wallust only ever takes a single image file, never a folder, so the
        // wallpaper itself has to be the one path handed over.
        assert_eq!(args, vec!["run", image.to_str().unwrap()]);
    }

    #[test]
    fn generating_only_runs_once_per_image() {
        let image = wallpaper("once.png");
        let wallust = Wallust::new(true);
        // Clones share the state, so several monitors cannot theme the same image
        // twice or out of order.
        let clone = wallust.clone();
        assert!(wallust.take_image(&image));
        assert!(!clone.take_image(&image));
        assert!(!wallust.take_image(&image));
    }

    #[test]
    fn different_images_are_all_queued() {
        let wallust = Wallust::new(true);
        let first = wallpaper("first.png");
        let second = wallpaper("second.png");
        assert!(wallust.take_image(&first));
        assert!(wallust.take_image(&second));
    }

    #[test]
    fn generating_is_skipped_when_disabled() {
        let image = wallpaper("disabled.png");
        assert!(!Wallust::new(false).take_image(&image));
    }

    #[test]
    fn wallpapers_without_a_path_are_skipped() {
        // Saved wallpapers can predate the folder they were picked from.
        let wallust = Wallust::new(true);
        assert!(!wallust.take_image(Path::new("")));
    }

    #[test]
    fn wallpapers_that_cannot_be_read_are_skipped() {
        let wallust = Wallust::new(true);
        assert!(!wallust.take_image(Path::new("/tmp/waytrogen/wallust/missing.png")));
        assert!(!wallust.take_image(Path::new("/tmp/waytrogen/wallust")));
        // An empty file would only make wallust fail after it started.
        let empty = std::env::temp_dir().join("waytrogen-wallust-tests/empty.png");
        std::fs::write(&empty, b"").expect("empty wallpaper");
        assert!(!wallust.take_image(&empty));
    }

    #[test]
    fn theming_no_images_does_nothing() {
        // A command that changed no wallpaper must not start a worker.
        theme_images_in_background(&[]);
        assert!(theme_images(&[]));
    }
}
