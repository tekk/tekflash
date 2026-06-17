use clap::Parser;
use color_eyre::Result;

mod cli;
mod help;
mod json_mode;
mod tui;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    color_eyre::install()?;
    install_panic_terminal_guard();

    let args = cli::Args::parse();

    // Initialize tracing to a daily-rotated file under the audit-log dir. STDERR is left
    // for human-facing messages (privilege errors, panics).
    let _guard = init_logging();

    // Honor --check before forcing elevation, so non-root scripted preflight works.
    if args.check {
        return run_check();
    }

    // Privilege gate. Subcommands that only print info (`list --no-tui --json`) still
    // benefit from a permission probe, but the TUI absolutely needs it. We always require
    // elevation; `list` works fine as root and a non-elevated `list` would just lie.
    tekflash_core::privilege::require_elevation();

    match args.command {
        Some(cli::Command::List(opts)) => cli::run_list(opts, args.global).await,
        Some(cli::Command::Flash(opts)) => cli::run_flash(opts, args.global).await,
        Some(cli::Command::Backup(opts)) => cli::run_backup(opts, args.global).await,
        Some(cli::Command::Archive(opts)) => cli::run_archive(opts, args.global).await,
        Some(cli::Command::Restore(opts)) => cli::run_restore(opts, args.global).await,
        Some(cli::Command::Verify(opts)) => cli::run_verify(opts, args.global).await,
        Some(cli::Command::VerifyQueue) => cli::run_verify_queue(args.global).await,
        Some(cli::Command::Keygen(opts)) => cli::run_keygen(opts).await,
        None if args.global.no_tui => {
            eprintln!("--no-tui requires a subcommand. Try `tekflash --help`.");
            std::process::exit(2);
        }
        None => tui::run(args.global).await,
    }
}

fn run_check() -> Result<()> {
    let status = tekflash_core::privilege::check();
    println!("elevated: {}", status.elevated);
    println!();
    println!("{}", status.advice);
    std::process::exit(if status.elevated { 0 } else { 1 });
}

fn init_logging() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let dir = dirs::data_local_dir()
        .map(|p| p.join("tekflash").join("logs"))
        .unwrap_or_else(|| std::path::PathBuf::from(".tekflash-logs"));
    if std::fs::create_dir_all(&dir).is_err() {
        return None;
    }
    let appender = tracing_appender::rolling::daily(dir, "tekflash.log");
    let (nb, guard) = tracing_appender::non_blocking(appender);
    let _ = tracing_subscriber::fmt()
        .with_writer(nb)
        .with_ansi(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();
    Some(guard)
}

/// Restore the terminal if a panic leaves us inside the alt screen. Without this, a
/// crash leaves a broken shell that can't echo or break out of raw mode.
fn install_panic_terminal_guard() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = crossterm::terminal::disable_raw_mode();
        let mut stdout = std::io::stdout();
        let _ = crossterm::execute!(
            stdout,
            crossterm::terminal::LeaveAlternateScreen,
            crossterm::cursor::Show
        );
        prev(info);
    }));
}
