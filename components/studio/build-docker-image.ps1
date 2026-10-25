param ([String[]]$InstallHarts = $null)

$ErrorActionPreference = "Stop"

function info($message) {
    Write-Host -ForegroundColor Cyan -NoNewline "build-docker-image: "
    Write-Host $message
}
  
if((Get-Command doj -ErrorAction SilentlyContinue) -eq $null) {
  Write-Error "   'doj' command must be present on PATH, aborting"
}

If (-not (Test-Path env:IMAGE_NAME)) {
   $imageName = "localhost/dojo/win-studio-x86_64-windows"
} Else {
   $imageName = $env:IMAGE_NAME
}

$startDir="$pwd"
$tmpRoot = mkdir (Join-Path $env:TEMP ([System.IO.Path]::GetRandomFileName()))
Push-Location $tmpRoot
try {
    $env:FS_ROOT="$tmpRoot/rootfs"
    # Ensure that no existing `DOJ_BINLINK_DIR` environment variable is present,
    # like it would if executed in a Studio instance.
    $env:DOJ_BINLINK_DIR = $null
    
    info "Installing and extracting initial Dojo packages"
    # Generally, we want to use packages that are aligned with the channel
    # set when building the image which is usually unstable or an rc channel
    # in our pipelines. Because the windows-service package is maintained
    # in another repo and on a slower deployment cadence, we will always
    # install from its stable channel
    if(!$InstallHarts) {
        $InstallHarts = @(
            "karate/doj-studio",
            "karate/doj-sup",
            "karate/windows-service"
        )
    }
    $InstallHarts | % {
        Invoke-Expression "doj pkg install $_"
        if ($LASTEXITCODE -ne 0) {
            Write-Error "doj install failed for $_, aborting"
        }
    }
    $studioPath = doj pkg path karate/doj-studio
    if ($LASTEXITCODE -ne 0) {
      Write-Error "karate/doj-studio must be installed, aborting"
    }
    
    info "Purging container doj cache"
    Remove-Item "$env:FS_ROOT/doj/cache" -Recurse -Force
    
    $pathParts = $studioPath.Replace("\", "/").Split("/")
    $ident = [String]::Join("/", $pathParts[-4..-1])
    $shortVersion = $pathParts[-2]
    $version = "$($pathParts[-2])-$($pathParts[-1])"
    
@"
FROM microsoft/windowsservercore
MAINTAINER Strip Mall Karate Company
ADD rootfs /
WORKDIR /src
RUN /doj/pkgs/$ident/bin/doj/doj.exe pkg exec karate/windows-service install
ENTRYPOINT ["/doj/pkgs/$ident/bin/powershell/pwsh.exe", "-ExecutionPolicy", "bypass", "-NoLogo", "-file", "/doj/pkgs/$ident/bin/doj-studio.ps1"]
"@ | Out-File "$tmpRoot/Dockerfile" -Encoding ascii
    
    info "Building Docker image ${imageName}:$version'"
    docker build --no-cache -t ${imageName}:$version .
    if ($LASTEXITCODE -ne 0) {
        Write-Error "docker build failed, aborting"
    }

    info "Tagging latest image to ${imageName}:$version"
    docker tag ${imageName}:$version ${imageName}:latest
    
    info "Tagging latest image to ${imageName}:$shortVersion"
    docker tag ${imageName}:$version ${imageName}:$shortVersion
    
@"
`$docker_image="$imageName"
`$docker_image_version="$version"
`$docker_image_short_version="$shortVersion"
"@ | Out-File "$startDir/results/last_image.ps1" -Encoding ascii
    
    info ""
    info "Docker Image: ${imageName}:$version"
    info "Build Report: $startDir/results/last_image.ps1"
    info ""
}
finally {
    Pop-Location
    $env:FS_ROOT = $null
    Remove-Item $tmpRoot -Recurse -Force
}
