;; The Launcher's plan.sh and plan.ps1 as one plan.

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
 :targets ["x86_64-linux" "aarch64-linux" "x86_64-windows"]
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
            (b:install (b.scaffolding.cargo_out b "doj-launch") "bin"))

 :on {"x86_64-windows"
      {:deps ["karate/openssl" "karate/zlib" "karate/libarchive"
              "karate/libsodium" "karate/xz"]
       :build_deps ["karate/visual-cpp-redist-2015"
                    "karate/visual-cpp-build-tools-2015"
                    "karate/rust" "karate/cacerts" "karate/git"]

       :prepare (fn [b]
                  (b.default.prepare b)
                  (link-env b)
                  (b:env "SSL_CERT_FILE"
                         (b:path_for "cacerts" "ssl" "certs" "cacert.pem"))
                  (b:env "OPENSSL_LIBS" "ssleay32:libeay32"))

       :install (fn [b]
                  (b:install (b.scaffolding.cargo_out b "doj-launch") "bin")
                  (each [_ dep (ipairs [:openssl :zlib :libarchive :libsodium :xz])]
                    (b:install (b:glob (b:path_for dep "bin") "*.dll") "bin"))
                  (b:install (b:glob (b:path_for "visual-cpp-redist-2015" "bin") "*")
                             "bin"))}}}
