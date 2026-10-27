;; doj-sup-static, the Supervisor as a static musl program, without the API docs page.
{:name "doj-sup-static"
 :origin "karate"
 :version (fn [b]
            (b:capture ["cat" "../../../VERSION"]))
 :maintainer "Strip Mall Karate Company"
 :license ["Apache-2.0"]
 :targets ["x86_64-linux"]
 :description "The Dojo Supervisor, statically linked."
 :upstream_url "https://github.com/StripMallKarateCompany/dojo"
 :deps ["karate/busybox-static"]
 :build_deps ["karate/rust" "karate/gcc" "karate/musl" "karate/openssl-musl" "karate/protobuf" "karate/glibc" "karate/gcc-libs"]
 :bin_dirs ["bin"]
 :build (fn [b]
          (let [target-dir (b:join b.cache "doj-sup-static-target")]
            ;; The target directory starts empty: cargo would keep build scripts an earlier
            ;; toolchain linked.
            (b:run ["rm" "-rf" target-dir])
            (b:env "CARGO_TARGET_DIR" target-dir)
            (b:env "PLAN_VERSION" (.. b.version "/" b.release))
            (b:env "PLAN_PACKAGE_TARGET" b.target)
            ;; C code compiles against musl, and rustc links it static with its own musl. Build
            ;; scripts run here, against glibc, and need its and libgcc_s's directories in their
            ;; RUNPATH.
            (b:env "CC_x86_64_unknown_linux_musl" "musl-gcc")
            ;; glibc's include directory, from its build dependency, would mix into musl's C.
            (b:env "CFLAGS" "")
            (b:env "CPPFLAGS" "")
            ;; openssl-sys links musl's OpenSSL statically.
            (b:env "OPENSSL_DIR" (b:path_for "openssl-musl"))
            (b:env "OPENSSL_STATIC" "1")
            ;; prost-build runs protoc for the launcher's protocol.
            (b:env "PROTOC" (.. (b:path_for "protobuf") "/bin/protoc"))
            (b:env "LD_RUN_PATH" (.. (b:path_for "glibc") "/lib:" (b:path_for "gcc-libs") "/lib"))
            ;; The crate is the parent directory's.
            (b:run ["cargo" "build" "--release" "--locked" "--target" "x86_64-unknown-linux-musl" "--jobs" (tostring b.jobs)]
                   {:cwd ".."})))
 :install (fn [b]
            (b:copy (b:join b.cache "doj-sup-static-target/x86_64-unknown-linux-musl/release/doj-sup")
                    (b:join b.prefix "bin/doj-sup"))
            (b:chmod (b:join b.prefix "bin/doj-sup") "755"))}
