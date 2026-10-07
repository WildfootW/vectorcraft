//! The command palette: fuzzy search over every command and tool.

use serde_json::json;

use crate::theme::Tokens;
use crate::{VectorcraftApp, menus};

/// Everything the palette searches: (label, command id or `tool:<id>`, shortcut).
pub fn items() -> Vec<(String, String, String)> {
    let mut items = vec![];
    // Aliases kept for older scripts run a command that is listed already.
    for c in vectorcraft_engine::command_specs().iter().filter(|c| !vectorcraft_engine::cmd::is_alias(c.id)) {
        let path = c.menu.join(" › ");
        let label = if path.is_empty() { c.label.to_string() } else { format!("{path} › {}", c.label) };
        items.push((label, c.id.to_string(), menus::shortcut_of(c.id).unwrap_or("").to_string()));
    }
    for c in menus::UI_COMMANDS {
        items.push((c.1.to_string(), c.0.to_string(), menus::shortcut_of(c.0).unwrap_or("").to_string()));
    }
    for tool in vectorcraft_tools::catalog::all_tools() {
        items.push((tool.label.to_string(), format!("tool:{}", tool.id), crate::shortcut_editor::tool_shortcut(tool.id).unwrap_or("").to_string()));
    }
    items
}

/// A palette label in a language: each `›`-separated menu segment and the command label
/// translate on their own.
fn shown_label(lang: crate::i18n::Lang, label: &str) -> String {
    label.split(" › ").map(|s| crate::i18n::tr(lang, s)).collect::<Vec<_>>().join(" › ")
}

/// One searchable item: `items()`'s (label, id, shortcut) plus the shown label and the lowercased
/// texts the query is matched against.
struct Entry {
    id: String,
    shortcut: String,
    shown: String,
    label_lower: String,
    shown_lower: String,
    id_lower: String,
}

/// The items with their shown labels, built once per UI language and shortcut generation (not
/// per keystroke: translating and lowercasing every label each frame is what a typed query would
/// otherwise cost).
fn entries() -> std::sync::Arc<Vec<Entry>> {
    entries_for(crate::i18n::current(), crate::shortcut_editor::GENERATION.load(std::sync::atomic::Ordering::Relaxed))
}

fn entries_for(lang: crate::i18n::Lang, generation: u64) -> std::sync::Arc<Vec<Entry>> {
    use std::sync::{Arc, Mutex};
    /// (language code, shortcut generation, entries)
    type Cached = (&'static str, u64, Arc<Vec<Entry>>);
    static CACHE: Mutex<Option<Cached>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((l, g, entries)) = cache.as_ref()
        && *l == lang.code()
        && *g == generation
    {
        return Arc::clone(entries);
    }
    let entries: Arc<Vec<Entry>> = Arc::new(
        items()
            .into_iter()
            .map(|(label, id, shortcut)| {
                let shown = shown_label(lang, &label);
                Entry { label_lower: label.to_lowercase(), shown_lower: shown.to_lowercase(), id_lower: id.to_lowercase(), id, shortcut, shown }
            })
            .collect(),
    );
    *cache = Some((lang.code(), generation, Arc::clone(&entries)));
    entries
}

pub fn show(app: &mut VectorcraftApp, ctx: &egui::Context) {
    if !app.ui.palette_open {
        return;
    }
    let t = Tokens::get(ctx);
    let q = app.ui.palette_query.to_lowercase();
    let entries = entries();
    let matches: Vec<&Entry> = entries
        .iter()
        .filter(|e| {
            // Match the English text (what agents document), the shown text and the id.
            q.is_empty() || q.split_whitespace().all(|w| e.label_lower.contains(w) || e.shown_lower.contains(w) || e.id_lower.contains(w))
        })
        .take(14)
        .collect();
    let mut run: Option<String> = None;
    egui::Area::new(egui::Id::new("palette")).order(egui::Order::Foreground).anchor(egui::Align2::CENTER_TOP, [0.0, 90.0]).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).fill(t.panel).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
            ui.set_width(520.0);
            let r = ui.add(egui::TextEdit::singleline(&mut app.ui.palette_query).hint_text(tl!("Search commands and tools…")).desired_width(500.0));
            r.request_focus();
            ui.add_space(6.0);
            for (i, e) in matches.iter().enumerate() {
                let resp = ui.add(
                    egui::Button::new(&e.shown).shortcut_text(menus::pretty_shortcut(&e.shortcut)).min_size(egui::vec2(500.0, 24.0)).selected(i == 0),
                );
                if resp.clicked() {
                    run = Some(e.id.clone());
                }
            }
            if ui.input(|i| i.key_pressed(egui::Key::Enter))
                && let Some(first) = matches.first()
            {
                run = Some(first.id.clone());
            }
        });
    });
    if let Some(id) = run {
        app.ui.palette_open = false;
        if let Some(tool) = id.strip_prefix("tool:") {
            app.select_tool(tool);
        } else {
            menus::invoke(app, &id, json!({}));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Lang;

    /// The entries are built once per language and shortcut generation, and the shown labels are
    /// in that language while the English text and the id stay searchable.
    #[test]
    fn entries_are_cached_per_language_and_generation() {
        let en = entries_for(Lang::EN, 7);
        assert!(std::sync::Arc::ptr_eq(&en, &entries_for(Lang::EN, 7)), "same language and generation: cached");
        let save_as = en.iter().find(|e| e.id == "file.saveAs").unwrap();
        assert_eq!(save_as.shown, "File › Save As…");
        let zh = Lang::from_code("zh-hant").unwrap();
        let zh_entries = entries_for(zh, 7);
        assert!(!std::sync::Arc::ptr_eq(&en, &zh_entries), "another language: rebuilt");
        let save_as = zh_entries.iter().find(|e| e.id == "file.saveAs").unwrap();
        assert_eq!(save_as.shown, format!("{} › {}", crate::i18n::tr(zh, "File"), crate::i18n::tr(zh, "Save As…")));
        assert_ne!(save_as.shown, "File › Save As…");
        // A typed query matches the English text, the shown text and the id.
        assert!(
            save_as.label_lower.contains("save as")
                && save_as.shown_lower.contains(&crate::i18n::tr(zh, "Save As…").to_lowercase())
                && save_as.id_lower.contains("saveas")
        );
        let again = entries_for(zh, 8);
        assert!(!std::sync::Arc::ptr_eq(&zh_entries, &again), "shortcuts edited: rebuilt");
    }
}
