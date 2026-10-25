This is the *old* Docker export code, which is no longer being
developed. To make changes to Docker export code, please look in
[components/pkg-build-docker](../pkg-build-docker) instead.

Changes to this package *will not be built* by either TravisCI or
Dojo Builder.

The code still exists in the repository because `doj-pkg-cfize` has a
runtime dependency on `doj-pkg-dockerize`. The `doj-pkg-cfize`
exporter was written around the same time (but before)
`doj-pkg-export-docker` was rewritten. As a result, it uses the older
code.

Long term, `doj-pkg-cfize` should be rewritten in Rust, following the
basic pattern laid out by `doj-pkg-export-docker`. If changes need to
be made to `doj-pkg-cfize` in the meantime, and if those changes
should also require changes to `doj-pkg-dockerize`, try and make the
changes in `doj-pkg-cfize` instead of requiring new builds of old
packages. Newcomers to Dojo shouldn't need to be confused by having
"current" packages of `doj-pkg-dockerize` alongside
`doj-pkg-export-docker`.
