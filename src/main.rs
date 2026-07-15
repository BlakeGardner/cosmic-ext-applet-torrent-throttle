// SPDX-License-Identifier: GPL-3.0

mod app;
mod config;
mod i18n;
mod monitor;
mod qbit;
mod tray;

fn main() -> cosmic::iced::Result {
    // Get the system's preferred languages.
    let requested_languages = i18n_embed::DesktopLanguageRequester::requested_languages();

    // Enable localizations to be applied.
    i18n::init(&requested_languages);

    // Settings for configuring the application window and iced runtime.
    // `exit_on_close(false)` keeps the app running in the system tray when
    // the main window is closed.
    let settings = cosmic::app::Settings::default()
        .exit_on_close(false)
        .size(cosmic::iced::Size::new(700.0, 600.0))
        .size_limits(
            cosmic::iced::Limits::NONE
                .min_width(400.0)
                .min_height(300.0),
        );

    // Starts the application's event loop.
    cosmic::app::run::<app::AppModel>(settings, ())
}
