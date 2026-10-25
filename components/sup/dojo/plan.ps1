$pkg_name = "doj-sup"
$pkg_origin = "karate"
$pkg_version = "$(Get-Content $PLAN_CONTEXT/../../../VERSION)"
$pkg_maintainer = "Strip Mall Karate Company"
$pkg_license = @("Apache-2.0")
$pkg_bin_dirs = @("bin")
$pkg_deps=@(
    "karate/powershell",
    "karate/openssl",
    "karate/zlib",
    "karate/libarchive",
    "karate/libsodium",
    "karate/visual-cpp-redist-2015",
    "karate/xz",
    "karate/zeromq"
)
$pkg_build_deps = @(
    "karate/visual-cpp-build-tools-2015",
    "karate/rust",
    "karate/cacerts",
    "karate/raml2html"
)

function Invoke-Prepare {
    if($env:DOJ_CARGO_TARGET_DIR) {
        $env:CARGO_TARGET_DIR           = "$env:DOJ_CARGO_TARGET_DIR"
    }
    else {
        $env:CARGO_TARGET_DIR           = "$DOJ_CACHE_SRC_PATH/$pkg_dirname"
    }

    $env:SSL_CERT_FILE              = "$(Get-DojPackagePath "cacerts")/ssl/certs/cacert.pem"
    $env:LIB                        += ";$DOJ_CACHE_SRC_PATH/$pkg_dirname/lib"
    $env:INCLUDE                    += ";$DOJ_CACHE_SRC_PATH/$pkg_dirname/include"
    $env:SODIUM_LIB_DIR             = "$(Get-DojPackagePath "libsodium")/lib"
    $env:LIBARCHIVE_INCLUDE_DIR     = "$(Get-DojPackagePath "libarchive")/include"
    $env:LIBARCHIVE_LIB_DIR         = "$(Get-DojPackagePath "libarchive")/lib"
    $env:OPENSSL_LIBS               = 'ssleay32:libeay32'
    $env:OPENSSL_LIB_DIR            = "$(Get-DojPackagePath "openssl")/lib"
    $env:OPENSSL_INCLUDE_DIR        = "$(Get-DojPackagePath "openssl")/include"
    $env:LIBZMQ_PREFIX              = "$(Get-DojPackagePath "zeromq")"

    # Used by the `build.rs` program to set the version of the binaries
    $env:PLAN_VERSION = "$pkg_version/$pkg_release"
    Write-BuildLine "Setting env:PLAN_VERSION=$env:PLAN_VERSION"

    # Used to set the active package target for the binaries at build time
    $env:PLAN_PACKAGE_TARGET = "$pkg_target"
    Write-BuildLine "Setting env:PLAN_PACKAGE_TARGET=$env:PLAN_PACKAGE_TARGET"
}

function Invoke-Build {
    Push-Location "$PLAN_CONTEXT"
    try {
        cargo build --release --no-default-features --features apidocs
        if($LASTEXITCODE -ne 0) {
            Write-Error "Cargo build failed!"
        }
    }
    finally { Pop-Location }
}

function Invoke-Install {
    Copy-Item "$env:CARGO_TARGET_DIR/release/doj-sup.exe" "$pkg_prefix/bin/doj-sup.exe"
    Copy-Item "$(Get-DojPackagePath "openssl")/bin/*.dll" "$pkg_prefix/bin"
    Copy-Item "$(Get-DojPackagePath "zlib")/bin/*.dll" "$pkg_prefix/bin"
    Copy-Item "$(Get-DojPackagePath "libarchive")/bin/*.dll" "$pkg_prefix/bin"
    Copy-Item "$(Get-DojPackagePath "libsodium")/bin/*.dll" "$pkg_prefix/bin"
    Copy-Item "$(Get-DojPackagePath "xz")/bin/*.dll" "$pkg_prefix/bin"
    Copy-Item "$(Get-DojPackagePath "zeromq")/bin/*.dll" "$pkg_prefix/bin"
    Copy-Item "$(Get-DojPackagePath "visual-cpp-redist-2015")/bin/*.dll" "$pkg_prefix/bin"
}
