;; doj-plan-build, the builder that runs Fennel plans, from this crate.
{:name "doj-plan-build"
 :origin "karate"
 :version (fn [b]
            (b:capture ["cat" "../../VERSION"]))
 :maintainer "Strip Mall Karate Company"
 :license ["Apache-2.0"]
 :targets ["x86_64-linux"]
 :description "Builds Dojo packages from Fennel plans."
 :upstream_url "https://github.com/StripMallKarateCompany/dojo"
 :deps ["karate/glibc" "karate/gcc-libs"]
 :build_deps ["karate/rust" "karate/gcc"]
 :bin_dirs ["bin"]
 ;; Cargo builds in the cache, not in the checkout, and with the locked dependencies. The
 ;; target directory starts empty: cargo would keep build scripts an earlier toolchain linked.
 :build (fn [b]
          (let [target (b:join b.cache "doj-plan-build-target")]
            (b:run ["rm" "-rf" target])
            (b:env "CARGO_TARGET_DIR" target)
            (b:run ["cargo" "build" "--release" "--locked" "--jobs" (tostring b.jobs)])))
 :install (fn [b]
            (b:copy (b:join b.cache "doj-plan-build-target/release/doj-plan-build")
                    (b:join b.prefix "bin/doj-plan-build"))
            (b:chmod (b:join b.prefix "bin/doj-plan-build") "755"))}
