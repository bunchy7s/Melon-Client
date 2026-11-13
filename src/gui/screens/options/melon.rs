//! Melon's own settings, the ones vanilla has no screen for: how frames are

use crate::cycle::Cycle;
use crate::gui::screen::{ConfigChange, ScreenContext};
use crate::gui::screens::options::value_label;
use crate::gui::widgets::options_list::{OptionEntry, OptionsSubScreen, Section};
use crate::i18n::Lang;
use crate::settings::options::Config;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MelonOption {
    PresentMode,
}

static SECTIONS: [Section<MelonOption>; 1] = [Section {
    header: Some("melon.options.presentation.header"),
    options: &[MelonOption::PresentMode],
}];

impl OptionEntry for MelonOption {
    fn press(self, _ctx: &ScreenContext) -> ConfigChange {
        Box::new(move |config| match self {
            Self::PresentMode => config.present_mode = config.present_mode.next(),
        })
    }

    fn label(self, config: &Config, lang: &Lang) -> String {
        match self {
            Self::PresentMode => value_label(
                lang,
                "melon.options.presentMode",
                lang.get(config.present_mode.key()),
            ),
        }
    }

    fn tooltip(self, config: &Config) -> Option<&'static str> {
        match self {
            Self::PresentMode => Some(config.present_mode.tooltip_key()),
        }
    }
}

/// Melon's settings screen.
pub fn melon_settings_screen(ctx: &ScreenContext) -> OptionsSubScreen<MelonOption> {
    OptionsSubScreen::new("melon.options.title", &SECTIONS, ctx)
}
