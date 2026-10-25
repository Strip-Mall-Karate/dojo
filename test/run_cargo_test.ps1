#!/usr/bin/env powershell

#Requires -Version 5

param (
    # The name of the component to be built. Defaults to none
    [string]$Component,
    # Features to pass to cargo
    [string]$Features,
    # Options to pass to the cargo test command
    [string]$TestOptions
)

$ErrorActionPreference="stop"
$env:RUSTUP_HOME="C:\rust\.rustup"
$env:CARGO_HOME="C:\rust\.cargo"
$env:Path="$env:Path;$env:CARGO_HOME\bin"

If($Features) {
    $FeatureString = "--features $Features"
} Else {
    $FeatureString = ""
}

# Set cargo test invocation
$CargoTestCommand = "cargo test $FeatureString -- --nocapture $TestOptions"

choco install dojo -y

# TODO: these should be in a shared script?
Write-Host "--- Installing required prerequisites"
doj pkg install core/cacerts
doj pkg install core/libarchive
doj pkg install core/libsodium
doj pkg install core/openssl
doj pkg install core/protobuf
doj pkg install core/xz
doj pkg install core/zeromq
doj pkg install core/zlib

# Set up some path variables for ease of use later
$cacertsDir     = & doj pkg path core/cacerts
$libarchiveDir  = & doj pkg path core/libarchive
$libsodiumDir   = & doj pkg path core/libsodium
$opensslDir     = & doj pkg path core/openssl
$protobufDir    = & doj pkg path core/protobuf
$xzDir          = & doj pkg path core/xz
$zeromqDir      = & doj pkg path core/zeromq
$zlibDir        = & doj pkg path core/zlib

# Set some required variables
$env:SODIUM_LIB_DIR             = "$libsodiumDir\lib"
$env:LIBARCHIVE_INCLUDE_DIR     = "$libarchiveDir\include"
$env:LIBARCHIVE_LIB_DIR         = "$libarchiveDir\lib"
$env:OPENSSL_LIBS               = 'ssleay32:libeay32'
$env:OPENSSL_LIB_DIR            = "$opensslDir\lib"
$env:OPENSSL_INCLUDE_DIR        = "$opensslDir\include"
$env:LIBZMQ_PREFIX              = "$zeromqDir"
$env:SSL_CERT_FILE              = "$cacertsDir\ssl\certs\cacert.pem"
$env:SODIUM_STATIC              = "true"
$env:OPENSSL_STATIC             = "true"
$env:LD_LIBRARY_PATH            = "$env:LIBZMQ_PREFIX\lib;$env:SODIUM_LIB_DIR;$zlibDir\lib;$xzDir\lib"

# Make sure protoc is on the path, we also need to make sure the DLLs (in \bin) are on the path,
# because windows library pathing is weird and terrifying.
$env:Path="$env:Path;$protobufDir\bin;$zeromqDir\bin;$libarchiveDir\bin;$libsodiumDir\bin;$zlibDir\bin;$xzDir\bin;$opensslDir\bin"

Write-Host "--- Running cargo test on $Component with command: '$CargoTestCommand'"
cd components/$Component
Invoke-Expression $CargoTestCommand

if ($LASTEXITCODE -ne 0) {exit $LASTEXITCODE}
