# traitobject

This is traitobject 0.1.0 from crates.io, vendored because the published
crate no longer compiles: three of its `impl Trait for dyn Send + Sync`
blocks spelled the same trait object in different orders, which rustc now
rejects as conflicting implementations. Those three lines are removed and
nothing else changed.

> Unsafe helpers for dealing with raw trait objects.

## Usage

Use the crates.io repository; add this to your `Cargo.toml` along
with the rest of your dependencies:

```toml
[dependencies]
traitobject = "*"
```

## Author

[Jonathan Reem](https://medium.com/@jreem) is the primary author and maintainer
of traitobject.

## License

MIT/Apache-2.0

