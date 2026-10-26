(macro slurp [] (let [f (io.open "plan.fnl")] (f:read "*a")))
{:name "hello" :origin "karate" :version (slurp) :targets ["x86_64-linux"]}
