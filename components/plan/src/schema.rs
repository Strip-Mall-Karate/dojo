// Copyright (c) 2026 Strip Mall Karate Company
//
// Licensed under the MIT license. See LICENSE in the repository root.

//! The keys a plan may set, and how each is checked and reported.
//!
//! Parsing reads the plan's table without running any of its code: tables
//! with metatables are refused, and every read is raw.

use std::collections::{BTreeMap,
                       BTreeSet};

use mlua::{Table,
           Value};
use serde_json::{Map,
                 Value as Json,
                 json};

use crate::Error;

/// The package targets Dojo builds for, as `components/core` names them.
pub const TARGETS: &[&str] = &["x86_64-linux",
                               "aarch64-linux",
                               "x86_64-linux-kernel2",
                               "x86_64-darwin",
                               "x86_64-windows"];

/// The build phases, in the order a build runs them.
pub const PHASES: &[&str] =
    &["download", "verify", "unpack", "prepare", "build", "check", "install", "strip"];

#[derive(Clone, Copy)]
enum Kind {
    Name,
    Text,
    Version,
    Targets,
    Texts,
    Ident,
    Idents,
    Paths,
    Source,
    Type,
    Svc,
    Exports,
    Binds,
    On,
}

/// Metadata keys in the order `show` prints them.
const KEYS: &[(&str, Kind)] = &[("name", Kind::Name),
                                ("origin", Kind::Name),
                                ("version", Kind::Version),
                                ("targets", Kind::Targets),
                                ("maintainer", Kind::Text),
                                ("license", Kind::Texts),
                                ("description", Kind::Text),
                                ("upstream_url", Kind::Text),
                                ("source", Kind::Source),
                                ("deps", Kind::Idents),
                                ("build_deps", Kind::Idents),
                                ("scaffolding", Kind::Ident),
                                ("type", Kind::Type),
                                ("bin_dirs", Kind::Paths),
                                ("lib_dirs", Kind::Paths),
                                ("include_dirs", Kind::Paths),
                                ("pconfig_dirs", Kind::Paths),
                                ("interpreters", Kind::Paths),
                                ("svc", Kind::Svc),
                                ("exports", Kind::Exports),
                                ("exposes", Kind::Texts),
                                ("binds", Kind::Binds),
                                ("binds_optional", Kind::Binds),
                                ("on", Kind::On)];

const REQUIRED: &[&str] = &["name", "origin", "version", "targets"];

/// Keys a target's `on` entry may not override.
const BASE_ONLY: &[&str] = &["name", "origin", "targets", "on"];

#[derive(Clone, Debug)]
enum Field {
    Json(Json),
    ComputedVersion,
    Phase,
}

/// One table's worth of checked keys: the plan's base, or one `on` entry.
#[derive(Clone, Debug, Default)]
struct Fields(BTreeMap<&'static str, Field>);

/// A plan's checked metadata.
#[derive(Debug)]
pub struct Metadata {
    base: Fields,
    on:   BTreeMap<String, Fields>,
}

impl Metadata {
    /// Checks a plan's returned table.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] names the first key that does not fit the schema.
    pub fn parse(table: &Table) -> Result<Metadata, Error> {
        let mut base = Fields::default();
        let mut on = BTreeMap::new();
        for (key, value) in entries(table, "the plan")? {
            match kind_of(&key) {
                Some(("on", _)) => on = parse_on(&value)?,
                Some((name, kind)) => {
                    base.0.insert(name, parse_field(kind, &value, name)?);
                }
                None => return Err(unknown_key(&key, "the plan")),
            }
        }
        for name in REQUIRED {
            if !base.0.contains_key(name) {
                return Err(Error::Invalid(format!("the plan does not set `{name}`")));
            }
        }
        let targets = base.strings("targets");
        for target in on.keys() {
            if !targets.contains(target) {
                return Err(Error::Invalid(format!("`on.{target}`: `{target}` is not \
                                                   in `targets`")));
            }
        }
        let metadata = Metadata { base, on };
        metadata.base.check_exposes("")?;
        for target in &targets {
            metadata.merged(target)
                    .check_exposes(&format!("`on.{target}`: "))?;
        }
        Ok(metadata)
    }

    /// The targets the plan builds for.
    #[must_use]
    pub fn targets(&self) -> Vec<String> { self.base.strings("targets") }

    /// The metadata as JSON: the base keys with each target's overrides
    /// under `on`, or, given a target, the base with that target's overrides
    /// applied.
    #[must_use]
    pub fn to_json(&self, target: Option<&str>) -> Json {
        if let Some(target) = target {
            let mut out = Map::new();
            out.insert("target".into(), json!(target));
            out.extend(self.merged(target).to_json());
            Json::Object(out)
        } else {
            let mut out = self.base.to_json();
            if !self.on.is_empty() {
                let on =
                    self.on
                        .iter()
                        .map(|(target, fields)| (target.clone(), Json::Object(fields.to_json())))
                        .collect();
                out.insert("on".into(), Json::Object(on));
            }
            Json::Object(out)
        }
    }

    fn merged(&self, target: &str) -> Fields {
        let mut fields = self.base.clone();
        if let Some(overrides) = self.on.get(target) {
            fields.0
                  .extend(overrides.0.iter().map(|(key, field)| (*key, field.clone())));
        }
        fields
    }
}

impl Fields {
    fn strings(&self, key: &str) -> Vec<String> {
        match self.0.get(key) {
            Some(Field::Json(Json::Array(items))) => {
                items.iter()
                     .filter_map(|item| item.as_str().map(String::from))
                     .collect()
            }
            _ => Vec::new(),
        }
    }

    fn check_exposes(&self, prefix: &str) -> Result<(), Error> {
        let exports: BTreeSet<String> = match self.0.get("exports") {
            Some(Field::Json(Json::Object(exports))) => exports.keys().cloned().collect(),
            _ => BTreeSet::new(),
        };
        for exposed in self.strings("exposes") {
            if !exports.contains(&exposed) {
                return Err(Error::Invalid(format!("{prefix}`exposes` names \
                                                   `{exposed}`, which is not in \
                                                   `exports`")));
            }
        }
        Ok(())
    }

    fn to_json(&self) -> Map<String, Json> {
        let mut out = Map::new();
        for (name, _) in KEYS {
            match self.0.get(name) {
                Some(Field::Json(value)) => {
                    out.insert((*name).into(), value.clone());
                }
                Some(Field::ComputedVersion) => {
                    out.insert("version".into(), Json::Null);
                    out.insert("version_computed".into(), json!(true));
                }
                Some(Field::Phase) | None => {}
            }
        }
        let phases: Vec<&str> =
            PHASES.iter()
                  .copied()
                  .filter(|phase| matches!(self.0.get(phase), Some(Field::Phase)))
                  .collect();
        if !phases.is_empty() {
            out.insert("phases".into(), json!(phases));
        }
        out
    }
}

fn kind_of(key: &str) -> Option<(&'static str, Option<Kind>)> {
    if let Some((name, kind)) = KEYS.iter().find(|(name, _)| *name == key) {
        return Some((name, Some(*kind)));
    }
    PHASES.iter()
          .find(|phase| **phase == key)
          .map(|phase| (*phase, None))
}

fn unknown_key(key: &str, context: &str) -> Error {
    Error::Invalid(format!("{context} sets unknown key `{key}`"))
}

fn parse_on(value: &Value) -> Result<BTreeMap<String, Fields>, Error> {
    let mut on = BTreeMap::new();
    for (target, value) in entries(&table_of(value, "`on`")?, "`on`")? {
        if !TARGETS.contains(&target.as_str()) {
            return Err(Error::Invalid(format!("`on.{target}`: unknown target `{target}`")));
        }
        let context = format!("`on.{target}`");
        let mut fields = Fields::default();
        for (key, value) in entries(&table_of(&value, &context)?, &context)? {
            match kind_of(&key) {
                Some((name, _)) if BASE_ONLY.contains(&name) => {
                    return Err(Error::Invalid(format!("{context} may not override `{name}`")));
                }
                Some((name, kind)) => {
                    let what = format!("on.{target}.{name}");
                    fields.0.insert(name, parse_field(kind, &value, &what)?);
                }
                None => return Err(unknown_key(&key, &context)),
            }
        }
        on.insert(target, fields);
    }
    Ok(on)
}

/// Parses one key. `kind` is `None` for a phase.
fn parse_field(kind: Option<Kind>, value: &Value, what: &str) -> Result<Field, Error> {
    let what = format!("`{what}`");
    let Some(kind) = kind else {
        return match value {
            Value::Function(_) => Ok(Field::Phase),
            other => Err(wrong_type(&what, "a function", other)),
        };
    };
    let json = match kind {
        Kind::Name => json!(name_of(value, &what)?),
        Kind::Text => json!(string_of(value, &what)?),
        Kind::Version => {
            return match value {
                Value::Function(_) => Ok(Field::ComputedVersion),
                Value::String(_) => Ok(Field::Json(json!(nonempty(value, &what)?))),
                other => Err(wrong_type(&what, "a string or a function", other)),
            };
        }
        Kind::Targets => {
            let targets = strings_of(value, &what)?;
            if targets.is_empty() {
                return Err(Error::Invalid(format!("{what} is empty")));
            }
            let mut seen = BTreeSet::new();
            for target in &targets {
                if !TARGETS.contains(&target.as_str()) {
                    return Err(Error::Invalid(format!("{what}: unknown target `{target}`")));
                }
                if !seen.insert(target) {
                    return Err(Error::Invalid(format!("{what} lists `{target}` twice")));
                }
            }
            json!(targets)
        }
        Kind::Texts => json!(strings_of(value, &what)?),
        Kind::Ident => json!(ident_of(value, &what)?),
        Kind::Idents => {
            let idents = strings_of(value, &what)?;
            for ident in &idents {
                check_ident(ident, &what)?;
            }
            json!(idents)
        }
        Kind::Paths => {
            let paths = strings_of(value, &what)?;
            for path in &paths {
                check_relative(path, &what)?;
            }
            json!(paths)
        }
        Kind::Source => {
            let fields = record(value, &what, &["url", "sha256"], &["filename", "dirname"])?;
            let sha = fields["sha256"].as_str().unwrap_or_default();
            if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(Error::Invalid(format!("{what}: `sha256` is not 64 \
                                                   hexadecimal digits")));
            }
            Json::Object(fields)
        }
        Kind::Type => {
            let kind = string_of(value, &what)?;
            if kind != "standalone" && kind != "composite" {
                return Err(Error::Invalid(format!("{what} is `{kind}`, not \
                                                   `standalone` or `composite`")));
            }
            json!(kind)
        }
        Kind::Svc => Json::Object(record(value, &what, &["run"], &["user", "group"])?),
        Kind::Exports => {
            let mut exports = Map::new();
            for (key, value) in entries(&table_of(value, &what)?, &what)? {
                exports.insert(key.clone(),
                               json!(nonempty(&value, &format!("{what}.{key}"))?));
            }
            Json::Object(exports)
        }
        Kind::Binds => {
            let mut binds = Map::new();
            for (key, value) in entries(&table_of(value, &what)?, &what)? {
                binds.insert(key.clone(),
                             json!(strings_of(&value, &format!("{what}.{key}"))?));
            }
            Json::Object(binds)
        }
        Kind::On => unreachable!("`on` is parsed by parse_on"),
    };
    Ok(Field::Json(json))
}

/// A table with fixed string keys, some required.
fn record(value: &Value,
          what: &str,
          required: &[&str],
          optional: &[&str])
          -> Result<Map<String, Json>, Error> {
    let mut out = Map::new();
    for (key, value) in entries(&table_of(value, what)?, what)? {
        if !required.contains(&key.as_str()) && !optional.contains(&key.as_str()) {
            return Err(unknown_key(&key, what));
        }
        out.insert(key.clone(),
                   json!(nonempty(&value, &format!("{what}.{key}"))?));
    }
    for key in required {
        if !out.contains_key(*key) {
            return Err(Error::Invalid(format!("{what} does not set `{key}`")));
        }
    }
    Ok(out)
}

fn table_of(value: &Value, what: &str) -> Result<Table, Error> {
    match value {
        Value::Table(table) if table.metatable().is_some() => {
            Err(Error::Invalid(format!("{what} is a table with a metatable")))
        }
        Value::Table(table) => Ok(table.clone()),
        other => Err(wrong_type(what, "a table", other)),
    }
}

/// A table's entries, which must all have string keys, sorted by key.
fn entries(table: &Table, what: &str) -> Result<Vec<(String, Value)>, Error> {
    if table.metatable().is_some() {
        return Err(Error::Invalid(format!("{what} is a table with a metatable")));
    }
    let mut out = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair.map_err(|e| Error::Invalid(format!("{what}: {e}")))?;
        match key {
            Value::String(key) => {
                let key =
                    key.to_str().map_err(|_| {
                                     Error::Invalid(format!("{what} has a key that is not UTF-8"))
                                 })?;
                out.push((key.to_string(), value));
            }
            other => {
                return Err(Error::Invalid(format!("{what} has a {} key, where only \
                                                   string keys are allowed",
                                                  other.type_name())));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn string_of(value: &Value, what: &str) -> Result<String, Error> {
    match value {
        Value::String(text) => {
            text.to_str()
                .map(|text| text.to_string())
                .map_err(|_| Error::Invalid(format!("{what} is not UTF-8")))
        }
        other => Err(wrong_type(what, "a string", other)),
    }
}

fn nonempty(value: &Value, what: &str) -> Result<String, Error> {
    let text = string_of(value, what)?;
    if text.trim().is_empty() {
        return Err(Error::Invalid(format!("{what} is empty")));
    }
    Ok(text)
}

/// A sequence of strings: keys 1 to n and nothing else.
fn strings_of(value: &Value, what: &str) -> Result<Vec<String>, Error> {
    let table = table_of(value, what)?;
    let length = table.raw_len();
    let mut count = 0;
    for pair in table.pairs::<Value, Value>() {
        let (key, _) = pair.map_err(|e| Error::Invalid(format!("{what}: {e}")))?;
        match key {
            Value::Integer(i) if i >= 1 && usize::try_from(i).is_ok_and(|i| i <= length) => {}
            _ => return Err(Error::Invalid(format!("{what} is not a list"))),
        }
        count += 1;
    }
    if count != length {
        return Err(Error::Invalid(format!("{what} is not a list")));
    }
    (1..=length).map(|i| {
                    let item = table.raw_get::<Value>(i)
                                    .map_err(|e| Error::Invalid(format!("{what}: {e}")))?;
                    nonempty(&item, &format!("{what}[{i}]"))
                })
                .collect()
}

fn is_name(text: &str) -> bool {
    !text.is_empty()
    && text.bytes()
           .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn name_of(value: &Value, what: &str) -> Result<String, Error> {
    let name = string_of(value, what)?;
    if !is_name(&name) {
        return Err(Error::Invalid(format!("{what} is `{name}`; names use \
                                           letters, digits, `_` and `-`")));
    }
    Ok(name)
}

fn ident_of(value: &Value, what: &str) -> Result<String, Error> {
    let ident = string_of(value, what)?;
    check_ident(&ident, what)?;
    Ok(ident)
}

/// `origin/name`, optionally followed by `/version` and `/release`.
fn check_ident(ident: &str, what: &str) -> Result<(), Error> {
    let parts: Vec<&str> = ident.split('/').collect();
    let valid = (2..=4).contains(&parts.len())
                && is_name(parts[0])
                && is_name(parts[1])
                && parts[2..].iter()
                             .all(|part| !part.is_empty() && !part.contains(char::is_whitespace));
    if valid {
        Ok(())
    } else {
        Err(Error::Invalid(format!("{what}: `{ident}` is not a package ident like `origin/name`")))
    }
}

fn check_relative(path: &str, what: &str) -> Result<(), Error> {
    let absolute = path.starts_with('/') || path.starts_with('\\') || path.contains(':');
    let escapes = path.split(['/', '\\']).any(|part| part == "..");
    if absolute || escapes {
        return Err(Error::Invalid(format!("{what}: `{path}` must be a path inside the package")));
    }
    Ok(())
}

fn wrong_type(what: &str, expected: &str, value: &Value) -> Error {
    Error::Invalid(format!("{what} is a {}, not {expected}", value.type_name()))
}
