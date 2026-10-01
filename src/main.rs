mod acp;
mod app;
mod appearance;
mod config;
mod diff_view;
mod diff_watch;
mod discovery;
mod file_search;
mod folder_search;
mod git_diff;
mod git_sync;
mod images;
mod mobile;
mod panes;
mod persistence;
mod remote;
mod session;
#[cfg(unix)]
mod shell_env;
mod theme;
mod theming;

fn main() {
    #[cfg(target_os = "linux")]
    match std::env::args().nth(1).as_deref() {
        Some("--gtk-theme-probe") => theming::exit_after_probe(theming::Backend::Gtk),
        Some("--qt-theme-probe") => theming::exit_after_probe(theming::Backend::Qt),
        _ => {}
    }

    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--version")
    {
        println!("agentaps {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    #[cfg(unix)]
    shell_env::import_login_path();
    app::run();
}
