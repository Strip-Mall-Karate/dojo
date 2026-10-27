;; doj-sup, the Supervisor, with its API docs page made by raml2html.
{:name "doj-sup"
 :origin "karate"
 :version (fn [b]
            (b:capture ["cat" "../../VERSION"]))
 :maintainer "Strip Mall Karate Company"
 :license ["Apache-2.0"]
 :targets ["x86_64-linux"]
 :description "The Dojo Supervisor."
 :upstream_url "https://github.com/StripMallKarateCompany/dojo"
 :deps ["karate/busybox-static" "karate/glibc" "karate/gcc-libs" "karate/openssl"]
 :build_deps ["karate/rust" "karate/gcc" "karate/raml2html" "karate/protobuf"]
 :bin_dirs ["bin"]
 ;; The target directory starts empty: cargo would keep build scripts an earlier toolchain linked.
 :build (fn [b]
          (let [target-dir (b:join b.cache "doj-sup-target")]
            (b:run ["rm" "-rf" target-dir])
            (b:env "CARGO_TARGET_DIR" target-dir)
            ;; The version and target the program reports.
            (b:env "PLAN_VERSION" (.. b.version "/" b.release))
            (b:env "PLAN_PACKAGE_TARGET" b.target)
            (b:env "OPENSSL_DIR" (b:path_for "openssl"))
            ;; prost-build runs protoc for the launcher's protocol.
            (b:env "PROTOC" (.. (b:path_for "protobuf") "/bin/protoc"))
            (b:run ["cargo" "build" "--release" "--locked" "--features" "apidocs" "--jobs" (tostring b.jobs)])))
 :install (fn [b]
            (b:copy (b:join b.cache "doj-sup-target/release/doj-sup") (b:join b.prefix "bin/doj-sup"))
            (b:chmod (b:join b.prefix "bin/doj-sup") "755"))}
