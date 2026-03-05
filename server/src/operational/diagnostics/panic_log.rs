use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

const DEFAULT_PANIC_LOG_DIR: &str = "data";
const DEFAULT_PANIC_LOG_FILE: &str = "panic.log";
const DEFAULT_PANIC_LOG_MIN_INTERVAL_SECS: u64 = 5;

static LAST_PANIC_LOG_UNIX_SECS: AtomicU64 = AtomicU64::new(0);

pub fn install_panic_logging_hook() {
    std::panic::set_hook(Box::new(|panic_info| {
        eprintln!("PANIC: {}", panic_info);
        if let Some(location) = panic_info.location() {
            eprintln!(
                "Location: {}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            );
        }

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if !should_rate_limit_panic_log(timestamp) {
            append_panic_log_entry(timestamp, panic_info);
        }

        eprintln!("Backtrace:\n{:?}", std::backtrace::Backtrace::capture());
    }));
}

fn parse_u64_env(var_name: &str, default_value: u64) -> u64 {
    std::env::var(var_name)
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default_value)
}

fn sanitize_panic_log_file_name(raw: &str) -> Option<String> {
    let candidate = raw.trim();
    if candidate.is_empty()
        || candidate.contains('/')
        || candidate.contains('\\')
        || candidate.contains("..")
    {
        return None;
    }
    if !candidate
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.')
    {
        return None;
    }
    Some(candidate.to_owned())
}

fn panic_log_path_from_env() -> PathBuf {
    let log_dir = std::env::var("MGS_PANIC_LOG_DIR")
        .ok()
        .map(|raw| raw.trim().to_owned())
        .filter(|raw| !raw.is_empty())
        .unwrap_or_else(|| DEFAULT_PANIC_LOG_DIR.to_owned());
    let log_file = std::env::var("MGS_PANIC_LOG_FILE")
        .ok()
        .and_then(|raw| sanitize_panic_log_file_name(&raw))
        .unwrap_or_else(|| DEFAULT_PANIC_LOG_FILE.to_owned());
    Path::new(&log_dir).join(log_file)
}

fn should_rate_limit_panic_log(unix_secs: u64) -> bool {
    let min_interval_secs = parse_u64_env(
        "MGS_PANIC_LOG_MIN_INTERVAL_SECS",
        DEFAULT_PANIC_LOG_MIN_INTERVAL_SECS,
    )
    .clamp(1, 3600);

    loop {
        let previous = LAST_PANIC_LOG_UNIX_SECS.load(AtomicOrdering::Relaxed);
        if unix_secs.saturating_sub(previous) < min_interval_secs {
            return true;
        }
        if LAST_PANIC_LOG_UNIX_SECS
            .compare_exchange(
                previous,
                unix_secs,
                AtomicOrdering::Relaxed,
                AtomicOrdering::Relaxed,
            )
            .is_ok()
        {
            return false;
        }
    }
}

fn append_panic_log_entry(unix_secs: u64, panic_info: &std::panic::PanicHookInfo<'_>) {
    let panic_log_path = panic_log_path_from_env();
    if let Some(parent_dir) = panic_log_path.parent() {
        let _ = std::fs::create_dir_all(parent_dir);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&panic_log_path)
    {
        use std::io::Write;
        if let Some(location) = panic_info.location() {
            let _ = writeln!(
                file,
                "PANIC at {} [{}:{}:{}]: {}",
                unix_secs,
                location.file(),
                location.line(),
                location.column(),
                panic_info
            );
        } else {
            let _ = writeln!(file, "PANIC at {}: {}", unix_secs, panic_info);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_env_var<T>(key: &str, value: Option<&str>, f: impl FnOnce() -> T) -> T {
        temp_env::with_var(key, value, f)
    }

    fn with_env_vars<T>(vars: &[(&str, Option<&str>)], f: impl FnOnce() -> T) -> T {
        temp_env::with_vars(vars, f)
    }

    #[test]
    fn test_sanitize_panic_log_file_name_rejects_traversal() {
        assert_eq!(
            sanitize_panic_log_file_name("../panic.log"),
            None,
            "path traversal must be rejected"
        );
        assert_eq!(
            sanitize_panic_log_file_name("panic/alt.log"),
            None,
            "path separators must be rejected"
        );
        assert_eq!(
            sanitize_panic_log_file_name("panic.log"),
            Some("panic.log".to_owned())
        );
    }

    #[test]
    fn test_panic_log_path_from_env_falls_back_on_invalid_file_name() {
        with_env_vars(
            &[
                ("MGS_PANIC_LOG_DIR", Some("tmp/panic-tests")),
                ("MGS_PANIC_LOG_FILE", Some("../evil.log")),
            ],
            || {
                assert_eq!(
                    panic_log_path_from_env(),
                    Path::new("tmp/panic-tests").join(DEFAULT_PANIC_LOG_FILE)
                );
            },
        );
        with_env_vars(
            &[
                ("MGS_PANIC_LOG_DIR", Some("tmp/panic-tests")),
                ("MGS_PANIC_LOG_FILE", Some("server-panic.log")),
            ],
            || {
                assert_eq!(
                    panic_log_path_from_env(),
                    Path::new("tmp/panic-tests").join("server-panic.log")
                );
            },
        );
    }

    #[test]
    fn test_should_rate_limit_panic_log_blocks_immediate_repeats() {
        LAST_PANIC_LOG_UNIX_SECS.store(0, AtomicOrdering::Relaxed);
        with_env_var("MGS_PANIC_LOG_MIN_INTERVAL_SECS", Some("10"), || {
            assert!(!should_rate_limit_panic_log(100));
            assert!(should_rate_limit_panic_log(105));
            assert!(!should_rate_limit_panic_log(111));
        });
    }
}
