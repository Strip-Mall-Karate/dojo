$pkg_name="doj-studio"
$pkg_origin="core"
$pkg_version=Get-Content "$PLAN_CONTEXT/../../../VERSION"
$pkg_maintainer="The Dojo Maintainers <humans@habitat.sh>"
$pkg_license=@("Apache-2.0")
$pkg_build_deps=@("core/powershell", "core/doj", "core/doj-plan-build-ps1", "core/7zip")
$pkg_bin_dirs=@("bin")

function Invoke-Build {
  Get-Content "$PLAN_CONTEXT/../bin/doj-studio.ps1" | % {
    $_.Replace("@author@", $pkg_maintainer).Replace("@version@", $pkg_version)
  } | Add-Content -Path doj-studio.ps1
}

function Invoke-Install {
  mkdir "$pkg_prefix/bin/powershell" | Out-Null
  mkdir "$pkg_prefix/bin/doj" | Out-Null
  mkdir "$pkg_prefix/bin/7zip" | Out-Null

  Copy-Item doj-studio.ps1 "$pkg_prefix/bin/doj-studio.ps1"
  Copy-Item $PLAN_CONTEXT/../bin/doj-studio.bat "$pkg_prefix/bin/doj-studio.bat"
  Copy-Item $PLAN_CONTEXT/../bin/SupervisorBootstrapper.cs "$pkg_prefix/bin/SupervisorBootstrapper.cs"

  Copy-Item "$(Get-DojPackagePath powershell)/bin/*" "$pkg_prefix/bin/powershell" -Recurse
  Copy-Item "$(Get-DojPackagePath doj)/bin/*" "$pkg_prefix/bin/doj"
  Copy-Item "$(Get-DojPackagePath 7zip)/bin/*" "$pkg_prefix/bin/7zip"
  Copy-Item "$(Get-DojPackagePath doj-plan-build-ps1)/bin/*" "$pkg_prefix/bin"
}
