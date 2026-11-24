//! Safe storage path resolution & profile name sanitization.

use std::path::{Path, PathBuf};

const RESERVED_WIN_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Validate and normalize a user-supplied profile name so it cannot escape `profiles_dir`
/// or fail on Windows filename rules.
pub fn sanitize_profile_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("方案名不能为空".into());
    }
    if trimmed.chars().count() > 80 {
        return Err("方案名过长（最多 80 个字符）".into());
    }
    if trimmed == "." || trimmed == ".." || trimmed.starts_with('.') || trimmed.ends_with('.') {
        return Err("方案名不能以点号开头或结尾".into());
    }
    if trimmed.contains("..") {
        return Err("方案名不能包含 '..'".into());
    }
    for ch in trimmed.chars() {
        if (ch as u32) < 0x20
            || matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        {
            return Err(format!("方案名包含非法字符 '{}'", ch));
        }
    }
    let stem_upper = trimmed
        .split('.')
        .next()
        .unwrap_or(trimmed)
        .to_ascii_uppercase();
    if RESERVED_WIN_NAMES.contains(&stem_upper.as_str()) {
        return Err(format!("方案名 '{}' 为 Windows 保留名称", trimmed));
    }
    Ok(trimmed.to_string())
}

/// Safely resolve `<profiles_dir>/<sanitized_name>.json` and guarantee it stays inside `profiles_dir`.
pub fn resolve_profile_path(profiles_dir: &Path, name: &str) -> Result<PathBuf, String> {
    let clean = sanitize_profile_name(name)?;
    let path = profiles_dir.join(format!("{}.json", clean));
    if path.parent() != Some(profiles_dir) {
        return Err("方案路径越界".into());
    }
    Ok(path)
}

fn dir_is_writable(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(format!(".c80_write_probe_{}", std::process::id()));
    match std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Resolve runtime data directory:
/// 1. Portable mode if `<exe_dir>/studio` exists and is writable (seamless compatibility).
/// 2. Portable mode (`<exe_dir>/data`) if `<exe_dir>/portable.txt` exists and `<exe_dir>` is writable.
/// 3. Standard `%APPDATA%\Crush80Studio` (safe even when installed under `C:\Program Files`).
pub fn resolve_data_dir_with(exe_dir: Option<&Path>, appdata: Option<&Path>) -> PathBuf {
    if let Some(ed) = exe_dir {
        let legacy_studio = ed.join("studio");
        if legacy_studio.is_dir() && dir_is_writable(&legacy_studio) {
            return legacy_studio;
        }
        if ed.join("portable.txt").is_file() {
            let portable_data = ed.join("data");
            if dir_is_writable(&portable_data) {
                return portable_data;
            }
        }
    }
    if let Some(ad) = appdata {
        return ad.join("Crush80Studio");
    }
    exe_dir
        .map(|d| d.join("data"))
        .unwrap_or_else(|| PathBuf::from("data"))
}

pub fn resolve_data_dir() -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from);
    resolve_data_dir_with(exe_dir.as_deref(), appdata.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_profile_name_blocks_traversal_and_reserved() {
        for bad in [
            "",
            "   ",
            ".",
            "..",
            "../autoswitch",
            "..\\autoswitch",
            "a/b",
            "a\\b",
            "C:\\evil",
            "a:b",
            "foo*bar",
            "foo?bar",
            "foo\"bar",
            "foo<bar>",
            "foo|bar",
            "trailing.",
            ".leading",
            "CON",
            "con",
            "NUL",
            "PRN",
            "AUX",
            "COM1",
            "LPT9",
        ] {
            assert!(sanitize_profile_name(bad).is_err(), "expected Err for {:?}", bad);
        }

        assert_eq!(sanitize_profile_name("  游戏配置 1  ").unwrap(), "游戏配置 1");
        assert_eq!(sanitize_profile_name("Gaming_Pro-v2").unwrap(), "Gaming_Pro-v2");
        assert_eq!(sanitize_profile_name("日本語プロファイル").unwrap(), "日本語プロファイル");
    }

    #[test]
    fn test_resolve_profile_path_stays_inside_profiles_dir() {
        let dir = Path::new(r"C:\Users\Test\AppData\Roaming\Crush80Studio\profiles");
        let p = resolve_profile_path(dir, "MyProfile").unwrap();
        assert_eq!(p.parent(), Some(dir));
        assert_eq!(p.file_name().unwrap(), "MyProfile.json");
        assert!(resolve_profile_path(dir, "..\\autoswitch").is_err());
    }

    #[test]
    fn test_resolve_data_dir_prefers_portable_studio_or_marker_else_appdata() {
        let tmp = std::env::temp_dir().join(format!("c80_storage_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let exe_dir = tmp.join("exe");
        let appdata = tmp.join("appdata");
        std::fs::create_dir_all(&exe_dir).unwrap();
        std::fs::create_dir_all(&appdata).unwrap();

        // No studio/ or portable.txt -> %APPDATA%\Crush80Studio
        assert_eq!(
            resolve_data_dir_with(Some(&exe_dir), Some(&appdata)),
            appdata.join("Crush80Studio")
        );

        // Creating portable.txt -> exe_dir/data
        std::fs::write(exe_dir.join("portable.txt"), "1").unwrap();
        assert_eq!(
            resolve_data_dir_with(Some(&exe_dir), Some(&appdata)),
            exe_dir.join("data")
        );

        // Creating studio/ -> exe_dir/studio (takes precedence for seamless compatibility)
        std::fs::create_dir_all(exe_dir.join("studio")).unwrap();
        assert_eq!(
            resolve_data_dir_with(Some(&exe_dir), Some(&appdata)),
            exe_dir.join("studio")
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
