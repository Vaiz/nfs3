use tracing::subscriber::set_global_default;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_subscriber::fmt::layer;
use tracing_subscriber::layer::SubscriberExt;

static STDOUT_LOGGER: std::sync::OnceLock<NonBlocking> = std::sync::OnceLock::new();
static FILE_LOGGER: std::sync::OnceLock<NonBlocking> = std::sync::OnceLock::new();

pub fn init_logging(
    log_level: &str,
    log_file: Option<&str>,
    enable_stdout: bool,
) -> Vec<WorkerGuard> {
    let log_level = match log_level.to_lowercase().as_str() {
        "error" => tracing::Level::ERROR,
        "warn" => tracing::Level::WARN,
        "info" => tracing::Level::INFO,
        "debug" => tracing::Level::DEBUG,
        "trace" => tracing::Level::TRACE,
        _ => panic!("invalid log level: {log_level}"),
    };

    let level_filter = tracing_subscriber::filter::LevelFilter::from_level(log_level);
    let subscriber = tracing_subscriber::Registry::default().with(level_filter);

    match (enable_stdout, log_file) {
        (false, None) => {
            // No logging
            vec![]
        }
        (true, None) => {
            // Console logging
            let stdout_guard = init_stdout_logger();
            let subscriber = subscriber.with(layer().with_writer(stdout_logger));
            set_global_default(subscriber).expect("failed to set global subscriber");
            vec![stdout_guard]
        }
        (false, Some(log_file)) => {
            // File logging only
            let file_guard = init_file_logger(log_file);
            let subscriber = subscriber.with(layer().with_writer(file_logger));
            set_global_default(subscriber).expect("failed to set global subscriber");
            vec![file_guard]
        }
        (true, Some(log_file)) => {
            // both console and file logging
            let stdout_guard = init_stdout_logger();
            let file_guard = init_file_logger(log_file);
            let subscriber = subscriber
                .with(layer().with_writer(stdout_logger))
                .with(layer().with_writer(file_logger));
            set_global_default(subscriber).expect("failed to set global subscriber");
            vec![stdout_guard, file_guard]
        }
    }
}

fn stdout_logger() -> impl std::io::Write {
    STDOUT_LOGGER
        .get()
        .expect("stdout logger not initialzied")
        .clone()
}

fn file_logger() -> impl std::io::Write {
    FILE_LOGGER
        .get()
        .expect("file logger not initialized")
        .clone()
}

fn init_stdout_logger() -> WorkerGuard {
    let (non_blocking, guard) = tracing_appender::non_blocking(std::io::stdout());
    STDOUT_LOGGER
        .set(non_blocking)
        .expect("stdout logger already initialized");

    guard
}

fn init_file_logger(log_file: &str) -> WorkerGuard {
    let path = std::path::Path::new(log_file);
    let file_appender = tracing_appender::rolling::never(
        path.parent().unwrap_or_else(|| std::path::Path::new(".")),
        path.file_name().expect("log file name is empty"),
    );
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    FILE_LOGGER
        .set(non_blocking)
        .expect("file logger already initialized");

    guard
}
