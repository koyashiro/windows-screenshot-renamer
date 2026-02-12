#[cfg(not(windows))]
compile_error!("this program only supports Windows");

use std::fs::{DirEntry, read_dir, rename};
use std::process::ExitCode;

use anyhow::{Context as _, anyhow};
use chrono::{DateTime, Local};
use dirs_next::picture_dir;
use lazy_regex::{Lazy, Regex, lazy_regex};
use log::{debug, error, info};

static RE_SNIPPING_TOOL_JA: Lazy<Regex> =
    lazy_regex!(r"^スクリーンショット (\d{4}-\d{2}-\d{2} \d{6})\.png$");
static RE_OLD_SNIPPING_TOOL_JA: Lazy<Regex> =
    lazy_regex!(r"^スクリーンショット_(\d{4})(\d{2})(\d{2})_(\d{6})\.png$");
static RE_SCREENSHOT_JA: Lazy<Regex> = lazy_regex!(r"^スクリーンショット(?: \(\d+\))?\.png$");

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    if let Err(e) = run() {
        error!("{e}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

fn run() -> anyhow::Result<()> {
    let screenshot_dir = screenshot_dir().context("failed to determine screenshot directory")?;
    let screenshot_files =
        read_dir(&screenshot_dir).context("failed to read screenshot directory")?;

    for entry in screenshot_files {
        let Ok(entry) = entry else {
            error!("failed to read screenshot file entry");
            continue;
        };

        let Ok(file_name) = entry.file_name().into_string() else {
            error!(
                "failed to convert file name to string: {:?}",
                entry.file_name()
            );
            continue;
        };

        let Ok(new_file_name) = new_file_name(&entry, &file_name) else {
            error!("failed to determine new file name for {:?}", file_name);
            continue;
        };
        let Some(new_file_name) = new_file_name else {
            debug!("skipping {:?}", file_name);
            continue;
        };
        let new_path = screenshot_dir.join(&new_file_name);

        if let Err(e) = rename(entry.path(), &new_path) {
            error!(
                "failed to rename {:?} to {:?}: {e}",
                file_name, new_file_name
            );
            continue;
        }
        info!("{:?} => {:?}", file_name, new_file_name);
    }

    Ok(())
}

fn screenshot_dir() -> anyhow::Result<std::path::PathBuf> {
    let picture_dir =
        picture_dir().ok_or_else(|| anyhow!("failed to determine picture directory"))?;

    Ok(picture_dir.join("Screenshots"))
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
