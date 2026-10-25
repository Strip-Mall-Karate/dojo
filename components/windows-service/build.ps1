function Test-PullRequest {
    ($env:APPVEYOR_REPO_BRANCH -like 'master') -and
        (test-path env:/APPVEYOR_PULL_REQUEST_NUMBER) -and
        (-not [string]::IsNullOrEmpty($env:APPVEYOR_PULL_REQUEST_NUMBER))
}

if(Test-PullRequest) {
    $channel = "unstable"
}
else {
    $channel = "stable"
}

$bootstrapDir = "c:\dojo"
$url = "https://api.bintray.com/content/dojo/stable/windows/x86_64/doj-$($env:doj_exe_version)-x86_64-windows.zip?bt_package=doj-x86_64-windows"
mkdir $bootstrapDir -Force
Write-Host "Download and install latest release of doj.exe from $url"
Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile doj.zip
Expand-Archive -Path doj.zip -DestinationPath $bootstrapDir -Force
Remove-Item doj.zip -Force
$dojExe = (Get-Item "$bootstrapDir/*/doj.exe").FullName

$env:DOJ_ORIGIN="core"
if($env:ORIGIN_KEY) {
    "SIG-SEC-1`ncore-20170318210306`n`n$($env:ORIGIN_KEY)" | & $dojExe origin key import
}
else {
    Write-Host "Generating fake secret origin key for core..."
    & $dojExe origin key generate core
}
if ($LASTEXITCODE -ne 0) {exit $LASTEXITCODE}

& $dojExe pkg build . -w
if ($LASTEXITCODE -ne 0) {exit $LASTEXITCODE}

$hart = (Get-Item "results\*.hart")[-1]
& $dojExe pkg install $hart.FullName
if ($LASTEXITCODE -ne 0) {exit $LASTEXITCODE}

$binPath = (Resolve-Path "/doj/pkgs/core/windows-service/*/*/bin").Path
$pathParts = $binPath.Split("\")
$versionStamp = "$($pathParts[-3])-$($pathParts[-2])"
Update-AppveyorBuild -Version $versionStamp

& $dojExe pkg exec core/windows-service install
if(!(Get-Service Dojo -ErrorAction SilentlyContinue)) {
    Throw "The Dojo service was not installed!"
}
Start-Service Dojo
if((Get-Service Dojo).Status -ne "Running") {
    Throw "The Dojo service was unable to start!"
}

$retry = 0
while ($retry -lt 10 -and !((Test-NetConnection localhost -Port 9632).TcpTestSucceeded)) {
    Write-Host "Waiting for Supervisor to listen on 9632..."
    Start-Sleep -Seconds 1
    $retry += 1
}

Write-Host "Validating we can talk to a running supervisor"
& $dojExe svc status
if ($LASTEXITCODE -ne 0) {exit $LASTEXITCODE}

if($env:DOJ_AUTH_TOKEN) {
    & $dojExe pkg upload $hart --channel $channel
    if ($LASTEXITCODE -ne 0) {exit $LASTEXITCODE}
}
