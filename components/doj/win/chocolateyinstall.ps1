$version = '$version$'
$url = "https://bintray.com/dojo/stable/download_file?file_path=windows%2Fx86_64%2Fdoj-$version-x86_64-windows.zip"
$unzipLocation = "$(Split-Path -parent $MyInvocation.MyCommand.Definition)"
$checksum = '$checksum$'

Install-ChocolateyZipPackage "dojo" $url $unzipLocation -checksum $checksum -checksumType sha256
