//! Provider selection settings page.

use super::super::OxiApp;
use super::provider_catalog::{PROVIDER_GROUPS, provider_blurb};
use crate::settings::LlmProviderKind;
use crate::ui::chrome::{setting_row, settings_section, settings_section_title};
use crate::ui::select_menu::{SelectMenu, SelectOption};
use eframe::egui::{self, Ui};

impl OxiApp {
    pub(super) fn render_settings_providers_panel(&mut self, ui: &mut Ui) {
        settings_section_title(
            ui,
            "Models",
            Some(
                "Where your models run. The active provider is used for new chats; each chat can switch in the composer.",
            ),
        );

        let active = self.conv.settings.active_provider;
        let configured = self.cached_configured_providers(ui.ctx());
        let provider = self.conv.settings_page.provider_tab;
        settings_section(ui, "Provider", None, |ui| {
            let kinds: Vec<LlmProviderKind> = PROVIDER_GROUPS
                .iter()
                .flat_map(|(_, providers)| providers.iter().copied())
                .collect();
            let picked = setting_row(ui, provider.label(), Some(provider_blurb(provider)), |ui| {
                if active == provider {
                    super::layout::active_pill(ui, "Active");
                } else if crate::ui::chrome::primary_button(ui, "Make active")
                    .on_hover_text("Use this provider for new chats")
                    .clicked()
                {
                    self.conv.settings.active_provider = provider;
                }
                ui.add_space(8.0);
                SelectMenu::new("settings_provider", "Change provider")
                    .framed(170.0)
                    .show(ui, || {
                        let mut options = Vec::new();
                        for (group, providers) in PROVIDER_GROUPS {
                            for &kind in *providers {
                                let mut option = SelectOption::new(
                                    dropdown_label(kind, active, &configured),
                                    kind == provider,
                                )
                                .group(Some((*group).to_owned()));
                                if let Some(hint) =
                                    chip_hint(kind == active, chip_state(kind, &configured))
                                {
                                    option = option.detail(hint);
                                }
                                options.push(option);
                            }
                        }
                        options
                    })
            });
            if let Some(kind) = picked.and_then(|i| kinds.get(i)) {
                self.conv.settings_page.provider_tab = *kind;
            }
        });

        if provider == LlmProviderKind::Router {
            self.render_router_settings(ui);
            return;
        }
        self.render_provider_config(ui, provider);

        if provider == LlmProviderKind::GptCodex {
            self.render_codex_oauth_section(ui);
        }
    }
}

impl OxiApp {
    /// Providers with usable credentials, refreshed at most every 2 s: the check reads the
    /// OAuth store (keychain-backed), far too slow to repeat every frame.
    pub(super) fn cached_configured_providers(&self, ctx: &egui::Context) -> Vec<LlmProviderKind> {
        let id = egui::Id::new("settings_configured_providers");
        let now = ctx.input(|i| i.time);
        if let Some((at, kinds)) = ctx.data(|d| d.get_temp::<(f64, Vec<LlmProviderKind>)>(id))
            && now - at < 2.0
        {
            return kinds;
        }
        let oauth = crate::oauth::load_oauth_store();
        let kinds = self.conv.settings.configured_provider_kinds(&oauth);
        ctx.data_mut(|d| d.insert_temp(id, (now, kinds.clone())));
        kinds
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChipState {
    /// Local servers and agents that handle their own sign-in: nothing to show.
    Neutral,
    /// A hosted API with a key / OAuth token available.
    Ready,
    /// A hosted API with no credentials yet.
    NeedsKey,
}

fn chip_state(provider: LlmProviderKind, configured: &[LlmProviderKind]) -> ChipState {
    let hosted = PROVIDER_GROUPS
        .iter()
        .find(|(label, _)| *label == "Hosted APIs")
        .is_some_and(|(_, kinds)| kinds.contains(&provider));
    // Azure has no env fallback to detect, so it always reports configured; don't claim it.
    if !hosted || provider == LlmProviderKind::AzureOpenAi {
        ChipState::Neutral
    } else if configured.contains(&provider) {
        ChipState::Ready
    } else {
        ChipState::NeedsKey
    }
}

fn dropdown_label(
    provider: LlmProviderKind,
    active: LlmProviderKind,
    configured: &[LlmProviderKind],
) -> String {
    let mut label = provider.label().to_string();
    if provider == active {
        label.push_str("  · active");
    } else if chip_state(provider, configured) == ChipState::NeedsKey {
        label.push_str("  · no key");
    }
    label
}

/// Hover text for a provider in the picker.
fn chip_hint(active: bool, state: ChipState) -> Option<&'static str> {
    match (active, state) {
        (true, _) => Some("Active for new chats"),
        (false, ChipState::Ready) => Some("Credentials found"),
        (false, ChipState::NeedsKey) => Some("No API key yet"),
        (false, ChipState::Neutral) => None,
    }
}
