$pkg_name="windows-service"
$pkg_origin="karate"
$pkg_version="0.4.0"
$pkg_maintainer="Strip Mall Karate Company"
$pkg_license=@('Apache-2.0')
$pkg_description="A Windows Service for runnung the Dojo Supervisor"
$pkg_deps=@("karate/doj-launcher", "karate/powershell")
$pkg_build_deps=@("karate/dotnet-core-sdk")
$pkg_bin_dirs=@("bin")

function Invoke-Build {
  Copy-Item $PLAN_CONTEXT/* $DOJ_CACHE_SRC_PATH/$pkg_dirname -force
  dotnet restore $DOJ_CACHE_SRC_PATH/$pkg_dirname/WindowsService.csproj
  dotnet build $DOJ_CACHE_SRC_PATH/$pkg_dirname/WindowsService.csproj -r win-x64
  if($LASTEXITCODE -ne 0) {
    Write-Error "dotnet build failed!"
  }
}

function Invoke-Install {
  dotnet publish $DOJ_CACHE_SRC_PATH/$pkg_dirname/WindowsService.csproj -r win-x64 --output "$pkg_prefix/bin"
}