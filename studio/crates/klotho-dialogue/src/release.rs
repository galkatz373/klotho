//! Release gates: missing key, VO, CC, font, approval, or rights fail closed.

use klotho_prove::ReleaseRights;

use crate::dialogue::DialogueModule;
use crate::error::DialogueError;
use crate::loc::{LocaleCatalog, SHIPPING_LOCALES};

/// Shipping locales that must be complete.
pub fn required_locales(catalogs: &[LocaleCatalog]) -> Result<Vec<&LocaleCatalog>, DialogueError> {
    let mut out = Vec::new();
    for id in SHIPPING_LOCALES {
        let Some(cat) = catalogs.iter().find(|c| c.locale.as_str() == *id) else {
            return Err(DialogueError::Release {
                token: (*id).to_owned(),
                missing: "locale".into(),
            });
        };
        out.push(cat);
    }
    Ok(out)
}

/// Fail closed when a shipping locale is missing a key, VO, CC, font, approval, or rights.
pub fn release_check(
    module: &DialogueModule,
    catalogs: &[LocaleCatalog],
) -> Result<(), DialogueError> {
    let shipping = required_locales(catalogs)?;
    for cat in shipping {
        if cat.approval.is_none() {
            return Err(DialogueError::Release {
                token: cat.locale.as_str().to_owned(),
                missing: "linguistic_approval".into(),
            });
        }
        cat.font.check_release()?;
        for line in &module.lines {
            if !cat.strings.contains_key(&line.key) {
                return Err(DialogueError::Release {
                    token: format!("{}:{}", cat.locale.as_str(), line.key),
                    missing: "key".into(),
                });
            }
            if line.cc.body.is_empty() {
                return Err(DialogueError::Release {
                    token: line.key.as_str().to_owned(),
                    missing: "cc".into(),
                });
            }
            for choice in &line.choices {
                if !cat.strings.contains_key(&choice.key) {
                    return Err(DialogueError::Release {
                        token: format!("{}:{}", cat.locale.as_str(), choice.key),
                        missing: "key".into(),
                    });
                }
            }
        }
    }
    for line in &module.lines {
        if line.vo_required {
            let Some(vo) = &line.vo else {
                return Err(DialogueError::Release {
                    token: line.key.as_str().to_owned(),
                    missing: "vo".into(),
                });
            };
            check_rights(line.key.as_str(), &vo.rights)?;
        }
    }
    Ok(())
}

fn check_rights(token: &str, rights: &ReleaseRights) -> Result<(), DialogueError> {
    rights.validate().map_err(|_| DialogueError::Release {
        token: token.to_owned(),
        missing: "rights".into(),
    })
}
