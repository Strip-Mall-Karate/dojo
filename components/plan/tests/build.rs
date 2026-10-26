// Copyright (c) 2026 Strip Mall Karate Company
//
// Licensed under the MIT license. See LICENSE in the repository root.

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
        options
    }

    fn build(&self, source: &str) -> Result<Built, String> {
        self.build_with(source, &self.options())
    }

    fn build_with(&self, source: &str, options: &Options) -> Result<Built, String> {
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
                 (r#"(b:write "../escape" "")"#, "may not contain `..`"),
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
    let sha = Sha256::digest(fs::read(&tarball).unwrap()).iter()
                                                         .fold(String::new(), |hex, b| {
                                                             hex + &format!("{b:02x}")
                                                         });
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
