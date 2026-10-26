;; The Launcher's plan.sh as a Fennel plan.

;; Link paths shared by every target.
(fn link-env [b]
  (each [_ lib (ipairs [:libarchive :libsodium :openssl])]
    (let [up (lib:upper)]
      (b:env (.. up :_LIB_DIR) (b:path_for lib :lib))
      (b:env (.. up :_INCLUDE_DIR) (b:path_for lib :include)))))

{:name "doj-launcher"
 :origin "karate"
 :maintainer "Strip Mall Karate Company"
 :license ["Apache-2.0"]
 :targets ["x86_64-linux" "aarch64-linux"]
 :scaffolding "karate/scaffolding-rust"
 :bin_dirs ["bin"]

 ;; Computed at build time: the commit count on main.
 :version (fn [b] (b:capture ["git" "rev-list" "main" "--count"]))

 :deps ["karate/glibc" "karate/gcc-libs" "karate/libarchive"
        "karate/libsodium" "karate/openssl"]
 :build_deps ["karate/coreutils" "karate/rust" "karate/gcc"
              "karate/git" "karate/protobuf"]

 :prepare (fn [b]
            (b.default.prepare b)
            (link-env b)
            (b:env "PROTOC" (b:path_for "protobuf" "bin" "protoc"))
            (b:env "PROTOC_INCLUDE" (b:path_for "protobuf" "include")))

 :install (fn [b]
            (b:install (b.scaffolding.cargo_out b "doj-launch") "bin"))}
