$pkg_name = "doj-launcher"
$pkg_origin = "core"
$pkg_maintainer = "The Dojo Maintainers <humans@habitat.sh>"
$pkg_license = @("Apache-2.0")
$pkg_deps=@(
    "core/openssl",
    "core/zlib",
    "core/libarchive",
    "core/libsodium",
    "core/xz"
)
$pkg_bin_dirs = @("bin")
$pkg_build_deps = @(
    "core/visual-cpp-redist-2015",
    "core/visual-cpp-build-tools-2015",
    "core/rust",
    "core/cacerts",
    "core/git"
)

function Invoke-Prepare {
    if($env:DOJ_CARGO_TARGET_DIR) {
        $env:CARGO_TARGET_DIR           = "$env:DOJ_CARGO_TARGET_DIR"
    }
    else {
        $env:CARGO_TARGET_DIR           = "$DOJ_CACHE_SRC_PATH/$pkg_dirname"
    }

    $env:SSL_CERT_FILE              = "$(Get-DojPackagePath "cacerts")/ssl/certs/cacert.pem"
    $env:PLAN_VERSION               = "$pkg_version/$pkg_release"
    $env:LIB                        += ";$DOJ_CACHE_SRC_PATH/$pkg_dirname/lib"
    $env:INCLUDE                    += ";$DOJ_CACHE_SRC_PATH/$pkg_dirname/include"
    $env:SODIUM_LIB_DIR             = "$(Get-DojPackagePath "libsodium")/lib"
    $env:LIBARCHIVE_INCLUDE_DIR     = "$(Get-DojPackagePath "libarchive")/include"
    $env:LIBARCHIVE_LIB_DIR         = "$(Get-DojPackagePath "libarchive")/lib"
    $env:OPENSSL_LIBS               = 'ssleay32:libeay32'
    $env:OPENSSL_LIB_DIR            = "$(Get-DojPackagePath "openssl")/lib"
    $env:OPENSSL_INCLUDE_DIR        = "$(Get-DojPackagePath "openssl")/include"
}

function pkg_version {
    git rev-list master --count
}
function Invoke-Before {
    Set-PkgVersion
    $script:pkg_dirname = "${pkg_name}-${pkg_version}"
    $script:pkg_prefix = "$DOJ_PKG_PATH\$pkg_origin\$pkg_name\$pkg_version\$pkg_release"
    $script:pkg_artifact="$DOJ_CACHE_ARTIFACT_PATH\${pkg_origin}-${pkg_name}-${pkg_version}-${pkg_release}-${pkg_target}.${_artifact_ext}"
}

function Invoke-Build {
    Push-Location "$PLAN_CONTEXT/.."
    try {
        cargo build --release
        if($LASTEXITCODE -ne 0) {
            Write-Error "Cargo build failed!"
        }
    }
    finally { Pop-Location }
}

function Invoke-Install {
    Copy-Item "$env:CARGO_TARGET_DIR/release/doj-launch.exe" "$pkg_prefix/bin/doj-launch.exe"
    Copy-Item "$(Get-DojPackagePath "openssl")/bin/*.dll" "$pkg_prefix/bin"
    Copy-Item "$(Get-DojPackagePath "zlib")/bin/*.dll" "$pkg_prefix/bin"
    Copy-Item "$(Get-DojPackagePath "libarchive")/bin/*.dll" "$pkg_prefix/bin"
    Copy-Item "$(Get-DojPackagePath "libsodium")/bin/*.dll" "$pkg_prefix/bin"
    Copy-Item "$(Get-DojPackagePath "xz")/bin/*.dll" "$pkg_prefix/bin"
    Copy-Item "$(Get-DojPackagePath "visual-cpp-redist-2015")/bin/*" "$pkg_prefix/bin"
}
