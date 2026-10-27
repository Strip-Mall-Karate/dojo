;; doj-studio: the studio scripts, with a static busybox and doj under libexec so a studio runs
;; before anything is installed in it.
{:name "doj-studio"
 :origin "karate"
 :version (fn [b]
            (b:capture ["cat" "../../VERSION"]))
 :maintainer "Strip Mall Karate Company"
 :license ["Apache-2.0"]
 :targets ["x86_64-linux"]
 :description "The clean-room build environment."
 :upstream_url "https://github.com/StripMallKarateCompany/dojo"
 :build_deps ["karate/busybox-static" "karate/doj"]
 :bin_dirs ["bin"]
 :build (fn [b]
          nil)
 :install (fn [b]
            (let [studio (b:join b.prefix "bin/doj-studio")
                  libexec (b:join b.prefix "libexec")]
              ;; The release version and author go into the program.
              (b:write studio (pick-values 1 (-> (b:read (b:join b.src "bin/doj-studio.sh"))
                                                 (: :gsub "@author@" "Strip Mall Karate Company")
                                                 (: :gsub "@version@" (.. b.version "/" b.release)))))
              (b:chmod studio "755")
              (each [_ file (ipairs (b:glob (b:join b.src "libexec") "doj-studio-*.sh"))]
                (let [to (b:join libexec (file:match "[^/]+$"))]
                  (b:copy file to)
                  (b:chmod to "755")))
              (b:copy (b:join (b:path_for "busybox-static") "bin/busybox") (b:join libexec "busybox"))
              ;; doj under a directory named for its ident, as the studio expects.
              (let [doj (b:path_for "doj")
                    dir (: (: (b:read (b:join doj "IDENT")) :gsub "%s+$" "") :gsub "/" "-")]
                (b:copy (b:join doj "bin/doj") (b:join libexec dir "bin/doj"))
                (b:symlink (.. dir "/bin/doj") (b:join libexec "doj")))))}
