#[cfg(not(windows))]
compile_error!("this program only supports Windows");

use std::fs::{DirEntry, OpenOptions, read_dir, rename};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::Context as _;
use chrono::{DateTime, Local, SecondsFormat};
use clap::Parser;
use dirs_next::{data_local_dir, picture_dir};
use lazy_regex::{Lazy, Regex, lazy_regex};
use log::{debug, error, info};
use notify::{RecursiveMode, Watcher};

#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// Screenshots directory
    #[arg(long, default_value_os_t = default_screenshots_dir())]
    screenshots_dir: PathBuf,

    /// Log file path
    #[arg(long, default_value_os_t = default_log_file())]
    log_file: PathBuf,

    /// Watch for changes and automatically rename
    #[arg(long)]
    watch: bool,

    /// Dry run (print what would be renamed without actually renaming)
    #[arg(long)]
    dry_run: bool,
}

fn default_screenshots_dir() -> PathBuf {
    picture_dir()
        .map(|p| p.join("Screenshots"))
        .unwrap_or_else(|| PathBuf::from("Screenshots"))
}

fn default_log_file() -> PathBuf {
    let name = env!("CARGO_PKG_NAME");
    let file_name = format!("{name}.log");
    data_local_dir()
        .map(|p| p.join(name).join(&file_name))
        .unwrap_or_else(|| PathBuf::from(file_name))
}

static RE_SNIPPING_TOOL_JA: Lazy<Regex> =
    lazy_regex!(r"^スクリーンショット (\d{4}-\d{2}-\d{2} \d{6})\.png$");
static RE_OLD_SNIPPING_TOOL_JA: Lazy<Regex> =
    lazy_regex!(r"^スクリーンショット_(\d{4})(\d{2})(\d{2})_(\d{6})\.png$");
static RE_SCREENSHOT_JA: Lazy<Regex> = lazy_regex!(r"^スクリーンショット(?: \(\d+\))?\.png$");

fn main() -> ExitCode {
    let args = Args::parse();

    let log_file = &args.log_file;
    if let Some(parent) = log_file.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        eprintln!(
            "failed to create log directory \"{}\": {e}",
            parent.display()
        );
        return ExitCode::FAILURE;
    }
    let file = match OpenOptions::new().create(true).append(true).open(log_file) {
        Ok(file) => file,
        Err(e) => {
            eprintln!("failed to open log file \"{}\": {e}", log_file.display());
            return ExitCode::FAILURE;
        }
    };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format(|buf, record| {
            use std::io::Write;
            let now = Local::now();
            writeln!(
                buf,
                "[{} {:5} {}] {}",
                now.to_rfc3339_opts(SecondsFormat::Millis, false),
                record.level(),
                record.target(),
                record.args()
            )
        })
        .target(env_logger::Target::Pipe(Box::new(file)))
        .init();

    if let Err(e) = run(&args) {
        error!("{e}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

fn run(args: &Args) -> anyhow::Result<()> {
    scan_and_rename(&args.screenshots_dir, args.dry_run)?;

    if args.watch {
        watch(&args.screenshots_dir, args.dry_run)?;
    }

    Ok(())
}

fn scan_and_rename(screenshot_dir: &Path, dry_run: bool) -> anyhow::Result<()> {
    let screenshot_files =
        read_dir(screenshot_dir).context("failed to read screenshot directory")?;

    for entry in screenshot_files {
        let Ok(entry) = entry else {
            error!("failed to read screenshot file entry");
            continue;
        };

        process_entry(screenshot_dir, &entry, dry_run);
    }

    Ok(())
}

fn process_entry(screenshot_dir: &Path, entry: &DirEntry, dry_run: bool) {
    let Ok(file_name) = entry.file_name().into_string() else {
        error!(
            "failed to convert file name to string: {:?}",
            entry.file_name()
        );
        return;
    };

    let Ok(new_file_name) = new_file_name(entry, &file_name) else {
        error!("failed to determine new file name for \"{}\"", file_name);
        return;
    };
    let Some(new_file_name) = new_file_name else {
        debug!("skipping \"{}\"", file_name);
        return;
    };
    let new_path = screenshot_dir.join(&new_file_name);

    let old_path = entry.path();

    if new_path.exists() {
        error!(
            "failed to rename \"{}\" to \"{}\": destination already exists",
            old_path.display(),
            new_path.display()
        );
        return;
    }

    if dry_run {
        info!(
            "\"{}\" => \"{}\" (dry run)",
            old_path.display(),
            new_path.display()
        );
        return;
    }

    if let Err(e) = rename(&old_path, &new_path) {
        error!(
            "failed to rename \"{}\" to \"{}\": {e}",
            old_path.display(),
            new_path.display()
        );
        return;
    }
    info!("\"{}\" => \"{}\"", old_path.display(), new_path.display());
}

fn watch(screenshot_dir: &Path, dry_run: bool) -> anyhow::Result<()> {
    let (tx, rx) = mpsc::channel();

    let mut watcher =
        notify::recommended_watcher(tx).context("failed to create filesystem watcher")?;
    watcher
        .watch(screenshot_dir, RecursiveMode::NonRecursive)
        .context("failed to watch screenshot directory")?;

    info!("watching \"{}\" for changes...", screenshot_dir.display());

    for result in rx {
        match result {
            Ok(_) => {
                std::thread::sleep(Duration::from_millis(200));
                if let Err(e) = scan_and_rename(screenshot_dir, dry_run) {
                    error!("failed to scan and rename: {e}");
                }
            }
            Err(e) => {
                error!("watch error: {e}");
            }
        }
    }

    Ok(())
}

fn new_file_name(entry: &DirEntry, file_name: &str) -> anyhow::Result<Option<String>> {
    if let Some(caps) = RE_SNIPPING_TOOL_JA.captures(file_name) {
        return Ok(Some(format!("Screenshot {}.png", &caps[1])));
    }

    if let Some(caps) = RE_OLD_SNIPPING_TOOL_JA.captures(file_name) {
        return Ok(Some(format!(
            "Screenshot {}-{}-{} {}.png",
            &caps[1], &caps[2], &caps[3], &caps[4]
        )));
    }

    if RE_SCREENSHOT_JA.is_match(file_name) {
        let metadata = entry.metadata().context("failed to read file metadata")?;
        let mtime = metadata.modified().context("failed to read mtime")?;
        let dt: DateTime<Local> = mtime.into();
        return Ok(Some(format!(
            "Screenshot {}.png",
            dt.format("%Y-%m-%d %H%M%S")
        )));
    }

    Ok(None)
}
