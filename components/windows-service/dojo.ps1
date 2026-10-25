function Install-DojService {
	if((Get-Service Dojo -ErrorAction SilentlyContinue) -ne $null) {
		Write-Error "The Dojo service is already installed. Please run 'doj exec core/windows-service uninstall' first if you wish to reinstall."
		return
	}

	if(!(Test-Path (Join-Path $env:SystemDrive "doj\pkgs\core\doj-sup"))) {
		$dojProc = Get-Process doj -ErrorAction SilentlyContinue
		if(!$dojProc) {
			Write-Error "Could not locate the Dojo CLI. Make sure you are running this via 'doj pkg exec core/windows-service install'."
			return
		}
		$dojExe = $dojProc[0].Path
		& $dojExe pkg install core/doj-sup
	}

	$svcPath = Join-Path $env:SystemDrive "doj\svc\windows-service"
	if(!(Test-Path $svcPath)) {
		mkdir $svcPath
	}

	Copy-Item "$PSScriptRoot\*" $svcPath -Force

	&$env:systemroot\system32\sc.exe create Dojo binpath= "$svcPath\DojService.exe" start= auto
	if($LASTEXITCODE -ne 0) {
	    Write-Error "Failed to install the Dojo Service!"
	}
	else {
		&$env:systemroot\system32\sc.exe description Dojo "The Dojo Supervisor service"
		Write-Host "Congratulations! The Dojo Service has succesfully been installed!"
	}
}

function Uninstall-DojService {
	if((Get-Service Dojo -ErrorAction SilentlyContinue) -eq $null) {
		Write-Error "The Dojo service is not installed."
		return
	}

	$svcPath = $env:SystemDrive -join "doj\svc\windows-service"
	Stop-Service Dojo
	while((Get-Service Dojo).Status -ne "Stopped") {
		Start-Sleep -Seconds 1
	}

	&$env:systemroot\system32\sc.exe delete Dojo

	if($LASTEXITCODE -ne 0) {
	    Write-Error "Failed to uninstall the Dojo Service!"
	}
	else {
		Write-Host "The Dojo Service has succesfully been uninstalled!"
	}
}