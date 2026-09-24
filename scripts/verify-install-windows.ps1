param(
  [Parameter(Mandatory = $true)][string]$PackageDirectory,
  [Parameter(Mandatory = $true)][string]$NodeExecutable
)
$ErrorActionPreference = 'Stop'
$installers = @(Get-ChildItem (Join-Path $PackageDirectory '*-setup.exe'))
if ($installers.Count -ne 1) { throw 'Expected one NSIS installer' }

# The installer registers shortcuts and an uninstaller even with a temporary /D.
# Refuse to overwrite a developer's installation; CI uses a clean runner.
$registryLocations = @(
  'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*',
  'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*',
  'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*'
)
$existing = @(Get-ItemProperty $registryLocations -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -eq 'BiBi' })
if ($existing.Count -gt 0) { throw 'BiBi is already installed. Run test-install in a clean Windows account or VM.' }

$root = Join-Path ([System.IO.Path]::GetTempPath()) ('bibi-install-' + [guid]::NewGuid().ToString('N'))
$destination = Join-Path $root 'app'
New-Item -ItemType Directory -Path $root | Out-Null
try {
  $process = Start-Process -FilePath $installers[0].FullName -ArgumentList "/S /D=$destination" -Wait -PassThru
  if ($process.ExitCode -ne 0) { throw "Installer failed: $($process.ExitCode)" }
  if (-not (Test-Path "$destination/bibi-desktop.exe")) { throw 'Desktop binary missing' }
  & $NodeExecutable scripts/verify-service.mjs "$destination/bibi.exe"
  if ($LASTEXITCODE -ne 0) { throw 'Installed server verification failed' }
} finally {
  $uninstaller = Join-Path $destination 'uninstall.exe'
  if (Test-Path $uninstaller) {
    $process = Start-Process -FilePath $uninstaller -ArgumentList '/S' -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "Test uninstaller failed: $($process.ExitCode)" }
  }
  if ((Test-Path "$destination/bibi.exe") -or (Test-Path "$destination/bibi-desktop.exe")) {
    throw "Test uninstallation left executable files in $destination"
  }
  Remove-Item -Recurse -Force $root
}
Write-Output 'Windows NSIS install, installed server, and uninstall verified.'
