@echo off
pwsh.exe -NoProfile -ExecutionPolicy bypass -NoLogo -Command ". '%~dp0dojo.ps1';Uninstall-DojService" %*
