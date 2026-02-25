# windows-screenshot-renamer

A tool that renames screenshot files saved in the Japanese locale on Windows to English format. It also renames files with sequential numbering (e.g. `スクリーンショット (1).png`) to a datetime-based name derived from the file's modification time.

Targets files in the `%USERPROFILE%\Pictures\Screenshots` directory.

## Supported Naming Conventions

| Before                                                  | After                                                   |
| ------------------------------------------------------- | ------------------------------------------------------- |
| `スクリーンショット 2026-02-13 123456.png`              | `Screenshot 2026-02-13 123456.png`                      |
| `スクリーンショット_20260213_123456.png`                | `Screenshot 2026-02-13 123456.png`                      |
| `スクリーンショット.png` / `スクリーンショット (1).png` | `Screenshot 2026-02-13 123456.png` (derived from mtime) |

## Installation

> [!NOTE]
> This tool is Windows-only.

```powershell
cargo install --git https://github.com/koyashiro/windows-screenshot-renamer
```

## Usage

```powershell
windows-screenshot-renamer.exe
```

### Options

| Flag                                  | Description                                           | Default                                                                    |
| ------------------------------------- | ----------------------------------------------------- | -------------------------------------------------------------------------- |
| `--screenshots-dir <SCREENSHOTS_DIR>` | Screenshots directory                                 | `%USERPROFILE%\Pictures\Screenshots`                                       |
| `--log-file <LOG_FILE>`               | Log file path                                         | `%LOCALAPPDATA%\windows-screenshot-renamer\windows-screenshot-renamer.log` |
| `--watch`                             | Watch for changes and automatically rename            |                                                                            |
| `--dry-run`                           | Print what would be renamed without actually renaming |                                                                            |

### Examples

Watch mode:

```powershell
windows-screenshot-renamer.exe --watch
```

Dry run:

```powershell
windows-screenshot-renamer.exe --dry-run
```

The log level can be changed via the `RUST_LOG` environment variable.

```powershell
$env:RUST_LOG='debug'; windows-screenshot-renamer.exe
```
