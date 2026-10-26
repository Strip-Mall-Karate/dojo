// Copyright (c) 2026 Strip Mall Karate Company
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

//! Builds small plans into a Dojo root under the test target directory,
//! with fake installed packages. The plans run `sh`, so these tests run on
//! Unix; Windows builds are checked on a real Windows machine.

#![cfg(unix)]

use std::{collections::BTreeMap,
          fs,
          os::unix::fs::{PermissionsExt,
                         symlink},
          path::{Path,
                 PathBuf},
          process::Command};

use blake2::{Blake2b,
             digest::consts::U32};
use dojo_plan::{Built,
                Options,
                Plan,
                Resolve};
use sha2::{Digest,
           Sha256};

const TARGET: &str = "x86_64-linux";
const RELEASE: &str = "20261025000000";

/// Installed packages by ident.
#[derive(Default)]
struct Packages(BTreeMap<String, PathBuf>);

impl Resolve for Packages {
    fn resolve(&self, ident: &str) -> Result<PathBuf, String> {
        self.0
            .get(ident)
            .cloned()
            .ok_or_else(|| format!("no installed package matches `{ident}`"))
    }
}

/// A fresh directory for one test, holding the Dojo root `doj` and the
/// plan's source directory `context`.
struct Sandbox {
    dir:      PathBuf,
    packages: Packages,
}

impl Sandbox {
    fn new(test: &str) -> Sandbox {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("build")
                                                        .join(test);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("context")).expect("create the sandbox");
        fs::create_dir_all(dir.join("keys")).expect("create the key cache");
        fs::write(dir.join("keys").join(format!("test-{RELEASE}.sig.key")), "")
            .expect("write the key");
        // Signs by copying behind a header, so a test can see what was
        // signed, by whom and with which key cache.
        fs::create_dir_all(dir.join("bin")).expect("create bin");
        let doj = dir.join("bin/doj");
        fs::write(&doj,
                  "#!/bin/sh\n[ \"$1 $2 $3\" = \"pkg sign --origin\" ] || exit 64\n[ -d \
                   \"$DOJ_CACHE_KEY_PATH\" ] || exit 65\n{ echo \"signed by $4\"; cat \"$5\"; } \
                   > \"$6\"\n").expect("write doj");
        fs::set_permissions(&doj, fs::Permissions::from_mode(0o755)).expect("make doj runnable");
        Sandbox { dir,
                  packages: Packages::default() }
    }

    fn context(&self) -> PathBuf { self.dir.join("context") }

    /// Installs an empty package for `ident` (`origin/name`) and returns its
    /// directory.
    fn package(&mut self, ident: &str) -> PathBuf {
        let path = self.dir
                       .join("doj/pkgs")
                       .join(ident)
                       .join("1.0")
                       .join(RELEASE);
        fs::create_dir_all(&path).expect("create the package");
        self.packages.0.insert(ident.to_string(), path.clone());
        path
    }

    fn options(&self) -> Options {
        let mut options = Options::new(TARGET, &self.context(), &self.context());
        options.root = self.dir.join("doj");
        options.release = RELEASE.to_string();
        options.env = vec![("PATH".to_string(), std::env::var("PATH").expect("PATH is set"))];
        options.keys = self.dir.join("keys");
        options.output = self.dir.join("results");
        options.doj = self.dir.join("bin/doj").into();
        options
    }

    fn build(&self, source: &str) -> Result<Built, String> {
        self.build_with(source, &self.options())
    }

    fn build_with(&self, source: &str, options: &Options) -> Result<Built, String> {
        fs::write(options.plan_dir.join("plan.fnl"), source).expect("write the plan");
        let plan = Plan::from_source(source).map_err(|e| format!("load: {e}"))?;
        plan.build(options, &self.packages)
            .map_err(|e| e.to_string())
    }
}

/// A plan named `name` whose phases are `phases`, Fennel key-value pairs,
/// with `build` and `install` doing nothing unless `phases` sets them.
fn plan(name: &str, extra: &str, phases: &str) -> String {
    let mut source = format!("{{:name {name:?} :origin \"test\" :targets [{TARGET:?}] {extra}\n");
    if !extra.contains(":version") {
        source.push_str(" :version \"1.0.0\"\n");
    }
    for (key, default) in [(":build", " :build (fn [b])\n"),
                           (":install", " :install (fn [b])\n")]
    {
        if !phases.contains(key) {
            source.push_str(default);
        }
    }
    source.push_str(phases);
    source.push('}');
    source
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter()
         .fold(String::new(), |hex, b| hex + &format!("{b:02x}"))
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn assert_refused(result: Result<Built, String>, expected: &str) {
    match result {
        Ok(built) => panic!("built {}, but should fail with `{expected}`", built.ident),
        Err(message) => {
            assert!(message.contains(expected),
                    "failed with `{message}`, not `{expected}`");
        }
    }
}

#[test]
fn phases_resolve_from_target_to_plan_to_scaffolding_to_builtin() {
    let mut sandbox = Sandbox::new("chain");
    let scaffolding = sandbox.package("test/scaffolding");
    fs::create_dir_all(scaffolding.join("lib")).unwrap();
    fs::write(scaffolding.join("lib/scaffolding.fnl"),
              r#"{:prepare (fn [b] (b:write "trace/prepare-scaffolding" "") (b.default.prepare b))
                  :build (fn [b] (b:write (.. "trace/build-scaffolding-" (b.scaffolding.tag)) ""))
                  :tag (fn [] "exported")}"#).unwrap();
    let source = plan("chain",
                      r#":scaffolding "test/scaffolding""#,
                      r#" :prepare (fn [b] (b:write "trace/prepare-base" ""))
                          :build (fn [b] (b:write "trace/build-plan" "") (b.default.build b))
                          :on {"x86_64-linux"
                               {:prepare (fn [b]
                                           (b:write "trace/prepare-on" "")
                                           (b.default.prepare b))}}"#);
    let built = sandbox.build(&source).expect("builds");

    let mut trace: Vec<String> = fs::read_dir(built.prefix.join("trace")).unwrap()
                                                                         .map(|entry| {
                                                                             entry.unwrap()
                                                                                  .file_name()
                                                                                  .into_string()
                                                                                  .unwrap()
                                                                         })
                                                                         .collect();
    trace.sort();
    // The target's `prepare` replaces the base one, and `b.default` goes on to
    // the scaffolding, then the builtin.
    assert_eq!(trace,
               ["build-plan",
                "build-scaffolding-exported",
                "prepare-on",
                "prepare-scaffolding"]);
}

#[test]
fn a_scaffolding_adds_metadata_and_reads_the_plans_settings() {
    let mut sandbox = Sandbox::new("scaffold-meta");
    sandbox.package("test/runtime");
    let scaffolding = sandbox.package("test/scaffolding");
    fs::create_dir_all(scaffolding.join("lib")).unwrap();
    // `metadata` sees the plan's metadata, its settings and its directories;
    // phases see the settings on `b`, and defaults the build installs follow
    // the plan's.
    fs::write(scaffolding.join("lib/scaffolding.fnl"),
              r#"{:metadata (fn [plan settings dirs]
                              (let [app (dirs.src.json "package.json")]
                                {:deps ["test/runtime"]
                                 :bin_dirs ["bin" (.. plan.name "/bin")]
                                 :exports {:port "app.port" :host "ignored.host"}
                                 :exposes [(. settings.ports 1)]
                                 :description app.description
                                 :license [(if (dirs.plan.exists "plan.fnl") "MIT" "none")]}))
                  :build (fn [b]
                           (b:write "greeting" b.scaffolding_settings.greeting)
                           (b:write "source" (tostring b.source))
                           (b:write "default.toml" "[extra]\nkey = 1\n"))}"#).unwrap();
    fs::write(sandbox.context().join("default.toml"),
              "[app]\nport = 8000\nhost = \"0.0.0.0\"\n").unwrap();
    fs::write(sandbox.context().join("package.json"),
              r#"{"description": "from package.json"}"#).unwrap();
    let source = plan("scaffolded",
                      r#":scaffolding {:package "test/scaffolding"
                                      :settings {:greeting "hi" :ports ["port"]}}
                         :bin_dirs ["bin"]
                         :exports {:host "app.host"}"#,
                      " :build (fn [b] (b.default.build b))");
    let built = sandbox.build(&source).expect("builds");
    assert_eq!(read(&built.prefix.join("greeting")), "hi");
    assert_eq!(read(&built.prefix.join("source")),
               "nil",
               "no source, no b.source");
    assert_eq!(read(&built.prefix.join("DEPS")).trim(),
               format!("test/runtime/1.0/{RELEASE}"),
               "the scaffolding's run dependency is the package's");
    let exports = read(&built.prefix.join("EXPORTS"));
    assert!(exports.contains("port=app.port") && exports.contains("host=app.host"),
            "the plan's export wins, the scaffolding's adds: {exports}");
    let path = read(&built.prefix.join("PATH"));
    assert!(path.contains("scaffolded/bin"),
            "bin_dirs gained the scaffolding's: {path}");
    assert_eq!(path.matches("/bin").count(),
               2,
               "`bin` once, from both: {path}");
    let manifest = read(&built.prefix.join("MANIFEST"));
    assert!(manifest.contains("from package.json") && manifest.contains("MIT"),
            "description and license from the scaffolding: {manifest}");
    assert_eq!(read(&built.prefix.join("default.toml")),
               "[app]\nport = 8000\nhost = \"0.0.0.0\"\n\n[extra]\nkey = 1\n",
               "the plan's defaults, then the build's");
    assert_eq!(read(&built.prefix.join("EXPOSES")).trim(), "8000");

    for (metadata, expected) in [("{:name \"other\"}", "a scaffolding may set"),
                                 ("{:deps \"test/runtime\"}", "is a string, not a table"),
                                 ("{:exposes [\"nothing\"]}", "`exposes` names `nothing`"),
                                 ("(do (dirs.src.read \"../escape\") {})", "is not a path inside")]
    {
        fs::write(scaffolding.join("lib/scaffolding.fnl"),
                  format!("{{:metadata (fn [plan settings dirs] {metadata})}}")).unwrap();
        assert_refused(sandbox.build(&source), expected);
    }
    let bad = plan("scaffolded",
                   r#":scaffolding {:package "test/scaffolding" :settings {:f (fn [])}}"#,
                   "");
    assert_refused(sandbox.build(&bad), "is a function, not a string, number");
}

#[test]
fn packaging_uses_the_builders_tools_not_a_dependencys() {
    // A run dependency's xz (busybox's only decompresses) is first on the
    // build's PATH; the package is still compressed with the builder's.
    let mut sandbox = Sandbox::new("packager-path");
    let shadow = sandbox.package("test/shadow");
    fs::create_dir_all(shadow.join("bin")).unwrap();
    for tool in ["xz", "tar"] {
        fs::write(shadow.join("bin").join(tool), "#!/bin/sh\nexit 7\n").unwrap();
        fs::set_permissions(shadow.join("bin").join(tool),
                            fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::write(shadow.join("PATH"),
              format!("{}\n", shadow.join("bin").display())).unwrap();
    let source = plan("shadowed",
                      r#":deps ["test/shadow"]"#,
                      r#" :build (fn [b] (b:write "shadowed" (tostring (b:run ["xz"] {:check false}))))"#);
    let built = sandbox.build(&source).expect("builds and packages");
    assert_eq!(read(&built.prefix.join("shadowed")),
               "7",
               "the build itself sees the dependency's xz");
}

#[test]
fn b_write_replaces_a_read_only_file_in_the_source() {
    // Archives ship read-only files (libtool's build-aux/ltmain.sh); the bash
    // plans rewrote them with sed -i, which replaces the file.
    let sandbox = Sandbox::new("write-read-only");
    let source = plan("readonly",
                      "",
                      r#" :build (fn [b]
                                   (b:write (b:join b.src "locked") "before")
                                   (b:chmod (b:join b.src "locked") "444")
                                   (b:write (b:join b.src "locked") "after")
                                   (b:write "seen" (b:read (b:join b.src "locked"))))"#);
    let built = sandbox.build(&source).expect("writes over a read-only file");
    assert_eq!(read(&built.prefix.join("seen")), "after");
}

#[test]
fn stripping_uses_the_builders_strip_not_the_packages() {
    // The package's own bin is on the build's PATH, and binutils installs a
    // strip there that cannot rewrite itself while it runs; the strip phase
    // uses the builder's.
    let sandbox = Sandbox::new("strip-path");
    let source = plan("stripped",
                      r#":bin_dirs ["bin"]"#,
                      r##" :install (fn [b]
                                     (b:mkdir (.. b.prefix "/bin"))
                                     (b:write (.. b.prefix "/bin/strip") "#!/bin/sh\nexit 7\n")
                                     (b:chmod (.. b.prefix "/bin/strip") "755")
                                     (b:run ["cp" "/bin/true" (.. b.prefix "/bin/true")]))"##);
    let built = sandbox.build(&source).expect("strips with the builder's strip");
    assert!(built.prefix.join("bin/true").exists());
}

#[test]
fn a_version_function_runs_with_b_and_sets_the_prefix() {
    let sandbox = Sandbox::new("version");
    let source = plan("versioned",
                      r#":version (fn [b]
                                    (b:env "GREETING" "hello")
                                    (b:capture ["sh" "-c" "echo 1.2.3"]))"#,
                      r#" :build (fn [b]
                                   (b:env_append "GREETING" "world")
                                   (b:write "out" (.. b.version " "
                                                      (b:capture ["sh" "-c" "echo \"$GREETING\""]))))"#);
    let built = sandbox.build(&source).expect("builds");
    assert_eq!(built.ident, format!("test/versioned/1.2.3/{RELEASE}"));
    assert_eq!(built.prefix,
               sandbox.dir
                      .join("doj/pkgs/test/versioned/1.2.3")
                      .join(RELEASE));
    assert_eq!(read(&built.prefix.join("out")), "1.2.3 hello:world");
}

#[test]
fn a_version_with_whitespace_is_refused() {
    let sandbox = Sandbox::new("bad-version");
    assert_refused(sandbox.build(&plan("v", r#":version (fn [b] "1 2")"#, "")),
                   "the version `1 2`");
    assert_refused(sandbox.build(&plan("v", ":version (fn [b] 12)", "")),
                   "returned a value of type integer, not a string");
}

#[test]
fn a_failing_program_names_the_phase_argv_and_code() {
    let sandbox = Sandbox::new("exit-code");
    let source = plan("failing",
                      "",
                      r#" :build (fn [b] (b:run ["sh" "-c" "exit 3"]))"#);
    assert_refused(sandbox.build(&source),
                   r#"the `build` phase failed: `sh -c "exit 3"` exited with code 3"#);
}

#[test]
fn path_for_finds_declared_dependencies_only() {
    let mut sandbox = Sandbox::new("path-for");
    let zlib = sandbox.package("test/zlib");
    let cc = sandbox.package("test/cc");
    let source = plan("paths",
                      r#":deps ["test/zlib"] :build_deps ["test/cc"]"#,
                      r#" :build (fn [b]
                                   (b:write "zlib" (b:path_for "zlib" "lib" "libz.so"))
                                   (b:write "cc" (b:path_for "test/cc")))"#);
    let built = sandbox.build(&source).expect("builds");
    assert_eq!(read(&built.prefix.join("zlib")),
               zlib.join("lib/libz.so").to_str().unwrap());
    assert_eq!(read(&built.prefix.join("cc")), cc.to_str().unwrap());

    let undeclared = plan("paths",
                          r#":deps ["test/zlib"]"#,
                          r#" :build (fn [b] (b:path_for "cc"))"#);
    assert_refused(sandbox.build(&undeclared),
                   "b:path_for: `cc` is not a declared dependency");

    sandbox.package("other/zlib");
    let ambiguous = plan("paths",
                         r#":deps ["test/zlib"] :build_deps ["other/zlib"]"#,
                         r#" :build (fn [b] (b:path_for "zlib"))"#);
    assert_refused(sandbox.build(&ambiguous),
                   "`zlib` matches test/zlib, other/zlib");
    let absolute = plan("paths",
                        r#":deps ["test/zlib"]"#,
                        r#" :build (fn [b] (b:path_for "zlib" "/etc"))"#);
    assert_refused(sandbox.build(&absolute), "`/etc` is absolute");
}

#[test]
fn a_missing_dependency_stops_the_build_before_plan_code_runs() {
    let sandbox = Sandbox::new("missing-dep");
    let source = plan("missing",
                      r#":deps ["test/nowhere"] :version (fn [b] (b:write "x" "") "1")"#,
                      "");
    assert_refused(sandbox.build(&source),
                   "no installed package matches `test/nowhere`");
}

#[test]
fn writes_stay_inside_the_prefix_and_source() {
    let sandbox = Sandbox::new("writes");
    let outside = sandbox.dir.join("outside");
    fs::create_dir_all(&outside).unwrap();
    symlink(&outside, sandbox.context().join("link")).unwrap();

    let cases = [(r#"(b:write "/etc/dojo-test" "")"#, "is outside the prefix"),
                 (r#"(b:write "../escape" "")"#, "is outside the prefix"),
                 (r#"(b:mkdir (b:join b.src "link" "inner"))"#, "through a symbolic link"),
                 (r#"(b:write (b:join b.src "link") "")"#, "through a symbolic link")];
    for (call, expected) in cases {
        let source = plan("writes", "", &format!(" :build (fn [b] {call})"));
        assert_refused(sandbox.build(&source), expected);
    }
    assert!(fs::read_dir(&outside).unwrap().next().is_none(),
            "nothing was written through the link");

    let allowed = plan("writes",
                       "",
                       r#" :build (fn [b]
                                    (b:mkdir "etc/conf.d")
                                    (b:write (b:join b.src "generated.txt") "made"))"#);
    let built = sandbox.build(&allowed).expect("builds");
    assert!(built.prefix.join("etc/conf.d").is_dir());
    assert_eq!(read(&sandbox.context().join("generated.txt")), "made");
}

#[test]
fn a_runaway_phase_stops_at_its_budget() {
    let sandbox = Sandbox::new("runaway");
    let source = plan("runaway", "", " :build (fn [b] (while true nil))");
    assert_refused(sandbox.build(&source), "the plan ran past its budget");
}

#[test]
fn each_phase_gets_its_own_budget() {
    // Each loop runs most of a phase's 20 million instructions; together
    // they would pass one shared budget.
    let sandbox = Sandbox::new("budget");
    let busy = "(fn [b] (for [i 1 15000000] nil))";
    let source = plan("busy", "", &format!(" :build {busy} :install {busy}"));
    sandbox.build(&source).expect("builds");
}

#[test]
fn b_is_called_with_a_colon_and_defaults_only_in_their_phase() {
    let sandbox = Sandbox::new("misuse");
    assert_refused(sandbox.build(&plan("misuse", "", r#" :build (fn [b] (b.run ["true"]))"#)),
                   "call b:run with a colon");
    assert_refused(sandbox.build(&plan("misuse", "", " :build (fn [b] (b.default.install b))")),
                   "b.default.install can only run during the `install` phase");
    assert_refused(sandbox.build(&plan("misuse", "", " :build (fn [b] (b.default.build {}))")),
                   "pass b to b.default.build");
    assert_refused(sandbox.build(&plan("misuse",
                                       "",
                                       r#" :build (fn [b] (b:run ["true"] {:shell true}))"#)),
                   "b:run: unknown option `shell`");
}

#[test]
fn check_runs_only_when_asked() {
    let sandbox = Sandbox::new("check");
    let source = plan("checked", "", r#" :check (fn [b] (b:write "checked" ""))"#);
    let built = sandbox.build(&source).expect("builds");
    assert!(!built.prefix.join("checked").exists());

    let mut options = sandbox.options();
    options.check = true;
    let built = sandbox.build_with(&source, &options).expect("builds");
    assert!(built.prefix.join("checked").exists());
}

#[test]
fn glob_and_install_copy_matching_files_with_a_mode() {
    let sandbox = Sandbox::new("install");
    let dist = sandbox.context().join("dist");
    fs::create_dir_all(&dist).unwrap();
    for name in ["b.dll", "a.dll", "notes.txt"] {
        fs::write(dist.join(name), name).unwrap();
    }
    let source = plan("installer",
                      r#":bin_dirs ["bin"]"#,
                      r#" :install (fn [b]
                                     (let [dlls (b:glob "dist" "*.dll")]
                                       (b:write "globbed" (table.concat dlls ","))
                                       (b:install dlls "bin" {:mode "750"})))"#);
    let built = sandbox.build(&source).expect("builds");
    let dist = dist.to_str().unwrap();
    assert_eq!(read(&built.prefix.join("globbed")),
               format!("{dist}/a.dll,{dist}/b.dll"));
    let mode = fs::metadata(built.prefix.join("bin/a.dll")).unwrap()
                                                           .permissions()
                                                           .mode();
    assert_eq!(mode & 0o7777, 0o750);
    assert!(!built.prefix.join("bin/notes.txt").exists());

    let bad_mode = plan("installer",
                        "",
                        r#" :install (fn [b] (b:install "dist/a.dll" "bin" {:mode "rwx"}))"#);
    assert_refused(sandbox.build(&bad_mode), "not octal permissions");
    let escape = plan("installer",
                      "",
                      r#" :install (fn [b] (b:install "dist/a.dll" "../bin"))"#);
    assert_refused(sandbox.build(&escape),
                   "must be a relative path inside the prefix");
}

#[test]
fn a_source_is_downloaded_verified_and_unpacked() {
    let sandbox = Sandbox::new("source");
    let upstream = sandbox.dir.join("upstream");
    fs::create_dir_all(upstream.join("hello-2.0")).unwrap();
    fs::write(upstream.join("hello-2.0/hello.txt"), "hi from the tarball").unwrap();
    let status = Command::new("tar").args(["czf", "hello-2.0.tar.gz", "hello-2.0"])
                                    .current_dir(&upstream)
                                    .status()
                                    .expect("run tar");
    assert!(status.success());
    let tarball = upstream.join("hello-2.0.tar.gz");
    let sha = hex(&Sha256::digest(fs::read(&tarball).unwrap()));
    let with_sha = |sha: &str| {
        plan("hello",
             &format!(r#":version "2.0" :source {{:url "file://{}" :sha256 "{sha}"}}"#,
                      tarball.display()),
             r#" :build (fn [b] (b:write (b:join b.src "built") (b:capture ["cat" "hello.txt"])))"#)
    };

    let built = sandbox.build(&with_sha(&sha)).expect("builds");
    let src = sandbox.dir.join("doj/cache/src/hello-2.0");
    assert_eq!(read(&src.join("built")), "hi from the tarball");
    assert!(sandbox.dir.join("doj/cache/src/hello-2.0.tar.gz").is_file());
    assert!(built.prefix.is_dir());

    // A cached file that no longer matches is fetched again; a download that
    // does not match fails verification.
    let wrong = "0".repeat(64);
    assert_refused(sandbox.build(&with_sha(&wrong)),
                   &format!("has SHA-256 {sha}, but `source.sha256` is {wrong}"));

    // A source directory may be inside what the archive unpacks, and `b.source`
    // tells phases what the plan downloaded.
    fs::create_dir_all(upstream.join("hello-2.0/sub")).unwrap();
    fs::write(upstream.join("hello-2.0/sub/inner.txt"), "inner").unwrap();
    let status = Command::new("tar").args(["czf", "hello-2.0.tar.gz", "hello-2.0"])
                                    .current_dir(&upstream)
                                    .status()
                                    .expect("run tar");
    assert!(status.success());
    let sha = hex(&Sha256::digest(fs::read(&tarball).unwrap()));
    let nested = plan("hello",
                      &format!(r#":version "2.0" :source {{:url "file://{}" :sha256 "{sha}" :dirname "hello-2.0/sub"}}"#,
                               tarball.display()),
                      r#" :build (fn [b]
                                   (b:write "inner" (b:capture ["cat" "inner.txt"]))
                                   (b:write "source" (.. b.source.filename " " b.source.dirname " " b.source.sha256)))"#);
    let built = sandbox.build(&nested).expect("builds");
    assert_eq!(read(&built.prefix.join("inner")), "inner");
    assert_eq!(read(&built.prefix.join("source")),
               format!("hello-2.0.tar.gz hello-2.0/sub {sha}"));
    for dirname in ["../escape", "/abs", "a//b"] {
        let bad = plan("hello",
                       &format!(r#":version "2.0" :source {{:url "file://{}" :sha256 "{sha}" :dirname "{dirname}"}}"#,
                                tarball.display()),
                       "");
        assert_refused(sandbox.build(&bad),
                       "must be a relative path inside the cache");
    }
}

#[test]
fn dependencies_set_path_and_compiler_flags() {
    let mut sandbox = Sandbox::new("environment");
    let tool = sandbox.package("test/tool");
    fs::create_dir_all(tool.join("bin")).unwrap();
    fs::write(tool.join("bin/greet"),
              "#!/bin/sh\necho greetings from the tool\n").unwrap();
    fs::set_permissions(tool.join("bin/greet"), fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(tool.join("PATH"),
              format!("{}:/usr/local/not-the-tool\n", tool.join("bin").display())).unwrap();
    fs::write(tool.join("CFLAGS"),
              format!("-I{}/include\n", tool.display())).unwrap();

    let source = plan("environment",
                      r#":build_deps ["test/tool"]"#,
                      r#" :build (fn [b]
                                   (b:write "greet" (b:capture ["greet"]))
                                   (b:write "path" (b:capture ["sh" "-c" "echo \"$PATH\""]))
                                   (b:write "flags" (b:capture ["sh" "-c" "echo \"$CFLAGS|$CPPFLAGS|$PREFIX\""])))"#);
    let built = sandbox.build(&source).expect("builds");
    assert_eq!(read(&built.prefix.join("greet")), "greetings from the tool");
    let path = read(&built.prefix.join("path"));
    assert!(path.starts_with(&format!("{}:", tool.join("bin").display())),
            "PATH starts with the tool: {path}");
    assert!(!path.contains("not-the-tool"),
            "PATH entries outside the dependency are dropped: {path}");
    let include = format!("-I{}/include", tool.display());
    assert_eq!(read(&built.prefix.join("flags")),
               format!("{include}|{include}|{}", built.prefix.display()));
}

#[test]
fn a_target_the_plan_does_not_list_is_refused() {
    let sandbox = Sandbox::new("target");
    let mut options = sandbox.options();
    options.target = "aarch64-linux".to_string();
    assert_refused(sandbox.build_with(&plan("t", "", ""), &options),
                   "the plan does not build for `aarch64-linux`");
}

/// The files directly in `dir`, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> =
        fs::read_dir(dir).unwrap()
                         .map(|e| e.unwrap().file_name().into_string().unwrap())
                         .collect();
    names.sort();
    names
}

/// A package built from a plan that sets every metadata key, with a run
/// dependency that has its own, and a run hook, config and default.toml.
struct Packaged {
    sandbox: Sandbox,
    built:   Built,
    source:  String,
    /// The direct run dependency.
    lib:     PathBuf,
}

fn packaged(test: &str) -> Packaged {
    let mut sandbox = Sandbox::new(test);
    let base = sandbox.package("test/base");
    fs::write(base.join("IDENT"), format!("test/base/1.0/{RELEASE}\n")).unwrap();
    fs::write(base.join("RUNTIME_ENVIRONMENT"),
              "GEM_PATH=/base/gems\nJAVA_HOME=/base\n").unwrap();
    let lib = sandbox.package("test/lib");
    fs::write(lib.join("IDENT"), format!("test/lib/1.0/{RELEASE}\n")).unwrap();
    fs::write(lib.join("TDEPS"), format!("test/base/1.0/{RELEASE}\n")).unwrap();
    fs::write(lib.join("PATH"),
              format!("{}:/usr/elsewhere\n", lib.join("bin").display())).unwrap();
    fs::write(lib.join("RUNTIME_ENVIRONMENT"),
              "GEM_PATH=/lib/gems\nJAVA_HOME=/lib\nPATH=/ignored\n").unwrap();
    let context = sandbox.context();
    fs::create_dir_all(context.join("hooks")).unwrap();
    fs::write(context.join("hooks/run"), "#!/bin/sh\nexec tool\n").unwrap();
    fs::create_dir_all(context.join("config/sub")).unwrap();
    fs::write(context.join("config/sub/app.toml"), "x = 1\n").unwrap();
    fs::write(context.join("config/app.toml.bak"), "old\n").unwrap();
    fs::write(context.join("default.toml"), "[http]\nport = 8080\n").unwrap();

    let source = plan("pkg",
                      r#":deps ["test/base" "test/lib"]
                         :description "A package." :license ["MIT" "Apache-2.0"]
                         :bin_dirs ["bin"] :lib_dirs ["lib"] :include_dirs ["include"]
                         :interpreters ["bin/tool"] :svc {:run "tool"}
                         :exports {:port "http.port"} :exposes ["port"]
                         :binds {:db ["port" "host"]}"#,
                      r##" :install (fn [b]
                                     (b:write "bin/tool" "#!/bin/sh\n")
                                     (b:write "env" (b:capture ["sh" "-c" "echo \"$GEM_PATH|$JAVA_HOME\""])))"##);
    let built = sandbox.build(&source).expect("builds");
    Packaged { sandbox,
               built,
               source,
               lib }
}

#[test]
fn metadata_files_describe_the_package_and_its_dependencies() {
    let Packaged { built, lib, .. } = packaged("metadata");
    let prefix = &built.prefix;
    let ident = format!("test/pkg/1.0.0/{RELEASE}");
    // Run dependencies' environments reach the build: lists join, the rest
    // take the last dependency's value.
    assert_eq!(read(&prefix.join("env")), "/lib/gems:/base/gems|/lib");

    assert_eq!(names(prefix),
               ["BINDS",
                "CFLAGS",
                "CPPFLAGS",
                "CXXFLAGS",
                "DEPS",
                "EXPORTS",
                "EXPOSES",
                "FILES",
                "IDENT",
                "INTERPRETERS",
                "LDFLAGS",
                "LD_RUN_PATH",
                "MANIFEST",
                "PATH",
                "RUNTIME_ENVIRONMENT",
                "RUNTIME_ENVIRONMENT_PROVENANCE",
                "RUNTIME_PATH",
                "SVC_GROUP",
                "SVC_USER",
                "TARGET",
                "TDEPS",
                "bin",
                "config",
                "default.toml",
                "env",
                "hooks",
                "include",
                "lib"]);
    let p = prefix.display();
    let expected = [("IDENT", format!("{ident}\n")),
                    ("TARGET", format!("{TARGET}\n")),
                    ("DEPS", format!("test/base/1.0/{RELEASE}\ntest/lib/1.0/{RELEASE}\n")),
                    ("TDEPS", format!("test/base/1.0/{RELEASE}\ntest/lib/1.0/{RELEASE}\n")),
                    ("PATH", format!("{p}/bin\n")),
                    ("RUNTIME_PATH", format!("{p}/bin:{}\n", lib.join("bin").display())),
                    ("LD_RUN_PATH", format!("{p}/lib\n")),
                    ("LDFLAGS", format!("-L{p}/lib\n")),
                    ("CFLAGS", format!("-I{p}/include\n")),
                    ("INTERPRETERS", format!("{p}/bin/tool\n")),
                    ("EXPORTS", "port=http.port\n".to_string()),
                    ("EXPOSES", "8080\n".to_string()),
                    ("BINDS", "db=port host\n".to_string()),
                    ("SVC_USER", "doj\n".to_string()),
                    ("SVC_GROUP", "doj\n".to_string()),
                    ("RUNTIME_ENVIRONMENT",
                     format!("GEM_PATH=/lib/gems:/base/gems\nJAVA_HOME=/lib\nPATH={p}/bin:{}\n",
                             lib.join("bin").display())),
                    ("RUNTIME_ENVIRONMENT_PROVENANCE",
                     format!("GEM_PATH=test/lib/1.0/{RELEASE} \
                              test/base/1.0/{RELEASE}\nJAVA_HOME=test/lib/1.0/{RELEASE}\n"))];
    for (file, contents) in expected {
        assert_eq!(read(&prefix.join(file)), contents, "{file}");
    }
    assert_eq!(names(&prefix.join("config")),
               ["sub"],
               "backup files stay out");
    assert_eq!(read(&prefix.join("hooks/run")), "#!/bin/sh\nexec tool\n");
    assert!(!prefix.join("run").exists(),
            "a run hook replaces the run script");
}

#[test]
fn files_and_manifest_list_the_package() {
    let Packaged { built, source, .. } = packaged("manifest");
    let prefix = &built.prefix;
    let p = prefix.display();
    let ident = format!("test/pkg/1.0.0/{RELEASE}");
    // FILES lists every file but itself and MANIFEST, signed.
    let files = read(&prefix.join("FILES"));
    let mut lines = files.lines();
    assert_eq!(lines.next(), Some("signed by test"));
    let listed: Vec<&str> = lines.collect();
    let ident_line = format!("{}  {p}/IDENT",
                             hex(&Blake2b::<U32>::digest(format!("{ident}\n"))));
    assert!(listed.contains(&ident_line.as_str()),
            "{ident_line} in {listed:?}");
    assert_eq!(listed.len(), 24, "{listed:?}");
    assert!(!files.contains("/MANIFEST") && !files.contains("/FILES"));

    let manifest = read(&prefix.join("MANIFEST"));
    for line in ["# test / pkg\nA package.\n",
                 "* __Release__: 20261025000000\n",
                 "* __License__: MIT Apache-2.0 \n",
                 "* __Dependencies__: `test/base test/lib `\n",
                 "* __Build Dependencies__: no build dependencies or undefined\n",
                 "* __Source__: source URL not provided or required\n",
                 "CFLAGS: no CFLAGS\n"]
    {
        assert!(manifest.contains(line),
                "MANIFEST has {line:?}:\n{manifest}");
    }
    assert!(manifest.ends_with(&format!("## Plan Source\n\n```fennel\n{source}\n```\n")));
}

#[test]
fn the_artifact_is_the_prefix_tarred_compressed_and_signed() {
    let Packaged { sandbox, built, .. } = packaged("artifact");
    let p = built.prefix.display();
    let ident = format!("test/pkg/1.0.0/{RELEASE}");
    let artifact = &built.artifact;
    assert_eq!(artifact.path,
               sandbox.dir
                      .join(format!("results/test-pkg-1.0.0-{RELEASE}-{TARGET}.hart")));
    let hart = fs::read(&artifact.path).unwrap();
    assert_eq!(hex(&Sha256::digest(&hart)), artifact.sha256);
    let body = hart.strip_prefix(b"signed by test\n".as_slice())
                   .expect("signed");
    let xz = sandbox.dir.join("body.tar.xz");
    fs::write(&xz, body).unwrap();
    let listing = Command::new("sh").arg("-c")
                                    .arg(format!("xz -dc '{}' | tar -tf -", xz.display()))
                                    .output()
                                    .unwrap();
    let listing = String::from_utf8(listing.stdout).unwrap();
    let member = format!("{}/bin/tool", p.to_string().trim_start_matches('/'));
    assert!(listing.lines().any(|line| line == member),
            "{member} in:\n{listing}");

    let report = read(&sandbox.dir.join("results/last_build.env"));
    let head = ["pkg_origin=test".to_string(),
                "pkg_name=pkg".to_string(),
                "pkg_version=1.0.0".to_string(),
                format!("pkg_release={RELEASE}"),
                format!("pkg_target={TARGET}"),
                format!("pkg_ident={ident}")];
    assert!(report.starts_with(&(head.join("\n") + "\n")), "{report}");
    assert!(report.contains(&format!("pkg_sha256sum={}\n", artifact.sha256)));
    assert!(report.contains(&format!("pkg_blake2bsum={}\n", artifact.blake2b)));
    assert!(!sandbox.dir.join("results/pre_build.env").exists());
}

#[test]
fn a_service_without_a_run_hook_gets_a_run_script() {
    let sandbox = Sandbox::new("service");
    let source = plan("svc", r#":svc {:run "tool --serve" :user "web"}"#, "");
    let built = sandbox.build(&source).expect("builds");
    let run = read(&built.prefix.join("run"));
    assert!(run.contains("    -U web:doj \\\n    -u web:doj \\\n    tool --serve 2>&1\n"),
            "{run}");
    assert!(run.contains(&format!("export HOME={}\n",
                                  sandbox.dir.join("doj/svc/svc/data").display())));
    assert_eq!(read(&built.prefix.join("SVC_USER")), "web\n");
    assert_eq!(read(&built.prefix.join("SVC_GROUP")), "doj\n");
}

#[test]
fn a_missing_signing_key_stops_the_build_before_plan_code_runs() {
    let sandbox = Sandbox::new("key");
    fs::remove_dir_all(sandbox.dir.join("keys")).unwrap();
    fs::create_dir_all(sandbox.dir.join("keys")).unwrap();
    fs::write(sandbox.dir.join("keys/other-20261025000000.sig.key"), "").unwrap();
    let source = plan("key", "", r#" :build (fn [b] (b:write "built" ""))"#);
    assert_refused(sandbox.build(&source), "no signing key for origin `test`");
    assert!(!sandbox.dir.join("doj/pkgs/test/key").exists());
}

#[test]
fn hooks_with_windows_line_endings_are_refused() {
    let sandbox = Sandbox::new("crlf");
    fs::create_dir_all(sandbox.context().join("hooks")).unwrap();
    fs::write(sandbox.context().join("hooks/init"),
              "#!/bin/sh\r\ntrue\r\n").unwrap();
    assert_refused(sandbox.build(&plan("crlf", "", "")), "CR+LF line endings");
}

#[test]
fn file_operations_work_inside_the_build() {
    let sandbox = Sandbox::new("file-ops");
    let source = plan("files",
                      "",
                      r#" :build (fn [b]
                                   (b:write (b:join b.src "tree" "a.txt") "alpha")
                                   (b:write (b:join b.src "tree" "deep" "b.txt") "beta")
                                   (b:write (b:join b.src "config.in") "PREFIX=@prefix@\n")
                                   (b:copy "tree" "share/tree")
                                   (b:copy "tree/a.txt" "share/a.txt")
                                   (b:move "share/a.txt" "share/moved.txt")
                                   (b:symlink "moved.txt" "share/link.txt")
                                   (b:chmod "share/moved.txt" "640")
                                   (let [text (b:read "config.in")
                                         (fixed _) (text:gsub "@prefix@" "/doj/pkgs/x")]
                                     (b:write "etc/config" fixed))
                                   (b:write "found"
                                            (table.concat (b:find (b:join b.prefix "share")
                                                                  "*.txt")
                                                          "\n"))
                                   (b:write "exists"
                                            (.. (tostring (b:exists "tree/a.txt")) " "
                                                (tostring (b:exists "tree/nowhere"))))
                                   (b:remove "share/tree/deep")
                                   (b:remove "share/nothing-here"))"#);
    let built = sandbox.build(&source).expect("builds");
    let prefix = &built.prefix;
    assert_eq!(read(&prefix.join("share/tree/a.txt")), "alpha");
    assert!(!prefix.join("share/tree/deep").exists(),
            "the subtree was removed");
    assert!(!prefix.join("share/a.txt").exists(),
            "the file was moved away");
    assert_eq!(read(&prefix.join("share/moved.txt")), "alpha");
    assert_eq!(fs::read_link(prefix.join("share/link.txt")).unwrap(),
               Path::new("moved.txt"));
    assert_eq!(fs::metadata(prefix.join("share/moved.txt")).unwrap()
                                                           .permissions()
                                                           .mode()
               & 0o777,
               0o640);
    assert_eq!(read(&prefix.join("etc/config")), "PREFIX=/doj/pkgs/x\n");
    let share = prefix.join("share");
    let expected: Vec<String> =
        ["link.txt", "moved.txt", "tree/a.txt", "tree/deep/b.txt"].iter()
                                                                  .map(|p| {
                                                                      share.join(p)
                                                                           .to_str()
                                                                           .unwrap()
                                                                           .to_string()
                                                                  })
                                                                  .collect();
    assert_eq!(read(&prefix.join("found")), expected.join("\n"));
    assert_eq!(read(&prefix.join("exists")), "true false");
}

#[test]
fn file_operations_stay_inside_the_build() {
    let sandbox = Sandbox::new("file-op-bounds");
    let outside = sandbox.dir.join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("secret"), "kept").unwrap();
    let outside = outside.to_str().unwrap();
    let cases = [(format!(r#"(b:read "{outside}/secret")"#), "b:read: "),
                 (format!(r#"(b:exists "{outside}/secret")"#), "b:exists: "),
                 (format!(r#"(b:find "{outside}" "*")"#), "b:find: "),
                 (format!(r#"(b:copy "{outside}/secret" "secret")"#), "b:copy: "),
                 (format!(r#"(b:remove "{outside}/secret")"#), "b:remove: "),
                 (format!(r#"(b:move "{outside}/secret" "secret")"#), "b:move: "),
                 (r#"(b:copy "/etc/hosts" "hosts")"#.to_string(),
                  "is outside the prefix, the source"),
                 (r#"(b:write "x" "") (b:copy "x" "/etc/dojo-test")"#.to_string(),
                  "is outside the prefix ("),
                 (r#"(b:symlink "/etc" "../escape")"#.to_string(), "is outside the prefix"),
                 (r#"(b:read "../../plan.fnl")"#.to_string(), "is outside the prefix, the source"),
                 (r#"(b:read "/../../../etc/hosts")"#.to_string(), "climbs above the root"),
                 (r#"(b:write "x" "") (b:chmod "x" "9")"#.to_string(), "not octal permissions")];
    for (call, expected) in cases {
        let source = plan("bounds", "", &format!(" :build (fn [b] {call})"));
        assert_refused(sandbox.build(&source), expected);
    }
    assert_eq!(read(&sandbox.dir.join("outside/secret")), "kept");
}

#[test]
fn fix_interpreter_points_matching_shebangs_at_the_dependency() {
    let mut sandbox = Sandbox::new("fix-interpreter");
    let coreutils = sandbox.package("test/coreutils");
    fs::create_dir_all(coreutils.join("bin")).unwrap();
    fs::write(coreutils.join("bin/env"), "").unwrap();
    let source = plan("scripts",
                      r#":deps ["test/coreutils"]"#,
                      r##" :build (fn [b]
                                   (b:write "bin/tool" "#!/usr/bin/env python -u\nprint(1)\n")
                                   (b:write "bin/other" "#!/bin/sh\necho\n")
                                   (b:write "bin/plain" "no shebang\n")
                                   (b:fix_interpreter (b:glob (b:join b.prefix "bin") "*")
                                                      "coreutils" "bin/env"))"##);
    let built = sandbox.build(&source).expect("builds");
    let env = coreutils.join("bin/env");
    assert_eq!(read(&built.prefix.join("bin/tool")),
               format!("#!{} python -u\nprint(1)\n", env.display()));
    assert_eq!(read(&built.prefix.join("bin/other")), "#!/bin/sh\necho\n");
    assert_eq!(read(&built.prefix.join("bin/plain")), "no shebang\n");

    let missing = plan("scripts",
                       r#":deps ["test/coreutils"]"#,
                       r#" :build (fn [b] (b:fix_interpreter [] "coreutils" "bin/python"))"#);
    assert_refused(sandbox.build(&missing), "bin/python is not a file");
}

#[test]
fn download_checks_its_checksum() {
    let sandbox = Sandbox::new("download");
    let file = sandbox.dir.join("extra.txt");
    fs::write(&file, "extra source").unwrap();
    let url = format!("file://{}", file.display());
    let sha = hex(&Sha256::digest(b"extra source"));
    let good = plan("fetch",
                    "",
                    &format!(r#" :build (fn [b]
                                   (b:copy (b:download "{url}" "{sha}" "extra.txt") "extra"))"#));
    let built = sandbox.build(&good).expect("builds");
    assert_eq!(read(&built.prefix.join("extra")), "extra source");

    let wrong = "0".repeat(64);
    let bad = plan("fetch",
                   "",
                   &format!(r#" :build (fn [b] (b:download "{url}" "{wrong}" "wrong.txt"))"#));
    assert_refused(sandbox.build(&bad), "has SHA-256");
    let path = plan("fetch",
                    "",
                    &format!(r#" :build (fn [b] (b:download "{url}" "{sha}" "../x"))"#));
    assert_refused(sandbox.build(&path), "is a path, not a file name");
}

#[test]
fn a_plan_passes_its_own_environment_on() {
    let mut sandbox = Sandbox::new("exports");
    let dep = sandbox.package("test/python");
    fs::write(dep.join("RUNTIME_ENVIRONMENT"),
              "PYTHONPATH=/dep/lib\nCLASSPATH=/dep/a.jar\n").unwrap();
    fs::write(dep.join("IDENT"), "test/python/1.0/20261025000000\n").unwrap();
    let source = plan("exports",
                      r#":deps ["test/python"]"#,
                      r#" :build (fn [b]
                                   (b:runtime_env_push "PYTHONPATH" (b:join b.prefix "lib"))
                                   (b:runtime_env_push "CLASSPATH" (b:join b.prefix "b.jar"))
                                   (b:runtime_env "TOOL_HOME" b.prefix)
                                   (b:buildtime_env "TOOL_FLAGS" "-O2")
                                   (b:write "seen" (b:capture ["sh" "-c" "echo $PYTHONPATH $TOOL_FLAGS"])))"#);
    let built = sandbox.build(&source).expect("builds");
    let prefix = built.prefix.to_str().unwrap();
    let runtime = read(&built.prefix.join("RUNTIME_ENVIRONMENT"));
    assert!(runtime.contains(&format!("PYTHONPATH={prefix}/lib:/dep/lib\n")),
            "{runtime}");
    assert!(runtime.contains(&format!("CLASSPATH={prefix}/b.jar;/dep/a.jar\n")),
            "{runtime}");
    assert!(runtime.contains(&format!("TOOL_HOME={prefix}\n")),
            "{runtime}");
    assert!(!runtime.contains("TOOL_FLAGS"), "{runtime}");
    let provenance = read(&built.prefix.join("RUNTIME_ENVIRONMENT_PROVENANCE"));
    assert!(provenance.contains(&format!("PYTHONPATH={} test/python/1.0/20261025000000\n",
                                         built.ident)),
            "{provenance}");
    assert!(provenance.contains(&format!("TOOL_HOME={}\n", built.ident)),
            "{provenance}");
    assert_eq!(read(&built.prefix.join("BUILDTIME_ENVIRONMENT")),
               "TOOL_FLAGS=-O2\n");
    assert_eq!(read(&built.prefix.join("seen")),
               format!("{prefix}/lib:/dep/lib -O2"));
}

#[test]
fn getenv_reads_the_build_environment() {
    let sandbox = Sandbox::new("getenv");
    let source = plan("getenv",
                      "",
                      r#" :build (fn [b]
                                   (b:env "FLAGS" "-O2")
                                   (b:env "FLAGS" (.. (b:getenv "FLAGS") " -g"))
                                   (b:write "flags" (b:getenv "FLAGS"))
                                   (b:write "unset" (tostring (b:getenv "NOT_SET_ANYWHERE"))))"#);
    let built = sandbox.build(&source).expect("builds");
    assert_eq!(read(&built.prefix.join("flags")), "-O2 -g");
    assert_eq!(read(&built.prefix.join("unset")), "nil");
}

#[test]
fn a_service_with_a_run_hook_names_its_user_without_run() {
    let sandbox = Sandbox::new("service-hook");
    let source = plan("svc", r#":svc {:user "web" :group "web"}"#, "");
    assert_refused(sandbox.build(&source),
                   "`svc` names no `run`, and the plan has no hooks/run");

    fs::create_dir_all(sandbox.context().join("hooks")).unwrap();
    fs::write(sandbox.context().join("hooks/run"),
              "#!/bin/sh\nexec tool --serve\n").unwrap();
    let built = sandbox.build(&source).expect("builds");
    assert!(!built.prefix.join("run").exists(), "the run hook starts it");
    assert_eq!(read(&built.prefix.join("SVC_USER")), "web\n");
    assert_eq!(read(&built.prefix.join("SVC_GROUP")), "web\n");
}

#[test]
fn chmod_takes_symbolic_modes() {
    let sandbox = Sandbox::new("chmod-symbolic");
    let source = plan("chmod",
                      "",
                      r#" :build (fn [b]
                                   (b:write "a" "") (b:chmod "a" "644") (b:chmod "a" "+x")
                                   (b:write "b" "") (b:chmod "b" "640") (b:chmod "b" "o+r")
                                   (b:write "c" "") (b:chmod "c" "777") (b:chmod "c" "go-w,u=rw")
                                   (b:write "d" "") (b:chmod "d" "600") (b:chmod "d" "a+X")
                                   (b:mkdir "e") (b:chmod "e" "700") (b:chmod "e" "a+X"))"#);
    let built = sandbox.build(&source).expect("builds");
    let mode = |name: &str| {
        fs::metadata(built.prefix.join(name)).unwrap()
                                             .permissions()
                                             .mode()
        & 0o7777
    };
    assert_eq!(mode("a"), 0o755);
    assert_eq!(mode("b"), 0o644);
    assert_eq!(mode("c"), 0o655);
    assert_eq!(mode("d"),
               0o600,
               "X adds execute only where someone has it or for a directory");
    assert_eq!(mode("e"), 0o711);

    let bad = plan("chmod",
                   "",
                   r#" :build (fn [b] (b:write "a" "") (b:chmod "a" "+q"))"#);
    assert_refused(sandbox.build(&bad), "not octal or symbolic");
}

#[test]
fn run_with_check_false_returns_the_code_and_goes_on() {
    let sandbox = Sandbox::new("run-check");
    let source = plan("tolerant",
                      "",
                      r#" :build (fn [b]
                                   (b:write "code" (tostring (b:run ["sh" "-c" "exit 3"] {:check false})))
                                   (b:write "ok" (tostring (b:run ["true"]))))"#);
    let built = sandbox.build(&source).expect("builds");
    assert_eq!(read(&built.prefix.join("code")), "3");
    assert_eq!(read(&built.prefix.join("ok")), "0");

    let strict = plan("tolerant",
                      "",
                      r#" :build (fn [b] (b:run ["sh" "-c" "exit 3"]))"#);
    assert_refused(sandbox.build(&strict), "exited with code 3");
    let capture = plan("tolerant",
                       "",
                       r#" :build (fn [b] (b:capture ["true"] {:check false}))"#);
    assert_refused(sandbox.build(&capture), "b:capture");
}

#[test]
fn a_build_may_write_in_the_cache() {
    let sandbox = Sandbox::new("cache-writes");
    // The source project's plans configure in `../<name>-build` beside the source,
    // and move or mark what they download.
    let source = plan("sib",
                      "",
                      r#" :build (fn [b]
                                   (let [build (b:join b.cache (.. b.name "-build"))]
                                     (b:remove build)
                                     (b:mkdir (b:join build "sub"))
                                     (b:write (b:join build "sub/../made") "here")
                                     (b:run ["sh" "-c" "cat made > ../seen"] {:cwd build})
                                     (b:move (b:join b.cache "seen") (b:join b.cache "moved"))
                                     (b:write "copied" (b:read (b:join b.cache "moved")))))"#);
    let built = sandbox.build(&source).expect("builds");
    assert_eq!(read(&built.prefix.join("copied")), "here");

    for call in [r#"(b:mkdir (b:join b.cache "../escape"))"#,
                 r#"(b:write (b:join b.cache "sib-build/../../x") "")"#]
    {
        let bad = plan("sib", "", &format!(" :build (fn [b] {call})"));
        assert_refused(sandbox.build(&bad), "and the cache");
    }

    // `<path>/.` names only a directory, with or without `..` before it.
    let dirs = plan("sib",
                    "",
                    r#" :build (fn [b]
                                 (b:write "f" "")
                                 (b:mkdir "d")
                                 (b:write "answers"
                                          (table.concat [(tostring (b:exists (b:join b.prefix "f/.")))
                                                         (tostring (b:exists (b:join b.prefix "d/.")))
                                                         (tostring (b:exists (b:join b.prefix "d/../f/.")))
                                                         (tostring (b:exists (b:join b.prefix "f/../d/.")))]
                                                        " ")))"#);
    let built = sandbox.build(&dirs).expect("builds");
    assert_eq!(read(&built.prefix.join("answers")), "false true false true");
}

#[test]
fn read_and_write_keep_bytes_that_are_not_utf8() {
    let sandbox = Sandbox::new("bytes");
    // Latin-1 text and binary data, as source trees hold.
    let source = plan("bytes",
                      "",
                      r#" :build (fn [b]
                                   (b:write "latin1" "caf\xe9 /usr/bin/file\n")
                                   (let [text (b:read (b:join b.prefix "latin1"))]
                                     (b:write "fixed" (pick-values 1 (text:gsub "/usr/bin/file" "file")))
                                     (b:write "fed" (b:capture ["od" "-An" "-tx1"] {:stdin text}))))"#);
    let built = sandbox.build(&source).expect("builds");
    assert_eq!(fs::read(built.prefix.join("fixed")).unwrap(),
               b"caf\xe9 file\n");
    assert!(read(&built.prefix.join("fed")).contains("e9"),
            "stdin carried the byte");
}

#[test]
fn stdin_is_the_programs_input() {
    let sandbox = Sandbox::new("run-stdin");
    // More than a pipe holds, so a feeder that blocked on a full pipe would hang the test.
    let source = plan("fed",
                      "",
                      r#" :build (fn [b]
                                   (let [big (string.rep "line\n" 100000)]
                                     (b:write "count" (b:capture ["wc" "-l"] {:stdin big})))
                                   (b:run ["sh" "-c" "cat > fed"] {:stdin "one\ntwo\n" :cwd b.prefix})
                                   (b:write "code" (tostring (b:run ["grep" "-q" "x"] {:stdin "abc" :check false})))
                                   (b:write "early" (b:capture ["head" "-c" "3"] {:stdin (string.rep "y" 1000000)}))
                                   (b:write "empty" (b:capture ["cat"])))"#);
    let built = sandbox.build(&source).expect("builds");
    assert_eq!(read(&built.prefix.join("count")).trim(), "100000");
    assert_eq!(read(&built.prefix.join("fed")), "one\ntwo\n");
    assert_eq!(read(&built.prefix.join("code")), "1");
    assert_eq!(read(&built.prefix.join("early")),
               "yyy",
               "a program that stops reading is no error");
    assert_eq!(read(&built.prefix.join("empty")),
               "",
               "without stdin the input is empty");

    let bad = plan("fed", "", r#" :build (fn [b] (b:run ["cat"] {:stdin {}}))"#);
    assert_refused(sandbox.build(&bad), "`stdin` is a table");
}

#[test]
fn unsetenv_takes_a_variable_out_of_the_build() {
    let sandbox = Sandbox::new("unsetenv");
    let source = plan("unset",
                      "",
                      r#" :build (fn [b]
                                   (b:env "LD_RUN_PATH" "/somewhere")
                                   (b:unsetenv "LD_RUN_PATH")
                                   (b:write "lua" (tostring (b:getenv "LD_RUN_PATH")))
                                   (b:write "child" (b:capture ["sh" "-c" "echo ${LD_RUN_PATH-unset}"])))"#);
    let built = sandbox.build(&source).expect("builds");
    assert_eq!(read(&built.prefix.join("lua")), "nil");
    assert_eq!(read(&built.prefix.join("child")), "unset");
}
