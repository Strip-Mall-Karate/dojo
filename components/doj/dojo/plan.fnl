;; doj, the command-line tool: on Linux a static musl program in a package with no
;; dependencies, so a studio can run it before anything is installed.

(fn cargo-build [b rust-target]
  ;; The target directory starts empty: cargo would keep build scripts an earlier
  ;; toolchain linked.
  (let [target-dir (b:join b.cache "doj-target")]
    (b:run ["rm" "-rf" target-dir])
    (b:env "CARGO_TARGET_DIR" target-dir)
    ;; The version and target the program reports.
    (b:env "PLAN_VERSION" (.. b.version "/" b.release))
    (b:env "PLAN_PACKAGE_TARGET" b.target)
    (b:run ["cargo" "build" "--release" "--locked" "--target" rust-target "--jobs" (tostring b.jobs)])
    (b:join target-dir rust-target "release/doj")))

{:name "doj"
 :origin "karate"
 :version (fn [b]
            (b:capture ["cat" "../../VERSION"]))
 :maintainer "Strip Mall Karate Company"
 :license ["Apache-2.0"]
 :targets ["x86_64-linux" "x86_64-darwin"]
 :description "The Dojo command-line tool."
 :upstream_url "https://github.com/StripMallKarateCompany/dojo"
 :build_deps ["karate/rust" "karate/gcc" "karate/musl" "karate/glibc" "karate/gcc-libs"]
 :bin_dirs ["bin"]
 :build (fn [b]
          ;; doj's C code compiles against musl, and rustc links it static with its own musl.
          ;; Build scripts run here, against glibc, and need its and libgcc_s's directories in
          ;; their RUNPATH.
          (b:env "CC_x86_64_unknown_linux_musl" "musl-gcc")
          ;; glibc's include directory, from its build dependency, would mix into musl's C.
          (b:env "CFLAGS" "")
          (b:env "CPPFLAGS" "")
          (b:env "LD_RUN_PATH" (.. (b:path_for "glibc") "/lib:" (b:path_for "gcc-libs") "/lib"))
          (cargo-build b "x86_64-unknown-linux-musl"))
 :install (fn [b]
            (b:copy (b:join b.cache "doj-target/x86_64-unknown-linux-musl/release/doj")
                    (b:join b.prefix "bin/doj"))
            (b:chmod (b:join b.prefix "bin/doj") "755"))
 :on {"x86_64-darwin"
      {:build_deps []
       :build (fn [b]
                (cargo-build b "x86_64-apple-darwin"))
       :install (fn [b]
                  (b:copy (b:join b.cache "doj-target/x86_64-apple-darwin/release/doj")
                          (b:join b.prefix "bin/doj"))
                  (b:chmod (b:join b.prefix "bin/doj") "755"))}}}
