(macro grow []
  (var s "x")
  (while true (set s (.. s s)))
  nil)
(grow)
{:name "hello" :origin "karate" :version "1.0.0" :targets ["x86_64-linux"]}
