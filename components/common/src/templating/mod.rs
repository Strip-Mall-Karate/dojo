// Copyright (c) 2016-2017 Chef Software Inc. and/or applicable contributors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

pub mod config;
mod context;
pub mod helpers;
pub mod hooks;
pub mod package;
pub mod test_helpers;

use std::{fmt,
          ops::{Deref,
                DerefMut},
          path::Path,
          result};

use handlebars::{no_escape,
                 Handlebars,
                 TemplateError};
use serde::Serialize;
use serde_json;

use crate::{error::{Error,
                    Result},
            hcore::{fs,
                    package::PackageInstall},
            templating::hooks::{Hook,
                                InstallHook}};

pub use self::context::RenderContext;

/// A convenience method that compiles a package's install hook
/// and any configuration templates in its config_install folder
pub fn compile_for_package_install(package: &PackageInstall) -> Result<()> {
    let pkg = package::Pkg::from_install(package)?;

    fs::SvcDir::new(&pkg.name, &pkg.svc_user, &pkg.svc_group).create()?;

    let cfg = config::Cfg::new(&pkg, None)?;
    let ctx = RenderContext::new(&pkg, &cfg);
    let cfg_renderer = config::CfgRenderer::new(pkg.path.join("config_install"))?;
    cfg_renderer.compile(&pkg.name, &pkg, &pkg.svc_config_install_path, &ctx)?;

    if let Some(ref hook) = InstallHook::load(&pkg.name,
                                              &fs::svc_hooks_path(&pkg.name),
                                              &package.installed_path.join("hooks"))
    {
        hook.compile(&pkg.name, &ctx)?;
    };

    Ok(())
}

pub struct TemplateRenderer(Handlebars<'static>);

impl TemplateRenderer {
    pub fn new() -> Self {
        let mut handlebars = Handlebars::new();
        handlebars.register_helper("eachAlive", Box::new(helpers::EACH_ALIVE));
        handlebars.register_helper("pkgPathFor", Box::new(helpers::PKG_PATH_FOR));
        handlebars.register_helper("strConcat", Box::new(helpers::STR_CONCAT));
        handlebars.register_helper("strJoin", Box::new(helpers::STR_JOIN));
        handlebars.register_helper("strReplace", Box::new(helpers::STR_REPLACE));
        handlebars.register_helper("toUppercase", Box::new(helpers::TO_UPPERCASE));
        handlebars.register_helper("toLowercase", Box::new(helpers::TO_LOWERCASE));
        handlebars.register_helper("toJson", Box::new(helpers::TO_JSON));
        handlebars.register_helper("toToml", Box::new(helpers::TO_TOML));
        handlebars.register_helper("toYaml", Box::new(helpers::TO_YAML));

        // Templates render configuration files and hooks, not HTML.
        handlebars.register_escape_fn(no_escape);
        TemplateRenderer(handlebars)
    }

    /// Registers a template, first applying `standalone_tags_keep_newline`.
    pub fn register_template_string(&mut self,
                                    name: &str,
                                    template: impl AsRef<str>)
                                    -> result::Result<(), TemplateError> {
        self.0
            .register_template_string(name, standalone_tags_keep_newline(template.as_ref()))
    }

    /// Registers the template in a file, first applying `standalone_tags_keep_newline`.
    pub fn register_template_file(&mut self,
                                  name: &str,
                                  path: impl AsRef<Path>)
                                  -> result::Result<(), TemplateError> {
        let template =
            std::fs::read_to_string(path.as_ref()).map_err(|e| {
                                                      TemplateError::from((e, name.to_string()))
                                                  })?;
        self.register_template_string(name, template)
    }

    pub fn render<T>(&self, template: &str, ctx: &T) -> Result<String>
        where T: Serialize
    {
        let raw = serde_json::to_value(ctx).map_err(Error::RenderContextSerialization)?;
        debug!("Rendering template with context, {}, {}", template, raw);
        self.0
            .render(template, &raw)
            .map_err(|e| Error::TemplateRenderError(format!("{}", e)))
    }
}

/// Keeps the output Dojo's templates were written for when a block tag stands alone on its
/// line with a tilde on one side only, as in
///
/// ```text
/// # a comment
/// {{~#if cfg.enabled}}
/// enabled = true
/// {{~/if}}
/// ```
///
/// Handlebars removes a line that holds nothing but a block tag, its indentation and newline
/// included, and the tilde then also removes the whitespace beside the line, so the lines
/// either side are joined. Templates were written for a renderer that never removed such a
/// line: a tilde on the left removed the whitespace before the tag and kept the newline after
/// it, and a tilde on the right kept the tag's indentation and removed the whitespace after it,
/// so that the next line took the tag's indentation. This rewrites the template so that
/// Handlebars produces that output. A tag with tildes on both sides, or none, is left alone.
fn standalone_tags_keep_newline(template: &str) -> String {
    let mut out = String::with_capacity(template.len());
    // The indentation a right tilde gives the next line that holds more than whitespace.
    let mut carried: Option<String> = None;
    for line in template.split_inclusive('\n') {
        let line = match carried.take() {
            Some(indent) if line.trim().is_empty() => {
                carried = Some(indent);
                continue;
            }
            Some(indent) => indent + line.trim_start(),
            None => line.to_string(),
        };
        match standalone_tilde(&line) {
            Some(Tilde::Left) => {
                out.truncate(out.trim_end().len());
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&line.replacen("{{~", "{{", 1));
            }
            Some(Tilde::Right) => {
                carried = Some(line[..line.len() - line.trim_start().len()].to_string());
                let at = line.rfind("~}}").expect("the tag ends with a tilde");
                out.push_str(&line[..at]);
                out.push_str(&line[at + 1..]);
            }
            None => out.push_str(&line),
        }
    }
    out
}

enum Tilde {
    Left,
    Right,
}

/// Which side the tilde is on, if `line` holds nothing but a block tag with a tilde on one
/// side.
fn standalone_tilde(line: &str) -> Option<Tilde> {
    let inner = line.trim().strip_prefix("{{")?.strip_suffix("}}")?;
    if inner.contains("{{") {
        return None;
    }
    let body = inner.trim_matches('~').trim_start();
    let block = ["#", "^", "/", "!", ">"].iter()
                                         .any(|k| body.starts_with(k))
                || body.trim_end() == "else"
                || body.starts_with("else ");
    match (block, inner.starts_with('~'), inner.ends_with('~')) {
        (true, true, false) => Some(Tilde::Left),
        (true, false, true) => Some(Tilde::Right),
        _ => None,
    }
}

impl fmt::Debug for TemplateRenderer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Handlebars TemplateRenderer")
    }
}

impl Deref for TemplateRenderer {
    type Target = Handlebars<'static>;

    fn deref(&self) -> &Handlebars<'static> { &self.0 }
}

impl DerefMut for TemplateRenderer {
    fn deref_mut(&mut self) -> &mut Handlebars<'static> { &mut self.0 }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{hcore::{fs::{pkg_root_path,
                             FS_ROOT_PATH},
                        package::PackageIdent},
                templating::test_helpers::*};
    use serde_json;
    use std::{collections::BTreeMap,
              env,
              fs::File,
              io::Read,
              path::PathBuf};
    use tempfile::TempDir;
    use toml;

    pub fn root() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests") }

    pub fn fixtures() -> PathBuf { root().join("fixtures") }

    pub fn templates() -> PathBuf { fixtures().join("templates") }

    pub fn sample_configs() -> PathBuf { fixtures().join("sample_configs") }

    pub fn service_config_json_from_toml_file(filename: &str) -> serde_json::Value {
        let mut file = File::open(sample_configs().join(filename)).unwrap();
        let mut config = String::new();
        let _ = file.read_to_string(&mut config).unwrap();
        let toml = toml::de::from_str(&config).unwrap();
        toml_to_json(toml::Value::Table(toml))
    }

    fn toml_vec_to_json(toml: Vec<toml::Value>) -> serde_json::Value {
        serde_json::Value::Array(toml.into_iter().map(toml_to_json).collect())
    }

    // Translates a toml table to a mustache data structure.
    fn toml_table_to_json(toml: toml::value::Table) -> serde_json::Value {
        serde_json::Value::Object(toml.into_iter()
                                      .map(|(k, v)| (k, toml_to_json(v)))
                                      .collect())
    }

    pub fn toml_to_json(value: toml::Value) -> serde_json::Value {
        match value {
            toml::Value::String(s) => serde_json::Value::String(s.to_string()),
            toml::Value::Integer(i) => serde_json::Value::from(i as i64),
            toml::Value::Float(i) => serde_json::Value::from(i as f64),
            toml::Value::Boolean(b) => serde_json::Value::Bool(b),
            toml::Value::Datetime(s) => serde_json::Value::String(s.to_string()),
            toml::Value::Array(a) => toml_vec_to_json(a),
            toml::Value::Table(t) => toml_table_to_json(t),
        }
    }

    #[test]
    fn test_handlebars_json_helper() {
        let content = "{{toJson x}}".to_string();
        let mut data = BTreeMap::new();
        data.insert("test".into(), "something".into());

        let mut renderer = TemplateRenderer::new();
        renderer.register_template_string("t", content).unwrap();

        let mut m: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        m.insert("x".into(), data);

        let r = renderer.render("t", &m);

        assert_eq!(
                   r.ok().unwrap(),
                   r#"{
  "test": "something"
}"#.to_string()
        );
    }

    #[test]
    fn test_handlebars_toml_helper() {
        let content = "{{toToml x}}".to_string();
        let mut data = BTreeMap::new();
        data.insert("test".into(), "something".into());

        let mut renderer = TemplateRenderer::new();
        renderer.register_template_string("t", content).unwrap();

        let mut m: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        m.insert("x".into(), data);

        let r = renderer.render("t", &m);

        assert_eq!(
                   r.ok().unwrap(),
                   r#"test = "something"
"#.to_string()
        );
    }

    fn render_with_cfg(template: &str, cfg: serde_json::Value) -> String {
        let mut renderer = TemplateRenderer::new();
        renderer.register_template_string("t", template).unwrap();
        renderer.render("t", &serde_json::json!({ "cfg": cfg }))
                .unwrap()
    }

    #[test]
    fn a_standalone_block_tag_with_a_left_tilde_keeps_the_lines_apart() {
        let template = "# comment\n\n{{~#if cfg.on}}\non = true\n{{~/if}}\n\nlast\n";
        assert_eq!(render_with_cfg(template, serde_json::json!({ "on": true })),
                   "# comment\non = true\n\nlast\n");
        assert_eq!(render_with_cfg(template, serde_json::json!({ "on": false })),
                   "# comment\n\nlast\n");
    }

    #[test]
    fn standalone_each_with_a_left_tilde_puts_each_member_on_its_own_line() {
        let template = "first\n{{~#each cfg.hosts as |host|}}\nhost {{host}}\n{{~/each}}\nlast";
        assert_eq!(render_with_cfg(template, serde_json::json!({ "hosts": ["a", "b"] })),
                   "first\nhost a\nhost b\nlast");
    }

    #[test]
    fn a_standalone_block_tag_with_a_right_tilde_keeps_the_next_line_indented() {
        let template = "start \\\n  {{#if cfg.on ~}}\n  --on \\\n  {{else ~}}\n  --off \\\n  \
                        {{/if ~}}\n  --last\n";
        assert_eq!(render_with_cfg(template, serde_json::json!({ "on": true })),
                   "start \\\n  --on \\\n  --last\n");
        assert_eq!(render_with_cfg(template, serde_json::json!({ "on": false })),
                   "start \\\n  --off \\\n  --last\n");
    }

    #[test]
    fn a_right_tilde_gives_the_next_line_the_tags_indentation() {
        let template = "# Node Types\n{{#if cfg.leader ~}}\n  master: true\n{{else ~}}\n  \
                        master: false\n{{/if ~}}\n\n  data: true\n";
        assert_eq!(render_with_cfg(template, serde_json::json!({ "leader": false })),
                   "# Node Types\nmaster: false\ndata: true\n");
    }

    #[test]
    fn a_tilde_may_be_spaced_from_the_block_tag() {
        assert_eq!(render_with_cfg("a\n\n{{~ #unless cfg.on }}\nb\n{{~/unless}}\n",
                                   serde_json::json!({ "on": false })),
                   "a\nb\n");
    }

    #[test]
    fn tildes_off_standalone_lines_keep_their_meaning() {
        assert_eq!(render_with_cfg("a\n{{~#if cfg.on~}}\nb\n{{~/if~}}\nc",
                                   serde_json::json!({ "on": true })),
                   "abc");
        assert_eq!(render_with_cfg("a\n {{~cfg.x}}\n", serde_json::json!({ "x": "b" })),
                   "ab\n");
        assert_eq!(render_with_cfg("a {{~#if cfg.on}} b{{/if}}\n",
                                   serde_json::json!({ "on": true })),
                   "a b\n");
    }

    #[test]
    fn test_handlebars_yaml_helper() {
        let content = "{{toYaml x}}".to_string();
        let mut data = BTreeMap::new();
        data.insert("test".into(), "something".into());

        let mut renderer = TemplateRenderer::new();
        renderer.register_template_string("t", content).unwrap();

        let mut m: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        m.insert("x".into(), data);

        let r = renderer.render("t", &m);

        assert_eq!(r.ok().unwrap(), "test: something\n");
    }

    #[test]
    fn to_uppercase_helper() {
        let content = "{{toUppercase var}}".to_string();
        let mut renderer = TemplateRenderer::new();
        renderer.register_template_string("t", content).unwrap();

        let mut m: BTreeMap<String, String> = BTreeMap::new();
        m.insert("var".into(), "value".into());
        let rendered = renderer.render("t", &m).unwrap();
        assert_eq!(rendered, "VALUE".to_string());
    }

    #[test]
    fn to_lowercase_helper() {
        let content = "{{toLowercase var}}".to_string();
        let mut renderer = TemplateRenderer::new();
        renderer.register_template_string("t", content).unwrap();

        let mut m: BTreeMap<String, String> = BTreeMap::new();
        m.insert("var".into(), "VALUE".into());
        let rendered = renderer.render("t", &m).unwrap();
        assert_eq!(rendered, "value".to_string());
    }

    #[test]
    fn str_replace_helper() {
        let content = "{{strReplace var old new}}".to_string();
        let mut renderer = TemplateRenderer::new();
        renderer.register_template_string("t", content).unwrap();

        let mut m: BTreeMap<String, String> = BTreeMap::new();
        m.insert("var".into(), "this is old".into());
        m.insert("old".into(), "old".into());
        m.insert("new".into(), "new".into());
        let rendered = renderer.render("t", &m).unwrap();
        assert_eq!(rendered, "this is new".to_string());
    }

    #[test]
    fn bind_variable() {
        let content = "{{bind.foo.members.[0].sys.ip}}";
        let mut renderer = TemplateRenderer::new();
        let data = service_config_json_from_toml_file("complex_config.toml");

        renderer.register_template_string("t", content).unwrap();

        let rendered = renderer.render("t", &data).unwrap();
        assert_eq!(rendered, "172.17.0.5");
    }

    #[test]
    fn pkg_path_for_helper() {
        let content = "{{pkgPathFor \"karate/acl\"}}".to_string();
        let mut renderer = TemplateRenderer::new();
        renderer.register_template_string("t", content).unwrap();

        let data = service_config_json_from_toml_file("complex_config.toml");
        let rendered = renderer.render("t", &data).unwrap();
        assert_eq!(PathBuf::from(rendered),
                   pkg_root_path(Some(&*FS_ROOT_PATH)).join("karate/acl/2.2.52/20161208223311",));
    }

    #[test]
    fn each_alive_helper_content() {
        let mut renderer = TemplateRenderer::new();
        // template using the new `eachAlive` helper
        renderer.register_template_file("each_alive", templates().join("each_alive.txt"))
                .unwrap();

        // template using an each block with a nested if block filtering on `alive`
        renderer.register_template_file("all_members", templates().join("all_members.txt"))
                .unwrap();

        let data = service_config_json_from_toml_file("multiple_supervisors_config.toml");

        let each_alive_render = renderer.render("each_alive", &data).unwrap();
        let each_if_render = renderer.render("all_members", &data).unwrap();

        assert_eq!(each_alive_render, each_if_render);
    }

    #[test]
    fn each_alive_helper_first_node() {
        let mut renderer = TemplateRenderer::new();
        // template using the new `eachAlive` helper
        renderer.register_template_file("each_alive", templates().join("each_alive.txt"))
                .unwrap();

        // template using an each block with a nested if block filtering on `alive`
        renderer.register_template_file("all_members", templates().join("all_members.txt"))
                .unwrap();

        let data = service_config_json_from_toml_file("one_supervisor_not_started.toml");

        let each_alive_render = renderer.render("each_alive", &data).unwrap();
        let each_if_render = renderer.render("all_members", &data).unwrap();

        assert_eq!(each_alive_render, each_if_render);
    }

    #[test]
    fn each_alive_helper_with_identifier_alias() {
        let mut renderer = TemplateRenderer::new();
        // template using the new `eachAlive` helper
        renderer.register_template_file("each_alive",
                                        templates().join("each_alive_with_identifier.txt"))
                .unwrap();

        // template using an each block with a nested if block filtering on `alive`
        renderer.register_template_file("all_members", templates().join("all_members.txt"))
                .unwrap();

        let data = service_config_json_from_toml_file("multiple_supervisors_config.toml");

        let each_alive_render = renderer.render("each_alive", &data).unwrap();
        let each_if_render = renderer.render("all_members", &data).unwrap();

        assert_eq!(each_alive_render, each_if_render);
    }

    #[test]
    fn render_package_install() {
        let root = TempDir::new().expect("create temp dir").keep();
        env::set_var(fs::FS_ROOT_ENVVAR, &root);
        let pg_id = PackageIdent::new("testing", "test", Some("1.0.0"), Some("20170712000000"));

        let pkg_install =
            PackageInstall::new_from_parts(pg_id.clone(), root.clone(), root.clone(), root.clone());

        let toml_path = root.join("default.toml");
        create_with_content(toml_path, "message = \"Hello\"");
        let hooks_path = root.join("hooks");
        std::fs::create_dir_all(&hooks_path).unwrap();
        create_with_content(hooks_path.join("install"),
                            "install message is {{cfg.message}}");
        let config_path = root.join("config_install");
        std::fs::create_dir_all(&config_path).unwrap();
        create_with_content(config_path.join("config.txt"),
                            "config message is {{cfg.message}}");

        compile_for_package_install(&pkg_install).expect("compile package");

        assert_eq!(
            file_content(fs::svc_config_install_path(&pkg_install.ident().name).join("config.txt")),
            "config message is Hello"
        );
        assert_eq!(file_content(fs::svc_hooks_path(&pkg_install.ident().name).join("install")),
                   "install message is Hello");

        env::remove_var(fs::FS_ROOT_ENVVAR);
        std::fs::remove_dir_all(root).expect("removing temp root");
    }
}
