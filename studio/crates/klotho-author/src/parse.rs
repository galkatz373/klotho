//! RON canonical + kdown desugar → the same [`IntentDoc`] AST.
//!
//! kdown sugars document structure (`style`, `law`, `affordance`, `retract`,
//! `seed`). Rites, beats, minds, and provenance stay canonical RON.
//! Inner `when:` / `body:` (and affordance lists) are **RON fragments** of the
//! existing Pred / LawBody syntax.
//!
//! Comments are full-line `#` only (optional ` #` tails on statement lines,
//! not inside quotes). They are not RON payload.
//!
//! `seed pose <name> x y z yaw` matches [`PoseMm::new`] and struct fields
//! (`x, y, z, yaw`; pitch/roll default 0). Repeated `style notes` last-wins;
//! `style palette` / `style tag` append.
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
//! seed pose chair 1 2 3 4
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

/// Load a single [`IntentDoc`] by extension. Projects go through [`crate::load_file`].
pub fn load_doc_file(path: &Path) -> Result<IntentDoc, AuthorError> {
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
        let trimmed = raw.trim_start_matches([' ', '\t']);
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let (indent, rest) = indent_of(raw, no)?;
        let text = strip_trailing_comment(rest.trim_end());
        if text.is_empty() {
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
    let rest = match split_first(&line.text) {
        Some(("law", rest)) => rest,
        _ => return Err(parse_err(line.no, "expected law")),
    };
    let id = parse_block_id(line.no, rest)?;
    let (fields, next) = take_fields(lines, pos + 1, line.indent, &["when", "body"])?;
    let when = field_ron::<Pred>(line.no, &fields, "when")?;
    let body = field_ron::<LawBody>(line.no, &fields, "body")?;
    Ok((Law { id, when, body }, next))
}

fn parse_affordance(lines: &[Line], pos: usize) -> Result<(Affordance, usize), IrError> {
    let line = &lines[pos];
    let rest = match split_first(&line.text) {
        Some(("affordance", rest)) => rest,
        _ => return Err(parse_err(line.no, "expected affordance")),
    };
    let id = parse_block_id(line.no, rest)?;
    let (fields, next) = take_fields(
        lines,
        pos + 1,
        line.indent,
        &["requires", "grants", "conflicts"],
    )?;
    let requires = field_ron::<Vec<Pred>>(line.no, &fields, "requires")?;
    let grants = field_ron::<Vec<Name>>(line.no, &fields, "grants")?;
    let conflicts = field_ron::<Vec<Name>>(line.no, &fields, "conflicts")?;
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
    allowed: &[&str],
) -> Result<(Vec<Field>, usize), IrError> {
    if pos >= lines.len() || lines[pos].indent <= parent_indent {
        return Ok((Vec::new(), pos));
    }
    let field_indent = lines[pos].indent;
    let mut fields: Vec<Field> = Vec::new();
    while pos < lines.len() {
        let line = &lines[pos];
        if line.indent <= parent_indent {
            break;
        }
        if line.indent != field_indent {
            return Err(parse_err(line.no, "mixed field indent"));
        }
        let Some(key) = ident_key(&line.text) else {
            return Err(parse_err(line.no, "expected field key: value"));
        };
        if !allowed.contains(&key) {
            return Err(parse_err(line.no, &format!("unknown field '{key}'")));
        }
        let key = key.to_string();
        if fields.iter().any(|f| f.key == key) {
            return Err(parse_err(line.no, &format!("duplicate field '{key}'")));
        }
        let rest = split_colon(&line.text).map(|(_, r)| r).unwrap_or("");
        let mut buf = rest.trim().to_string();
        let field_no = line.no;
        pos += 1;
        while pos < lines.len() {
            let cont = &lines[pos];
            if cont.indent <= parent_indent {
                break;
            }
            if cont.indent < field_indent {
                return Err(parse_err(cont.no, "mixed field indent"));
            }
            if cont.indent == field_indent {
                if ident_key(&cont.text).is_some() {
                    break;
                }
            } else if ident_key(&cont.text).is_some_and(|k| allowed.contains(&k)) {
                return Err(parse_err(cont.no, "mixed field indent"));
            }
            if !buf.is_empty() {
                buf.push('\n');
            }
            buf.push_str(&cont.text);
            pos += 1;
        }
        if buf.trim().is_empty() {
            return Err(parse_err(field_no, &format!("empty value for '{key}'")));
        }
        fields.push(Field {
            key,
            value: buf,
            no: field_no,
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
    Rel::from_name(s).ok_or_else(|| parse_err(no, &format!("unknown Rel '{s}'")))
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

fn ident_key(text: &str) -> Option<&str> {
    let (key, _) = split_colon(text)?;
    is_ident(key).then_some(key)
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn strip_trailing_comment(s: &str) -> &str {
    let mut in_quote = false;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => in_quote = !in_quote,
            b'#' if !in_quote && (i == 0 || bytes[i - 1].is_ascii_whitespace()) => {
                return s[..i].trim_end();
            }
            _ => {}
        }
        i += 1;
    }
    s
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
seed pose chair 1 2 3 4
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
        Pose(of: "chair", pose: PoseMm(x: Mm(1), z: Mm(3), y: Mm(2), yaw: YawMd(4))),
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
                    pose: PoseMm::new(Mm(1), Mm(2), Mm(3), YawMd(4)),
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

    #[test]
    fn hanging_ron_closers_are_continuations() {
        let doc = parse_kdown(
            r#"
law lock.use:
  when: Or(
    EqVerb(Use),
    EqVerb(Talk)
  )
  body: Pred(
    must: EqVerb(Use),
    ought: None
  )

affordance Lockable:
  requires: [
    EqVerb(Use)
  ]
  grants: []
  conflicts: []
"#,
        )
        .unwrap();
        match &doc.canon_diffs[0] {
            CanonDiff::AddLaw(law) => match &law.when {
                Pred::Or(a, b) => {
                    assert_eq!(**a, Pred::EqVerb(Verb::Use));
                    assert_eq!(**b, Pred::EqVerb(Verb::Talk));
                }
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        }
        match &doc.canon_diffs[1] {
            CanonDiff::AddAffordance(a) => {
                assert_eq!(a.requires, vec![Pred::EqVerb(Verb::Use)]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn mixed_sibling_indent_is_an_error() {
        let err = parse_kdown(
            r#"
law lock.use:
  when: EqVerb(Use)
    body: Pred(must: EqVerb(Use), ought: None)
"#,
        )
        .unwrap_err();
        match err {
            IrError::Parse(s) => assert!(s.contains("mixed field indent"), "{s}"),
            other => panic!("{other}"),
        }
    }

    #[test]
    fn tabs_in_indent_are_an_error() {
        let err = parse_kdown("law lock.use:\n\twhen: EqVerb(Use)\n").unwrap_err();
        match err {
            IrError::Parse(s) => assert!(s.contains("tabs in indent"), "{s}"),
            other => panic!("{other}"),
        }
    }

    #[test]
    fn tab_on_comment_line_is_skipped() {
        let doc = parse_kdown("\t# comment\nseed locus chair Relic\n").unwrap();
        assert_eq!(doc.seed.len(), 1);
    }

    #[test]
    fn unknown_field_at_indent_is_not_swallowed() {
        let err = parse_kdown(
            r#"
law lock.use:
  when: EqVerb(Use)
  ought: None
  body: Pred(must: EqVerb(Use), ought: None)
"#,
        )
        .unwrap_err();
        match err {
            IrError::Parse(s) => assert!(s.contains("unknown field 'ought'"), "{s}"),
            other => panic!("{other}"),
        }
    }

    #[test]
    fn duplicate_block_field_is_an_error() {
        let err = parse_kdown(
            r#"
law lock.use:
  when: EqVerb(Use)
  when: EqVerb(Talk)
  body: Pred(must: EqVerb(Use), ought: None)
"#,
        )
        .unwrap_err();
        match err {
            IrError::Parse(s) => assert!(s.contains("duplicate field 'when'"), "{s}"),
            other => panic!("{other}"),
        }
    }

    #[test]
    fn style_notes_last_wins_tags_append() {
        let doc = parse_kdown(
            r#"
style notes "first"
style notes "second"
style tag prop.stool
style tag prop.stool
style palette stone
style palette metal
"#,
        )
        .unwrap();
        assert_eq!(doc.style.notes, "second");
        assert_eq!(
            doc.style.kitbash_tags,
            vec![Name::from("prop.stool"), Name::from("prop.stool")]
        );
        assert_eq!(
            doc.style.palettes,
            vec![Name::from("stone"), Name::from("metal")]
        );
    }

    #[test]
    fn trailing_hash_comment_is_stripped() {
        let doc = parse_kdown("seed locus chair Relic # oak\n").unwrap();
        assert_eq!(
            doc.seed[0],
            SeedFact::Locus {
                name: Name::from("chair"),
                kind: LocusKind::Relic,
            }
        );
        let notes = parse_kdown(r#"style notes "keep # hash""#).unwrap();
        assert_eq!(notes.style.notes, "keep # hash");
    }
}
