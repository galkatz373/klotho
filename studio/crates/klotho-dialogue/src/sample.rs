//! Canonical KAI-15 fixture: observatory conversation, quests, 10 locales.

use std::collections::BTreeMap;

use klotho_core::Tick;
use klotho_ir::{AnchorId, Name};
use klotho_prove::{ReleaseRights, RightsRoute, blob_id_of, hash_bytes};

use crate::bible::{
    ApprovedException, CharacterFact, GlossaryEntry, LocationFact, Presence, RatingLimits,
    SecretFact, StoryBible, ThemeFact, TimelineBeat, UnresolvedQuestion, VoiceProfile,
};
use crate::dialogue::{
    ClosedCaption, DialogueChoice, DialogueCond, DialogueLine, DialogueModule, LineTiming,
    VoBinding,
};
use crate::loc::{
    FontContract, Gender, LinguisticApproval, LocaleCatalog, LocaleId, Message, PluralForm,
    SHIPPING_LOCALES, shaping_for,
};
use crate::project::NarrativeProject;
use crate::quest::{QuestExclusion, QuestGraph, QuestNode, ReentryKind};

fn n(s: &str) -> Name {
    Name::from(s)
}

fn aid(token: &str) -> AnchorId {
    AnchorId::derive(b"kai-15", token.as_bytes())
}

fn rights() -> ReleaseRights {
    let h = |label: &str| hash_bytes(label.as_bytes());
    ReleaseRights {
        route: RightsRoute::Commissioned,
        origin: h("origin"),
        terms: h("terms"),
        ownership: h("ownership"),
        indemnity: h("indemnity"),
        source_permission: h("source"),
        consent: h("consent"),
        restrictions: h("restrictions"),
        approved_by: "legal.owner".into(),
        approval: h("approval"),
    }
}

fn voice() -> VoiceProfile {
    VoiceProfile {
        register: n("dry"),
        formality: n("you"),
        forbid: "no slang".into(),
    }
}

fn font(locale: &str) -> FontContract {
    FontContract {
        locale: LocaleId::from(locale),
        family: n("klotho-sans"),
        shaping: shaping_for(locale),
        fallbacks: vec![n("klotho-fallback")],
        controller_glyphs: n("xbox"),
    }
}

fn msg(key: &str, pattern: &str) -> Message {
    Message {
        key: n(key),
        pattern: pattern.to_owned(),
        gender: Some(Gender::Neutral),
        plural: Some(PluralForm::Other),
        context: "observatory".into(),
    }
}

fn catalog(
    locale: &str,
    greet: &str,
    idle: &str,
    offer: &str,
    yes: &str,
    no: &str,
) -> LocaleCatalog {
    let mut strings = BTreeMap::new();
    strings.insert(n("mira.greet"), msg("mira.greet", greet));
    strings.insert(n("mira.idle"), msg("mira.idle", idle));
    strings.insert(n("mira.offer"), msg("mira.offer", offer));
    strings.insert(n("mira.yes"), msg("mira.yes", yes));
    strings.insert(n("mira.no"), msg("mira.no", no));
    strings.insert(n("choice.yes"), msg("choice.yes", yes));
    strings.insert(n("choice.no"), msg("choice.no", no));
    LocaleCatalog {
        locale: LocaleId::from(locale),
        strings,
        approval: Some(LinguisticApproval {
            by: n("loc.lead"),
            locale: LocaleId::from(locale),
        }),
        font: font(locale),
    }
}

/// Observatory increment: decode plates, then Mira offers the door.
#[must_use]
pub fn observatory() -> NarrativeProject {
    let bible = StoryBible {
        anchor: aid("bible"),
        version: 1,
        characters: vec![CharacterFact {
            anchor: aid("char.mira"),
            name: n("mira"),
            voice: voice(),
            facts: vec![n("scholar")],
        }],
        timeline: vec![
            TimelineBeat {
                anchor: aid("beat.arrival"),
                id: n("arrival"),
                order: 1,
                location: n("keep"),
            },
            TimelineBeat {
                anchor: aid("beat.decode"),
                id: n("decode"),
                order: 2,
                location: n("keep"),
            },
            TimelineBeat {
                anchor: aid("beat.offer"),
                id: n("offer"),
                order: 3,
                location: n("keep"),
            },
        ],
        locations: vec![LocationFact {
            anchor: aid("loc.keep"),
            name: n("keep"),
        }],
        glossary: vec![GlossaryEntry {
            anchor: aid("gl.plate"),
            term: n("plate"),
            gloss: "star plate".into(),
        }],
        secrets: vec![SecretFact {
            anchor: aid("secret.plates"),
            knows: n("plates_decoded"),
            owner: n("mira"),
            reveal_at: n("decode"),
        }],
        themes: vec![ThemeFact {
            anchor: aid("theme.sky"),
            id: n("sky"),
        }],
        rating: RatingLimits {
            board: n("esrb_t"),
            forbid: vec![n("gore")],
        },
        unresolved: vec![UnresolvedQuestion {
            anchor: aid("q.who"),
            id: n("who_built_plates"),
        }],
        exceptions: vec![ApprovedException {
            anchor: aid("ex.1"),
            id: n("mira_repeats_plate"),
            owner: n("narrative.lead"),
        }],
        presence: vec![
            Presence {
                character: n("mira"),
                beat: n("arrival"),
                location: n("keep"),
            },
            Presence {
                character: n("mira"),
                beat: n("decode"),
                location: n("keep"),
            },
            Presence {
                character: n("mira"),
                beat: n("offer"),
                location: n("keep"),
            },
        ],
    };
    let quests = QuestGraph {
        anchor: aid("quests"),
        quests: vec![
            QuestNode {
                anchor: aid("quest.decode"),
                id: n("decode_plates"),
                prerequisites: Vec::new(),
                grants: vec![n("plates_decoded")],
                failure: None,
                cancel: None,
                reentry: ReentryKind::Checkpoint,
                critical: true,
                available_at_start: true,
                escape: false,
                ending: false,
            },
            QuestNode {
                anchor: aid("quest.open"),
                id: n("open_observatory"),
                prerequisites: vec![n("decode_plates")],
                grants: vec![n("observatory_open")],
                failure: None,
                cancel: None,
                reentry: ReentryKind::Never,
                critical: true,
                available_at_start: false,
                escape: false,
                ending: true,
            },
        ],
        exclusions: Vec::<QuestExclusion>::new(),
    };
    let vo = Some(VoBinding {
        blob: blob_id_of(b"mira-vo"),
        rights: rights(),
    });
    let timing = LineTiming {
        start: Tick(0),
        duration: Tick(12),
    };
    let dialogue = DialogueModule {
        anchor: aid("dlg"),
        id: n("mira_observatory"),
        entry: n("mira.greet"),
        lines: vec![
            DialogueLine {
                anchor: aid("line.greet"),
                key: n("mira.greet"),
                speaker: n("mira"),
                beat: n("arrival"),
                condition: DialogueCond::Always,
                grants: Vec::new(),
                choices: Vec::new(),
                timing,
                performance: "measured".into(),
                cc: ClosedCaption {
                    speaker: n("mira"),
                    body: "The plates still wait.".into(),
                    sdh: true,
                },
                source_text: "The plates still wait.".into(),
                vo: vo.clone(),
                vo_required: true,
                next: Some(n("mira.offer")),
            },
            DialogueLine {
                anchor: aid("line.idle"),
                key: n("mira.idle"),
                speaker: n("mira"),
                beat: n("arrival"),
                condition: DialogueCond::Always,
                grants: Vec::new(),
                choices: Vec::new(),
                timing,
                performance: "quiet".into(),
                cc: ClosedCaption {
                    speaker: n("mira"),
                    body: "Come back when the plates are read.".into(),
                    sdh: true,
                },
                source_text: "Come back when the plates are read.".into(),
                vo: vo.clone(),
                vo_required: true,
                next: None,
            },
            DialogueLine {
                anchor: aid("line.offer"),
                key: n("mira.offer"),
                speaker: n("mira"),
                beat: n("offer"),
                condition: DialogueCond::Knows(n("plates_decoded")),
                grants: Vec::new(),
                choices: vec![
                    DialogueChoice {
                        anchor: aid("choice.yes"),
                        id: n("yes"),
                        key: n("choice.yes"),
                        condition: DialogueCond::Always,
                        grants: vec![n("observatory_open")],
                        next: n("mira.yes"),
                    },
                    DialogueChoice {
                        anchor: aid("choice.no"),
                        id: n("no"),
                        key: n("choice.no"),
                        condition: DialogueCond::Always,
                        grants: Vec::new(),
                        next: n("mira.no"),
                    },
                ],
                timing,
                performance: "hopeful".into(),
                cc: ClosedCaption {
                    speaker: n("mira"),
                    body: "The observatory will open. Will you come?".into(),
                    sdh: true,
                },
                source_text: "The observatory will open. Will you come?".into(),
                vo: vo.clone(),
                vo_required: true,
                next: None,
            },
            DialogueLine {
                anchor: aid("line.yes"),
                key: n("mira.yes"),
                speaker: n("mira"),
                beat: n("offer"),
                condition: DialogueCond::Always,
                grants: Vec::new(),
                choices: Vec::new(),
                timing,
                performance: "warm".into(),
                cc: ClosedCaption {
                    speaker: n("mira"),
                    body: "Then we climb.".into(),
                    sdh: true,
                },
                source_text: "Then we climb.".into(),
                vo: vo.clone(),
                vo_required: true,
                next: None,
            },
            DialogueLine {
                anchor: aid("line.no"),
                key: n("mira.no"),
                speaker: n("mira"),
                beat: n("offer"),
                condition: DialogueCond::Always,
                grants: Vec::new(),
                choices: Vec::new(),
                timing,
                performance: "flat".into(),
                cc: ClosedCaption {
                    speaker: n("mira"),
                    body: "The sky can wait.".into(),
                    sdh: true,
                },
                source_text: "The sky can wait.".into(),
                vo,
                vo_required: true,
                next: None,
            },
        ],
    };
    let locales = SHIPPING_LOCALES
        .iter()
        .map(|id| match *id {
            "en" => catalog(
                id,
                "The plates still wait.",
                "Come back when the plates are read.",
                "The observatory will open. Will you come?",
                "Yes",
                "No",
            ),
            "es" => catalog(
                id,
                "Las placas siguen esperando.",
                "Vuelve cuando las placas esten leidas.",
                "El observatorio se abrira. Vienes?",
                "Si",
                "No",
            ),
            "fr" => catalog(
                id,
                "Les plaques attendent encore.",
                "Reviens quand les plaques seront lues.",
                "L observatoire s ouvrira. Tu viens?",
                "Oui",
                "Non",
            ),
            "de" => catalog(
                id,
                "Die Platten warten noch.",
                "Komm wieder, wenn die Platten gelesen sind.",
                "Das Observatorium oeffnet sich. Kommst du?",
                "Ja",
                "Nein",
            ),
            "ja" => catalog(
                id,
                "プレートはまだ待っている。",
                "プレートを読んでから戻って。",
                "観測所が開く。来る？",
                "はい",
                "いいえ",
            ),
            "ko" => catalog(
                id,
                "아직 판이 기다리고 있어.",
                "판을 읽고 다시 와.",
                "관측소가 열릴 거야. 올래?",
                "예",
                "아니오",
            ),
            "zh-Hans" => catalog(
                id,
                "星盘还在等。",
                "读完星盘再回来。",
                "观测台要开了。你来吗？",
                "是",
                "否",
            ),
            "pt-BR" => catalog(
                id,
                "As placas ainda esperam.",
                "Volte quando as placas forem lidas.",
                "O observatorio vai abrir. Voce vem?",
                "Sim",
                "Nao",
            ),
            "ar" => catalog(
                id,
                "اللوحات ما زالت تنتظر.",
                "عد عندما تُقرأ اللوحات.",
                "سيفتح المرصد. هل تأتي؟",
                "نعم",
                "لا",
            ),
            "ru" => catalog(
                id,
                "Пластины всё ещё ждут.",
                "Вернись, когда пластины будут прочитаны.",
                "Обсерватория откроется. Ты придёшь?",
                "Да",
                "Нет",
            ),
            _ => catalog(id, "x", "x", "x", "x", "x"),
        })
        .collect();
    NarrativeProject {
        anchor: aid("project"),
        id: n("observatory"),
        bible,
        quests,
        dialogue,
        locales,
    }
}
