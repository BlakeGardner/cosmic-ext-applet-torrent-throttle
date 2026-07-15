// SPDX-License-Identifier: GPL-3.0

mod app;
mod applet;
mod config;
mod engine;
mod i18n;
mod monitor;
mod qbit;

fn main() -> cosmic::iced::Result {
    // Get the system's preferred languages.
    let requested_languages = i18n_embed::DesktopLanguageRequester::requested_languages();

    // Enable localizations to be applied.
    i18n::init(&requested_languages);

    // `--applet` runs the panel applet; no arguments runs the settings window.
    if std::env::args().any(|arg| arg == "--applet") {
        return applet::run();
    }

    // The panel icon should be available whenever the app is used, even
    // after the applet was quit; the panel won't respawn it on its own.
    std::thread::spawn(applet::ensure_applet_running);

    // Settings for configuring the application window and iced runtime.
    let settings = cosmic::app::Settings::default().size_limits(
        cosmic::iced::Limits::NONE
            .min_width(400.0)
            .min_height(300.0),
    );

    // Starts the application's event loop.
    cosmic::app::run::<app::AppModel>(settings, ())
}
