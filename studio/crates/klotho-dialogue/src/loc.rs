//! Keyed localization: bounded messages, glossary, pseudo-locale, shaping.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use klotho_ir::Name;

use crate::bible::StoryBible;
use crate::dialogue::DialogueModule;
use crate::error::DialogueError;

/// First-title shipping locales. Replay goldens cover all ten.
pub const SHIPPING_LOCALES: &[&str] = &[
    "en", "es", "fr", "de", "ja", "ko", "zh-Hans", "pt-BR", "ar", "ru",
];

/// Pseudo-locale used to catch overflow before linguistic pass.
pub const PSEUDO_LOCALE: &str = "en-XA";

/// Maximum source characters per duration tick.
pub const MAX_CHARS_PER_TICK: u64 = 8;

fn check_name(field: &str, name: &Name) -> Result<(), DialogueError> {
    if name.as_str().is_empty() {
        Err(DialogueError::Name {
            field: field.to_owned(),
        })
    } else {
        Ok(())
    }
}

/// Locale id (`en`, `zh-Hans`, `en-XA`).
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LocaleId(pub String);

impl LocaleId {
    /// Borrow.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for LocaleId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

/// Bounded gender metadata. Not an executable select.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gender {
    /// Unmarked.
    Neutral,
    /// Feminine agreement.
    Feminine,
    /// Masculine agreement.
    Masculine,
}

impl Gender {
    /// Catalog name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Neutral => "neutral",
            Self::Feminine => "feminine",
            Self::Masculine => "masculine",
        }
    }
}

/// Bounded plural metadata. Not an executable select.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluralForm {
    /// Singular.
    One,
    /// Catch-all.
    Other,
}

impl PluralForm {
    /// Catalog name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::One => "one",
            Self::Other => "other",
        }
    }
}

/// Text shaping / font script class.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapingScript {
    /// Left-to-right alphabetic.
    Ltr,
    /// Right-to-left (Arabic, Hebrew).
    Rtl,
    /// CJK.
    Cjk,
    /// Complex (Indic, Thai).
    Complex,
}

impl ShapingScript {
    /// Catalog name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ltr => "ltr",
            Self::Rtl => "rtl",
            Self::Cjk => "cjk",
            Self::Complex => "complex",
        }
    }
}

/// Per-locale font, shaping, and controller-glyph contract.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FontContract {
    /// Locale this contract covers.
    pub locale: LocaleId,
    /// Primary family.
    pub family: Name,
    /// Shaping script.
    pub shaping: ShapingScript,
    /// Ordered fallbacks.
    pub fallbacks: Vec<Name>,
    /// Controller glyph set id.
    pub controller_glyphs: Name,
}

impl FontContract {
    /// Font family and glyph set must be present for a shipping locale.
    pub(crate) fn check_release(&self) -> Result<(), DialogueError> {
        if self.family.as_str().is_empty() || self.controller_glyphs.as_str().is_empty() {
            return Err(DialogueError::Release {
                token: self.locale.as_str().to_owned(),
                missing: "font".into(),
            });
        }
        Ok(())
    }

    fn check(&self) -> Result<(), DialogueError> {
        if self.locale.0.is_empty() {
            return Err(DialogueError::Loc {
                token: "font".into(),
                reason: "empty locale".into(),
            });
        }
        check_name("font.family", &self.family)?;
        check_name("font.glyphs", &self.controller_glyphs)?;
        for fb in &self.fallbacks {
            check_name("font.fallback", fb)?;
        }
        Ok(())
    }
}

/// ICU-style message: named placeholders only. No executable text.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    /// Localization key (matches a dialogue line or choice key).
    pub key: Name,
    /// Pattern with `{name}` placeholders.
    pub pattern: String,
    /// Optional gender metadata.
    pub gender: Option<Gender>,
    /// Optional plural metadata.
    pub plural: Option<PluralForm>,
    /// Translator context.
    pub context: String,
}

impl Message {
    /// Reject unmatched braces, nesting, and executable-looking text.
    pub fn check(&self) -> Result<(), DialogueError> {
        check_name("message.key", &self.key)?;
        if self.pattern.is_empty() {
            return Err(DialogueError::Loc {
                token: self.key.as_str().to_owned(),
                reason: "empty pattern".into(),
            });
        }
        let lower = self.pattern.to_ascii_lowercase();
        for needle in ["${", "eval(", "<script", "{{", "}}"] {
            if lower.contains(needle) {
                return Err(DialogueError::Loc {
                    token: self.key.as_str().to_owned(),
                    reason: "executable text".into(),
                });
            }
        }
        let mut depth = 0i32;
        let mut name = String::new();
        for c in self.pattern.chars() {
            match c {
                '{' => {
                    if depth != 0 {
                        return Err(DialogueError::Loc {
                            token: self.key.as_str().to_owned(),
                            reason: "nested placeholder".into(),
                        });
                    }
                    depth = 1;
                    name.clear();
                }
                '}' => {
                    if depth != 1 {
                        return Err(DialogueError::Loc {
                            token: self.key.as_str().to_owned(),
                            reason: "unmatched brace".into(),
                        });
                    }
                    if name.is_empty()
                        || !name
                            .chars()
                            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
                    {
                        return Err(DialogueError::Loc {
                            token: self.key.as_str().to_owned(),
                            reason: "illegal placeholder".into(),
                        });
                    }
                    depth = 0;
                }
                _ if depth == 1 => name.push(c),
                _ => {}
            }
        }
        if depth != 0 {
            return Err(DialogueError::Loc {
                token: self.key.as_str().to_owned(),
                reason: "unmatched brace".into(),
            });
        }
        Ok(())
    }
}

/// Named human linguistic approval for a shipping locale.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinguisticApproval {
    /// Approver.
    pub by: Name,
    /// Locale approved.
    pub locale: LocaleId,
}

impl LinguisticApproval {
    fn check(&self) -> Result<(), DialogueError> {
        check_name("loc.approver", &self.by)?;
        if self.locale.0.is_empty() {
            return Err(DialogueError::Loc {
                token: "approval".into(),
                reason: "empty locale".into(),
            });
        }
        Ok(())
    }
}

/// One locale catalog. Keys must cover every dialogue line and choice.
#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocaleCatalog {
    /// Locale id.
    pub locale: LocaleId,
    /// Messages keyed by line/choice key.
    pub strings: BTreeMap<Name, Message>,
    /// Linguistic approval. Required to ship the locale.
    pub approval: Option<LinguisticApproval>,
    /// Font / shaping contract.
    pub font: FontContract,
}

impl LocaleCatalog {
    /// Validate messages, keys, font, and reading speed against `module`.
    pub fn validate(
        &self,
        bible: &StoryBible,
        module: &DialogueModule,
    ) -> Result<(), DialogueError> {
        if self.locale.0.is_empty() {
            return Err(DialogueError::Loc {
                token: "locale".into(),
                reason: "empty id".into(),
            });
        }
        if self.font.locale != self.locale {
            return Err(DialogueError::Loc {
                token: self.locale.as_str().to_owned(),
                reason: "font locale mismatch".into(),
            });
        }
        self.font.check()?;
        if let Some(a) = &self.approval {
            a.check()?;
            if a.locale != self.locale {
                return Err(DialogueError::Loc {
                    token: self.locale.as_str().to_owned(),
                    reason: "approval locale mismatch".into(),
                });
            }
        }
        for (key, msg) in &self.strings {
            if key != &msg.key {
                return Err(DialogueError::Loc {
                    token: key.as_str().to_owned(),
                    reason: "map key != message key".into(),
                });
            }
            msg.check()?;
        }
        for line in &module.lines {
            let Some(msg) = self.strings.get(&line.key) else {
                return Err(DialogueError::Loc {
                    token: line.key.as_str().to_owned(),
                    reason: format!("missing key in {}", self.locale.as_str()),
                });
            };
            reading_speed(line.timing.duration.0, &msg.pattern, &line.key)?;
            for choice in &line.choices {
                if !self.strings.contains_key(&choice.key) {
                    return Err(DialogueError::Loc {
                        token: choice.key.as_str().to_owned(),
                        reason: format!("missing choice key in {}", self.locale.as_str()),
                    });
                }
            }
        }
        for term in &bible.glossary {
            let _ = term;
        }
        Ok(())
    }
}

fn reading_speed(duration: u64, text: &str, key: &Name) -> Result<(), DialogueError> {
    if duration == 0 {
        return Err(DialogueError::Loc {
            token: key.as_str().to_owned(),
            reason: "zero duration".into(),
        });
    }
    let chars = text.chars().count() as u64;
    if chars / duration > MAX_CHARS_PER_TICK {
        return Err(DialogueError::Loc {
            token: key.as_str().to_owned(),
            reason: format!("reading speed {chars}/{duration} exceeds {MAX_CHARS_PER_TICK}"),
        });
    }
    Ok(())
}

/// Expand English source into the overflow-catching pseudo-locale.
#[must_use]
pub fn pseudo_locale(source: &LocaleCatalog) -> LocaleCatalog {
    let mut strings = BTreeMap::new();
    for (key, msg) in &source.strings {
        let expanded = pseudo_text(&msg.pattern);
        strings.insert(
            key.clone(),
            Message {
                key: key.clone(),
                pattern: expanded,
                gender: msg.gender,
                plural: msg.plural,
                context: msg.context.clone(),
            },
        );
    }
    LocaleCatalog {
        locale: LocaleId::from(PSEUDO_LOCALE),
        strings,
        approval: None,
        font: FontContract {
            locale: LocaleId::from(PSEUDO_LOCALE),
            family: Name::from("klotho-sans"),
            shaping: ShapingScript::Ltr,
            fallbacks: source.font.fallbacks.clone(),
            controller_glyphs: source.font.controller_glyphs.clone(),
        },
    }
}

fn pseudo_text(src: &str) -> String {
    let mut out = String::from("[!");
    for c in src.chars() {
        out.push(match c {
            'a' | 'A' => 'À',
            'e' | 'E' => 'É',
            'i' | 'I' => 'Í',
            'o' | 'O' => 'Ø',
            'u' | 'U' => 'Ü',
            other => other,
        });
    }
    let pad = (src.chars().count() / 3).max(1);
    for _ in 0..pad {
        out.push('~');
    }
    out.push_str("!]");
    out
}

/// Default shaping for a shipping locale id.
#[must_use]
pub fn shaping_for(locale: &str) -> ShapingScript {
    match locale {
        "ar" => ShapingScript::Rtl,
        "ja" | "ko" | "zh-Hans" => ShapingScript::Cjk,
        _ => ShapingScript::Ltr,
    }
}
