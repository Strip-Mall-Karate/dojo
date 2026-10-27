;; doj-backline: what a studio puts on PATH. dojo-plans' backline, the base tools every build may
;; run, with the builder, editors and the rest of an interactive shell.
{:name "doj-backline"
 :origin "karate"
 :version (fn [b]
            (b:capture ["cat" "../../VERSION"]))
 :maintainer "Strip Mall Karate Company"
 :license ["Apache-2.0"]
 :targets ["x86_64-linux"]
 :description "The tools a Dojo studio puts on PATH."
 :upstream_url "https://github.com/StripMallKarateCompany/dojo"
 :deps ["karate/backline"
        "karate/doj-plan-build"
        "karate/less"
        "karate/mg"
        "karate/ncurses"
        "karate/util-linux"
        "karate/vim"]
 :build (fn [b]
          nil)
 :install (fn [b]
            nil)}
