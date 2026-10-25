#!/usr/bin/env powershell

#Requires -Version 5

param (
    # The name of the component to be built. Defaults to none
    [string]$Component
)

# Since we are only verifying we don't have build failures, make everything
# temp!
$env:DOJ_ORIGIN="throwaway"
# let's make a selfcontained tempdir for this job
$job_temp_root = mkdir (Join-Path $env:TEMP ([System.IO.Path]::GetRandomFileName()))
$env:DOJ_CACHE_KEY_PATH="$job_temp_root/keys"

Write-Host "--- :key: Generating fake origin key"
doj origin key generate
Write-Host "--- :doj: Running doj pkg build for $Component"
# REMOVE -R AFTER 0.75.0
doj pkg build -D -R components/$Component


if($LASTEXITCODE -ne 0) { Write-Error "Failed to build package!" }
