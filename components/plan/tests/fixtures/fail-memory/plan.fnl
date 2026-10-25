(local t [])
(var s "x")
(while true
  (set s (.. s s))
  (table.insert t s))
{:name "hello" :origin "karate" :version "1.0.0" :targets ["x86_64-linux"]}
