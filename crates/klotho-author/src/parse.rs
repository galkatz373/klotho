//! RON canonical + kdown desugar → the same [`IntentDoc`] AST.
//!
//! Inner `when:` / `body:` (and affordance lists) are **RON fragments** of the
//! existing Pred / LawBody syntax. kdown only sugars document structure.
//!
//! ```text
//! # comment
//! style notes "chunky readable silhouettes"
//! style palette stone
//! style tag prop.stool
//!
//! law lock.use:
//!   when: EqVerb(Use)
//!   body: Pred(must: EqVerb(Use), ought: None)
//!
//! affordance Lockable:
//!   requires: []
//!   grants: []
//!   conflicts: []
//!
//! seed locus chair Relic
//! seed pose chair 0 0 0 0
//! ```
//!
//! `law lock.use:` becomes [`CanonDiff::AddLaw`] with id `"lock.use"`.

use std::path::Path;

use klotho_core::{Hash, LocusKind, Mm, PoseMm, YawMd};
use klotho_ir::{
    Affordance, CanonDiff, IntentDoc, IrError, Law, LawBody, Name, Pred, ProvenanceId, Rel,
    SeedFact, StyleIntent, from_ron,
};
use serde::de::DeserializeOwned;

use crate::error::AuthorError;

/// Load by extension: `*.kdown` is sugar, anything else is canonical RON.
pub fn load_file(path: &Path) -> Result<IntentDoc, AuthorError> {
    let src = std::fs::read_to_string(path)
        .map_err(|e| AuthorError::Io(format!("{}: {e}", path.display())))?;
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("kdown") => parse_kdown(&src).map_err(Into::into),
        _ => parse_ron(&src).map_err(Into::into),
    }
}

/// Parse canonical RON into an [`IntentDoc`].
pub fn parse_ron(src: &str) -> Result<IntentDoc, IrError> {
    from_ron(src)
}

/// Desugar kdown into the same [`IntentDoc`] AST as [`parse_ron`].
pub fn parse_kdown(src: &str) -> Result<IntentDoc, IrError> {
    let lines = preprocess(src)?;
    let mut style = StyleIntent::default();
    let mut canon_diffs = Vec::new();
    let mut seed = Vec::new();
    let mut pos = 0usize;
    while pos < lines.len() {
        let line = &lines[pos];
        if line.indent != 0 {
            return Err(parse_err(
                line.no,
                "top-level statement must not be indented",
            ));
        }
        let (kw, rest) = split_first(&line.text).ok_or_else(|| parse_err(line.no, "empty line"))?;
        match kw {
            "style" => {
                parse_style(line.no, rest, &mut style)?;
                pos += 1;
            }
            "law" => {
                let (law, next) = parse_law(&lines, pos)?;
                canon_diffs.push(CanonDiff::AddLaw(law));
                pos = next;
            }
            "affordance" => {
                let (aff, next) = parse_affordance(&lines, pos)?;
                canon_diffs.push(CanonDiff::AddAffordance(aff));
                pos = next;
            }
            "retract" => {
                canon_diffs.push(parse_retract(line.no, rest)?);
                pos += 1;
            }
            "seed" => {
                seed.push(parse_seed(line.no, rest)?);
                pos += 1;
            }
            other => {
                return Err(parse_err(line.no, &format!("unknown statement '{other}'")));
            }
        }
    }
    Ok(IntentDoc {
        style,
        canon_diffs,
        seed,
        minds: Vec::new(),
        provenance: ProvenanceId(Hash::ZERO),
    })
}

struct Line {
    no: usize,
    indent: usize,
    text: String,
}

fn preprocess(src: &str) -> Result<Vec<Line>, IrError> {
    let mut out = Vec::new();
    for (i, raw) in src.lines().enumerate() {
        let no = i + 1;
        let (indent, rest) = indent_of(raw, no)?;
        let text = rest.trim_end();
        if text.is_empty() || text.starts_with('#') {
            continue;
        }
        out.push(Line {
            no,
            indent,
            text: text.to_string(),
        });
    }
    Ok(out)
}

fn indent_of(raw: &str, no: usize) -> Result<(usize, &str), IrError> {
    let mut indent = 0usize;
    for (i, c) in raw.char_indices() {
        match c {
            ' ' => indent += 1,
            '\t' => return Err(parse_err(no, "tabs in indent")),
            _ => return Ok((indent, &raw[i..])),
        }
    }
    Ok((indent, ""))
}

fn parse_style(no: usize, rest: &str, style: &mut StyleIntent) -> Result<(), IrError> {
    let (kw, rest) = split_first(rest).ok_or_else(|| parse_err(no, "style needs a field"))?;
    match kw {
        "notes" => {
            style.notes = parse_notes(no, rest)?;
        }
        "palette" => {
            let (name, tail) = parse_name_token(no, rest)?;
            expect_empty(no, tail)?;
            style.palettes.push(name);
        }
        "tag" => {
            let (name, tail) = parse_name_token(no, rest)?;
            expect_empty(no, tail)?;
            style.kitbash_tags.push(name);
        }
        other => return Err(parse_err(no, &format!("unknown style field '{other}'"))),
    }
    Ok(())
}

fn parse_notes(no: usize, rest: &str) -> Result<String, IrError> {
    let rest = rest.trim();
    if rest.starts_with('"') {
        let (s, tail) = parse_quoted(no, rest)?;
        expect_empty(no, tail)?;
        Ok(s)
    } else {
        Ok(rest.to_string())
    }
}

fn parse_law(lines: &[Line], pos: usize) -> Result<(Law, usize), IrError> {
    let line = &lines[pos];
    let (_, rest) = split_first(&line.text).expect("law");
    let id = parse_block_id(line.no, rest)?;
    let (fields, next) = take_fields(lines, pos + 1, line.indent)?;
    let when = field_ron::<Pred>(line.no, &fields, "when")?;
    let body = field_ron::<LawBody>(line.no, &fields, "body")?;
    reject_unknown(line.no, &fields, &["when", "body"])?;
    Ok((Law { id, when, body }, next))
}

fn parse_affordance(lines: &[Line], pos: usize) -> Result<(Affordance, usize), IrError> {
    let line = &lines[pos];
    let (_, rest) = split_first(&line.text).expect("affordance");
    let id = parse_block_id(line.no, rest)?;
    let (fields, next) = take_fields(lines, pos + 1, line.indent)?;
    let requires = field_ron::<Vec<Pred>>(line.no, &fields, "requires")?;
    let grants = field_ron::<Vec<Name>>(line.no, &fields, "grants")?;
    let conflicts = field_ron::<Vec<Name>>(line.no, &fields, "conflicts")?;
    reject_unknown(line.no, &fields, &["requires", "grants", "conflicts"])?;
    Ok((
        Affordance {
            id,
            requires,
            grants,
            conflicts,
        },
        next,
    ))
}

fn parse_retract(no: usize, rest: &str) -> Result<CanonDiff, IrError> {
    let (id, rest) = parse_name_token(no, rest)?;
    let reason = parse_notes(no, rest)?;
    if reason.is_empty() {
        return Err(IrError::EmptyName);
    }
    Ok(CanonDiff::RetractLaw { id, reason })
}

fn parse_seed(no: usize, rest: &str) -> Result<SeedFact, IrError> {
    let (kw, rest) = split_first(rest).ok_or_else(|| parse_err(no, "seed needs a kind"))?;
    match kw {
        "locus" => {
            let (name, rest) = parse_name_token(no, rest)?;
            let (kind_tok, tail) =
                split_first(rest).ok_or_else(|| parse_err(no, "seed locus needs a LocusKind"))?;
            expect_empty(no, tail)?;
            Ok(SeedFact::Locus {
                name,
                kind: parse_kind(no, kind_tok)?,
            })
        }
        "pose" => {
            let (of, rest) = parse_name_token(no, rest)?;
            let nums = rest.split_whitespace().collect::<Vec<_>>();
            if nums.len() != 4 {
                return Err(parse_err(no, "seed pose needs x y z yaw"));
            }
            let x = parse_i32(no, nums[0])?;
            let y = parse_i32(no, nums[1])?;
            let z = parse_i32(no, nums[2])?;
            let yaw = parse_i32(no, nums[3])?;
            Ok(SeedFact::Pose {
                of,
                pose: PoseMm::new(Mm(x), Mm(y), Mm(z), YawMd(yaw)),
            })
        }
        "rel" => {
            let (a, rest) = parse_name_token(no, rest)?;
            let (rel_tok, rest) =
                split_first(rest).ok_or_else(|| parse_err(no, "seed rel needs a Rel"))?;
            let (b, tail) = parse_name_token(no, rest)?;
            expect_empty(no, tail)?;
            Ok(SeedFact::Rel {
                a,
                rel: parse_rel(no, rel_tok)?,
                b,
            })
        }
        "qty" => {
            let (of, rest) = parse_name_token(no, rest)?;
            let (res, rest) = parse_name_token(no, rest)?;
            let (val, tail) =
                split_first(rest).ok_or_else(|| parse_err(no, "seed qty needs a value"))?;
            expect_empty(no, tail)?;
            Ok(SeedFact::Qty {
                of,
                res,
                value: parse_i32(no, val)?,
            })
        }
        other => Err(parse_err(no, &format!("unknown seed '{other}'"))),
    }
}

fn parse_block_id(no: usize, rest: &str) -> Result<Name, IrError> {
    let rest = rest.trim();
    let rest = rest
        .strip_suffix(':')
        .ok_or_else(|| parse_err(no, "expected trailing ':'"))?;
    let rest = rest.trim();
    if rest.is_empty() {
        return Ok(Name::from(""));
    }
    let (name, tail) = parse_name_token(no, rest)?;
    expect_empty(no, tail)?;
    Ok(name)
}

struct Field {
    key: String,
    value: String,
    no: usize,
}

fn take_fields(
    lines: &[Line],
    mut pos: usize,
    parent_indent: usize,
) -> Result<(Vec<Field>, usize), IrError> {
    let mut fields = Vec::new();
    while pos < lines.len() && lines[pos].indent > parent_indent {
        let line = &lines[pos];
        let Some((key, rest)) = split_colon(&line.text) else {
            return Err(parse_err(line.no, "expected field key: value"));
        };
        let mut buf = rest.trim().to_string();
        pos += 1;
        while pos < lines.len() && lines[pos].indent > line.indent {
            if !buf.is_empty() {
                buf.push('\n');
            }
            buf.push_str(&lines[pos].text);
            pos += 1;
        }
        if buf.trim().is_empty() {
            return Err(parse_err(line.no, &format!("empty value for '{key}'")));
        }
        fields.push(Field {
            key: key.to_string(),
            value: buf,
            no: line.no,
        });
    }
    Ok((fields, pos))
}

fn field_ron<T: DeserializeOwned>(no: usize, fields: &[Field], key: &str) -> Result<T, IrError> {
    let f = fields
        .iter()
        .find(|f| f.key == key)
        .ok_or_else(|| parse_err(no, &format!("missing '{key}'")))?;
    from_ron::<T>(&f.value).map_err(|e| match e {
        IrError::Parse(s) => IrError::Parse(format!("kdown:{}: {s}", f.no)),
        other => other,
    })
}

fn reject_unknown(no: usize, fields: &[Field], allowed: &[&str]) -> Result<(), IrError> {
    for f in fields {
        if !allowed.contains(&f.key.as_str()) {
            return Err(parse_err(no, &format!("unknown field '{}'", f.key)));
        }
    }
    Ok(())
}

fn parse_kind(no: usize, s: &str) -> Result<LocusKind, IrError> {
    match s {
        "Actor" => Ok(LocusKind::Actor),
        "Place" => Ok(LocusKind::Place),
        "Relic" => Ok(LocusKind::Relic),
        "Law" => Ok(LocusKind::Law),
        "Beat" => Ok(LocusKind::Beat),
        "Chorus" => Ok(LocusKind::Chorus),
        "Observer" => Ok(LocusKind::Observer),
        other => Err(parse_err(no, &format!("unknown LocusKind '{other}'"))),
    }
}

fn parse_rel(no: usize, s: &str) -> Result<Rel, IrError> {
    match s {
        "In" => Ok(Rel::In),
        "OwnedBy" => Ok(Rel::OwnedBy),
        "WieldedBy" => Ok(Rel::WieldedBy),
        "KeyedBy" => Ok(Rel::KeyedBy),
        "Knows" => Ok(Rel::Knows),
        "Owes" => Ok(Rel::Owes),
        "Fears" => Ok(Rel::Fears),
        "PartOf" => Ok(Rel::PartOf),
        "DerivedFrom" => Ok(Rel::DerivedFrom),
        "LockedBy" => Ok(Rel::LockedBy),
        "Dead" => Ok(Rel::Dead),
        other => Err(parse_err(no, &format!("unknown Rel '{other}'"))),
    }
}

fn parse_name_token(no: usize, s: &str) -> Result<(Name, &str), IrError> {
    let s = s.trim_start();
    if s.starts_with('"') {
        let (inner, rest) = parse_quoted(no, s)?;
        Ok((Name::from(inner.as_str()), rest))
    } else {
        let (tok, rest) = split_first(s).ok_or_else(|| parse_err(no, "expected name"))?;
        Ok((Name::from(tok), rest))
    }
}

fn parse_quoted(no: usize, s: &str) -> Result<(String, &str), IrError> {
    let s = s.trim_start();
    let rest = s
        .strip_prefix('"')
        .ok_or_else(|| parse_err(no, "expected quoted string"))?;
    match rest.find('"') {
        Some(end) => Ok((rest[..end].to_string(), rest[end + 1..].trim_start())),
        None => Err(parse_err(no, "unterminated quoted string")),
    }
}

fn parse_i32(no: usize, s: &str) -> Result<i32, IrError> {
    s.parse()
        .map_err(|_| parse_err(no, &format!("not an i32 '{s}'")))
}

fn split_first(s: &str) -> Option<(&str, &str)> {
    let s = s.trim_start();
    if s.is_empty() {
        return None;
    }
    match s.find(char::is_whitespace) {
        Some(i) => Some((&s[..i], s[i..].trim_start())),
        None => Some((s, "")),
    }
}

fn split_colon(s: &str) -> Option<(&str, &str)> {
    let i = s.find(':')?;
    Some((s[..i].trim(), s[i + 1..].trim()))
}

fn expect_empty(no: usize, s: &str) -> Result<(), IrError> {
    if s.trim().is_empty() {
        Ok(())
    } else {
        Err(parse_err(no, &format!("unexpected trailing '{s}'")))
    }
}

fn parse_err(no: usize, msg: &str) -> IrError {
    IrError::Parse(format!("kdown:{no}: {msg}"))
}

#[cfg(test)]
mod tests {
    use klotho_ir::{CanonDiff, LawBody, Verb, to_ron, validate_doc};

    use super::*;

    const CHAIR_KDOWN: &str = r#"
# comment
style notes "chunky readable silhouettes"
style palette stone
style tag prop.stool

law lock.use:
  when: EqVerb(Use)
  body: Pred(must: EqVerb(Use), ought: None)

affordance Lockable:
  requires: []
  grants: []
  conflicts: []

seed locus chair Relic
seed pose chair 0 0 0 0
"#;

    const CHAIR_RON: &str = r#"
IntentDoc(
    style: StyleIntent(
        notes: "chunky readable silhouettes",
        palettes: ["stone"],
        kitbash_tags: ["prop.stool"],
    ),
    canon_diffs: [
        AddLaw(Law(
            id: "lock.use",
            when: EqVerb(Use),
            body: Pred(must: EqVerb(Use), ought: None),
        )),
        AddAffordance(Affordance(
            id: "Lockable",
            requires: [],
            grants: [],
            conflicts: [],
        )),
    ],
    seed: [
        Locus(name: "chair", kind: Relic),
        Pose(of: "chair", pose: PoseMm(x: Mm(0), z: Mm(0), y: Mm(0), yaw: YawMd(0))),
    ],
    minds: [],
    provenance: ProvenanceId("0000000000000000000000000000000000000000000000000000000000000000"),
)
"#;

    fn chair_expected() -> IntentDoc {
        IntentDoc {
            style: StyleIntent {
                notes: "chunky readable silhouettes".into(),
                palettes: vec![Name::from("stone")],
                kitbash_tags: vec![Name::from("prop.stool")],
            },
            canon_diffs: vec![
                CanonDiff::AddLaw(Law {
                    id: Name::from("lock.use"),
                    when: Pred::EqVerb(Verb::Use),
                    body: LawBody::Pred {
                        must: Pred::EqVerb(Verb::Use),
                        ought: None,
                    },
                }),
                CanonDiff::AddAffordance(Affordance {
                    id: Name::from("Lockable"),
                    requires: vec![],
                    grants: vec![],
                    conflicts: vec![],
                }),
            ],
            seed: vec![
                SeedFact::Locus {
                    name: Name::from("chair"),
                    kind: LocusKind::Relic,
                },
                SeedFact::Pose {
                    of: Name::from("chair"),
                    pose: PoseMm::new(Mm(0), Mm(0), Mm(0), YawMd(0)),
                },
            ],
            minds: Vec::new(),
            provenance: ProvenanceId(Hash::ZERO),
        }
    }

    #[test]
    fn kdown_law_lock_use_desugars_to_add_law() {
        let doc = parse_kdown(CHAIR_KDOWN).unwrap();
        validate_doc(&doc).unwrap();
        match &doc.canon_diffs[0] {
            CanonDiff::AddLaw(law) => {
                assert_eq!(law.id.as_str(), "lock.use");
                assert_eq!(law.when, Pred::EqVerb(Verb::Use));
            }
            other => panic!("{other:?}"),
        }
        let ron_law: CanonDiff = from_ron(
            r#"AddLaw(Law(id: "lock.use", when: EqVerb(Use), body: Pred(must: EqVerb(Use), ought: None)))"#,
        )
        .unwrap();
        assert_eq!(doc.canon_diffs[0], ron_law);
    }

    #[test]
    fn kdown_and_ron_chair_docs_are_eq() {
        let from_k = parse_kdown(CHAIR_KDOWN).unwrap();
        let from_r = parse_ron(CHAIR_RON).unwrap();
        let expected = chair_expected();
        assert_eq!(from_k, expected);
        assert_eq!(from_r, expected);
        assert_eq!(from_k, from_r);
    }

    #[test]
    fn kdown_round_trip_via_canonical_ron() {
        let doc = parse_kdown(CHAIR_KDOWN).unwrap();
        let text = to_ron(&doc).unwrap();
        let again: IntentDoc = from_ron(&text).unwrap();
        assert_eq!(doc, again);
    }

    #[test]
    fn empty_law_id_fails_validate() {
        let src = r#"
law :
  when: EqVerb(Use)
  body: Pred(must: EqVerb(Use), ought: None)
"#;
        let doc = parse_kdown(src).unwrap();
        assert_eq!(validate_doc(&doc), Err(IrError::EmptyName));
    }

    #[test]
    fn seed_rel_and_qty() {
        let doc = parse_kdown(
            r#"
seed locus chair Relic
seed rel chair LockedBy chair
seed qty chair mass_g 12
"#,
        )
        .unwrap();
        assert_eq!(
            doc.seed[1],
            SeedFact::Rel {
                a: Name::from("chair"),
                rel: Rel::LockedBy,
                b: Name::from("chair"),
            }
        );
        assert_eq!(
            doc.seed[2],
            SeedFact::Qty {
                of: Name::from("chair"),
                res: Name::from("mass_g"),
                value: 12,
            }
        );
    }
}
