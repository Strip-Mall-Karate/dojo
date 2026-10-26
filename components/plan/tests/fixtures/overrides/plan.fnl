;; A plan whose macOS build differs from its Linux one.
{:name "hello"
 :origin "karate"
 :version "1.0.0"
 :targets ["x86_64-linux" "aarch64-linux" "aarch64-darwin"]
 :deps ["karate/glibc" "karate/zlib"]
 :build (fn [b] (b:run ["make"]))
 :on {"aarch64-darwin"
      {:deps ["karate/zlib"]
       :install (fn [b] (b:run ["make" "install-macos"]))}}}
