//! First-title pause / settings / remap menus. Declarative, locale-resolved.

use klotho_input::{BindTable, InputFamily, glyph};
use klotho_ir::{A11yProfile, ContrastMode, Verb};
use klotho_manifest::FocusRole;

use crate::layout::{UiKind, UiNode};

/// Pause sheet used by the capture matrix and focus journeys.
#[must_use]
pub fn pause_menu(locale: &str, profile: &A11yProfile) -> UiNode {
    UiNode::column("pause")
        .with(title("pause_title", t(locale, "pause")))
        .with(UiNode::button("resume", t(locale, "resume")))
        .with(UiNode::button("settings", t(locale, "settings")))
        .with(UiNode::button("accessibility", t(locale, "accessibility")))
        .with(UiNode::button("remap", t(locale, "remap")))
        .with(UiNode::button("captions", t(locale, "captions")))
        .with(UiNode::button("quit", t(locale, "quit")))
        .with(caption_node(locale, profile))
}

/// Accessibility settings sheet.
#[must_use]
pub fn settings_menu(locale: &str, profile: &A11yProfile) -> UiNode {
    let scale = format!("{}%", profile.text_scale_milli / 10);
    let contrast = if profile.contrast == ContrastMode::High {
        t(locale, "high")
    } else {
        t(locale, "default")
    };
    UiNode::column("settings")
        .with(title("settings_title", t(locale, "accessibility")))
        .with(UiNode::slider("text_scale", t(locale, "text_scale"), scale))
        .with(UiNode::toggle(
            "contrast",
            t(locale, "contrast"),
            profile.contrast == ContrastMode::High,
        ))
        .with(UiNode::toggle(
            "subtitles",
            t(locale, "subtitles"),
            profile.subtitles,
        ))
        .with(UiNode::toggle(
            "closed_captions",
            t(locale, "closed_captions"),
            profile.closed_captions,
        ))
        .with(UiNode::toggle(
            "reduce_motion",
            t(locale, "reduce_motion"),
            profile.reduce_motion,
        ))
        .with(UiNode::toggle(
            "reduce_shake",
            t(locale, "reduce_shake"),
            profile.reduce_shake,
        ))
        .with(UiNode::toggle(
            "hold_to_toggle",
            t(locale, "hold_to_toggle"),
            profile.hold_to_toggle,
        ))
        .with(UiNode::toggle(
            "screen_reader",
            t(locale, "screen_reader"),
            profile.screen_reader,
        ))
        .with(UiNode::button("back", t(locale, "back")))
        .with({
            let mut n = caption_node(locale, profile);
            n.value = contrast;
            n
        })
}

/// Remap sheet listing required verbs for `family`.
#[must_use]
pub fn remap_menu(
    locale: &str,
    family: InputFamily,
    table: &BindTable,
    profile: &A11yProfile,
) -> UiNode {
    let mut root = UiNode::column("remap").with(title("remap_title", t(locale, "remap")));
    for verb in family.required_verbs() {
        let id = format!("bind_{verb:?}").to_ascii_lowercase();
        let label = t(locale, verb_key(*verb));
        let g = table
            .binding_for(*verb, family)
            .map(|b| glyph(b.button))
            .unwrap_or("—");
        let mut row = UiNode::button(id, format!("{label} {g}"));
        row.role = FocusRole::Item;
        row.value = g.into();
        root.children.push(row);
    }
    root.children
        .push(UiNode::button("back", t(locale, "back")));
    root.children.push(caption_node(locale, profile));
    root
}

fn title(id: &str, text: String) -> UiNode {
    let mut n = UiNode::label(id, text);
    n.role = FocusRole::Menu;
    n.min_h = 40;
    n
}

fn caption_node(locale: &str, profile: &A11yProfile) -> UiNode {
    let mut n = UiNode::label("caption", t(locale, "caption_sample"));
    n.kind = UiKind::Caption;
    n.role = FocusRole::Caption;
    n.min_h = 24;
    if !profile.subtitles && !profile.closed_captions {
        n.text.clear();
        n.name.clear();
    }
    n
}

fn verb_key(verb: Verb) -> &'static str {
    match verb {
        Verb::Use => "verb_use",
        Verb::Carry => "verb_carry",
        Verb::Drop => "verb_drop",
        Verb::Talk => "verb_talk",
        Verb::Time => "verb_time",
        Verb::Pay => "verb_pay",
        Verb::Fire => "verb_fire",
        _ => "verb_use",
    }
}

fn t(locale: &str, key: &str) -> String {
    let body = match (locale, key) {
        (_, "pause") => match locale {
            "de" => "Pause",
            "ja" => "一時停止",
            "ar" => "إيقاف",
            "fr" => "Pause",
            "ru" => "Пауза",
            "ko" => "일시 정지",
            "zh-Hans" => "暂停",
            "pt-BR" => "Pausa",
            "es" => "Pausa",
            "en-XA" => "[!!! Pause !!!]",
            _ => "Pause",
        },
        (_, "resume") => match locale {
            "de" => "Fortsetzen",
            "ja" => "再開",
            "ar" => "استئناف",
            "fr" => "Reprendre",
            "ru" => "Продолжить",
            "ko" => "계속",
            "zh-Hans" => "继续",
            "pt-BR" => "Continuar",
            "es" => "Reanudar",
            "en-XA" => "[!!! Resume !!!]",
            _ => "Resume",
        },
        (_, "settings") => match locale {
            "de" => "Einstellungen",
            "ja" => "設定",
            "ar" => "إعدادات",
            "fr" => "Paramètres",
            "ru" => "Настройки",
            "ko" => "설정",
            "zh-Hans" => "设置",
            "pt-BR" => "Configurações",
            "es" => "Ajustes",
            "en-XA" => "[!!! Settings !!!]",
            _ => "Settings",
        },
        (_, "accessibility") => match locale {
            "de" => "Barrierefreiheit",
            "ja" => "アクセシビリティ",
            "ar" => "إمكانية الوصول",
            "fr" => "Accessibilité",
            "ru" => "Специальные возможности",
            "ko" => "접근성",
            "zh-Hans" => "辅助功能",
            "pt-BR" => "Acessibilidade",
            "es" => "Accesibilidad",
            "en-XA" => "[!!! Accessibility !!!]",
            _ => "Accessibility",
        },
        (_, "remap") => match locale {
            "de" => "Tastenbelegung",
            "ja" => "割り当て",
            "ar" => "إعادة التعيين",
            "fr" => "Raccourcis",
            "ru" => "Назначение",
            "ko" => "키 설정",
            "zh-Hans" => "按键绑定",
            "pt-BR" => "Atalhos",
            "es" => "Controles",
            "en-XA" => "[!!! Remap !!!]",
            _ => "Remap",
        },
        (_, "captions") => match locale {
            "de" => "Untertitel",
            "ja" => "字幕",
            "ar" => "ترجمة",
            "fr" => "Sous-titres",
            "ru" => "Субтитры",
            "ko" => "자막",
            "zh-Hans" => "字幕",
            "pt-BR" => "Legendas",
            "es" => "Subtítulos",
            "en-XA" => "[!!! Captions !!!]",
            _ => "Captions",
        },
        (_, "quit") => match locale {
            "de" => "Beenden",
            "ja" => "終了",
            "ar" => "خروج",
            "fr" => "Quitter",
            "ru" => "Выход",
            "ko" => "종료",
            "zh-Hans" => "退出",
            "pt-BR" => "Sair",
            "es" => "Salir",
            "en-XA" => "[!!! Quit !!!]",
            _ => "Quit",
        },
        (_, "back") => match locale {
            "de" => "Zurück",
            "ja" => "戻る",
            "ar" => "رجوع",
            "en-XA" => "[!!! Back !!!]",
            _ => "Back",
        },
        (_, "text_scale") => match locale {
            "de" => "Textgröße",
            "ja" => "文字サイズ",
            "ar" => "حجم النص",
            "en-XA" => "[!!! Text size !!!]",
            _ => "Text size",
        },
        (_, "contrast") => match locale {
            "de" => "Kontrast",
            "ja" => "コントラスト",
            "en-XA" => "[!!! Contrast !!!]",
            _ => "Contrast",
        },
        (_, "subtitles") => match locale {
            "de" => "Untertitel",
            "en-XA" => "[!!! Subtitles !!!]",
            _ => "Subtitles",
        },
        (_, "closed_captions") => match locale {
            "de" => "Erweiterte Untertitel",
            "en-XA" => "[!!! Closed captions !!!]",
            _ => "Closed captions",
        },
        (_, "reduce_motion") => match locale {
            "de" => "Bewegung reduzieren",
            "en-XA" => "[!!! Reduce motion !!!]",
            _ => "Reduce motion",
        },
        (_, "reduce_shake") => match locale {
            "de" => "Kamerawackeln reduzieren",
            "en-XA" => "[!!! Reduce shake !!!]",
            _ => "Reduce shake",
        },
        (_, "hold_to_toggle") => match locale {
            "de" => "Halten als Umschalter",
            "en-XA" => "[!!! Hold to toggle !!!]",
            _ => "Hold to toggle",
        },
        (_, "screen_reader") => match locale {
            "de" => "Bildschirmleser",
            "en-XA" => "[!!! Screen reader !!!]",
            _ => "Screen reader",
        },
        (_, "high") => "High",
        (_, "default") => "Default",
        (_, "caption_sample") => match locale {
            "de" => "Mira: Die Platten sind entschlüsselt.",
            "ja" => "ミラ：プレートは解読された。",
            "ar" => "ميرا: تم فك الألواح.",
            "zh-Hans" => "米拉：铭板已解读。",
            "ko" => "미라: 명판이 해독되었다.",
            "ru" => "Мира: Плиты расшифрованы.",
            "fr" => "Mira : Les plaques sont décodées.",
            "es" => "Mira: Las placas están descifradas.",
            "pt-BR" => "Mira: As placas foram decifradas.",
            "en-XA" => "[!!! Mira: The plates are decoded. !!!]",
            _ => "Mira: The plates are decoded.",
        },
        (_, "verb_use") => "Use",
        (_, "verb_carry") => "Carry",
        (_, "verb_drop") => "Drop",
        (_, "verb_talk") => "Talk",
        (_, "verb_time") => "Time",
        (_, "verb_pay") => "Pay",
        (_, "verb_fire") => "Fire",
        _ => key,
    };
    if matches!(
        locale,
        "en" | "de" | "ja" | "ar" | "fr" | "ru" | "ko" | "zh-Hans" | "pt-BR" | "es" | "en-XA"
    ) {
        body.to_owned()
    } else {
        format!("[{locale}] {body}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{aspect_viewport, layout};

    #[test]
    fn pause_menu_has_critical_actions() {
        let p = A11yProfile::first_title();
        let m = pause_menu("en", &p);
        let ids: Vec<_> = m.children.iter().map(|c| c.id.as_str()).collect();
        assert!(ids.contains(&"resume"));
        assert!(ids.contains(&"accessibility"));
        assert!(ids.contains(&"remap"));
        let frame = layout(&m, aspect_viewport(1_920, 1_080), &p, "en");
        frame.check("en").unwrap();
    }

    #[test]
    fn remap_menu_lists_gamepad_verbs() {
        let p = A11yProfile::first_title();
        let table = BindTable::hearth();
        let m = remap_menu("en", InputFamily::Gamepad, &table, &p);
        assert!(m.children.iter().any(|c| c.id == "bind_use"));
        assert!(m.children.iter().any(|c| c.value == "A"));
    }
}
