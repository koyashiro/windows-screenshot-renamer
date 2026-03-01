#[cfg(not(windows))]
compile_error!("this program only supports Windows");

use std::fs::{DirEntry, OpenOptions, read_dir, rename};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use anyhow::Context as _;
use chrono::{DateTime, Local, SecondsFormat};
use clap::Parser;
use dirs_next::{data_local_dir, picture_dir};
use lazy_regex::{Lazy, Regex, lazy_regex};
use log::{debug, error, info};
use notify::{RecursiveMode, Watcher};
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIconBuilder};

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
        watch(&args.screenshots_dir, args.dry_run, &args.log_file)?;
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

fn watch(screenshot_dir: &Path, dry_run: bool, log_file: &Path) -> anyhow::Result<()> {
    hide_console_window();

    let paused = Arc::new(AtomicBool::new(false));

    let dir = screenshot_dir.to_path_buf();
    let paused_clone = Arc::clone(&paused);
    std::thread::spawn(move || {
        if let Err(e) = watch_and_rename(&dir, dry_run, &paused_clone) {
            error!("filesystem watcher error: {e}");
        }
    });

    let open_log_item = MenuItem::new("Open Log", true, None);
    let pause_item = MenuItem::new("Pause", true, None);
    let quit_item = MenuItem::new("Exit", true, None);
    let menu = Menu::new();
    menu.append(&pause_item)
        .context("failed to add menu item")?;
    menu.append(&open_log_item)
        .context("failed to add menu item")?;
    menu.append(&quit_item).context("failed to add menu item")?;

    let icon = create_tray_icon_image();
    let _tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip(env!("CARGO_PKG_NAME"))
        .with_icon(icon)
        .build()
        .context("failed to create tray icon")?;

    debug!("tray icon created");

    info!("watching \"{}\" for changes...", screenshot_dir.display());

    run_message_loop(&open_log_item, &pause_item, &quit_item, &paused, log_file);

    info!("exiting...");
    Ok(())
}

fn hide_console_window() {
    use windows::Win32::System::Console::GetConsoleWindow;
    use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, ShowWindow};

    unsafe {
        let hwnd = GetConsoleWindow();
        if !hwnd.0.is_null() {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
}

fn create_tray_icon_image() -> Icon {
    let size = 32u32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    for pixel in rgba.chunks_exact_mut(4) {
        pixel[0] = 0x33; // R
        pixel[1] = 0x99; // G
        pixel[2] = 0xFF; // B
        pixel[3] = 0xFF; // A
    }
    Icon::from_rgba(rgba, size, size).expect("failed to create tray icon")
}

fn run_message_loop(
    open_log_item: &MenuItem,
    pause_item: &MenuItem,
    quit_item: &MenuItem,
    paused: &AtomicBool,
    log_file: &Path,
) {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, MSG, PostQuitMessage, TranslateMessage,
    };

    let open_log_id = open_log_item.id().clone();
    let pause_id = pause_item.id().clone();
    let quit_id = quit_item.id().clone();
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);

            if let Ok(event) = MenuEvent::receiver().try_recv() {
                if event.id == open_log_id {
                    if let Err(e) = Command::new("notepad").arg(log_file).spawn() {
                        error!("failed to open log file: {e}");
                    }
                } else if event.id == pause_id {
                    let was_paused = paused.fetch_xor(true, Ordering::Relaxed);
                    if was_paused {
                        pause_item.set_text("Pause");
                        info!("watching resumed");
                    } else {
                        pause_item.set_text("Resume");
                        info!("watching paused");
                    }
                } else if event.id == quit_id {
                    info!("exit requested from tray menu");
                    PostQuitMessage(0);
                }
            }
        }
    }
}

fn watch_and_rename(
    screenshot_dir: &Path,
    dry_run: bool,
    paused: &AtomicBool,
) -> anyhow::Result<()> {
    let (tx, rx) = mpsc::channel();

    let mut watcher =
        notify::recommended_watcher(tx).context("failed to create filesystem watcher")?;
    watcher
        .watch(screenshot_dir, RecursiveMode::NonRecursive)
        .context("failed to watch screenshot directory")?;

    for result in rx {
        match result {
            Ok(_) => {
                std::thread::sleep(Duration::from_millis(200));
                if paused.load(Ordering::Relaxed) {
                    continue;
                }
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
