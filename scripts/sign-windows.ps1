# Signs the Windows installer with Authenticode and an RFC 3161 timestamp, then verifies it.
#
# Certificate options (see docs/SIGNING.md):
#   - A certificate in the Windows certificate store (an OV/EV code-signing cert, or one on a token):
#       $env:NAKOS_CERT_THUMBPRINT = "ABC123..."
#   - Azure Trusted Signing / a cloud HSM: set $env:NAKOS_SIGN_COMMAND to the full signtool
#     command line for your provider. "{file}" is replaced with the path to sign.
#
#   powershell -ExecutionPolicy Bypass -File scripts\sign-windows.ps1 [-Installer path\to\setup.exe]
param([string]$Installer = "")
$ErrorActionPreference = "Stop"
Set-Location (git rev-parse --show-toplevel)

$version = (Get-Content package.json | ConvertFrom-Json).version
if (-not $Installer) { $Installer = "src-tauri\target\x86_64-pc-windows-msvc\release\bundle\nsis\Nakos_${version}_x64-setup.exe" }
if (-not (Test-Path $Installer)) { $Installer = "src-tauri\target\release\bundle\nsis\Nakos_${version}_x64-setup.exe" }
if (-not (Test-Path $Installer)) { throw "No installer at $Installer. Build first: npx tauri build --bundles nsis" }

$signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" | Sort-Object FullName -Descending | Select-Object -First 1
if (-not $signtool) { throw "signtool.exe not found (install the Windows SDK)." }
$timestamp = "http://timestamp.digicert.com"

if ($env:NAKOS_SIGN_COMMAND) {
    $cmd = $env:NAKOS_SIGN_COMMAND.Replace("{file}", "`"$Installer`"")
    Write-Host "== signing with the provider command"
    cmd /c $cmd
} elseif ($env:NAKOS_CERT_THUMBPRINT) {
    Write-Host "== signing with certificate $env:NAKOS_CERT_THUMBPRINT"
    & $signtool.FullName sign /sha1 $env:NAKOS_CERT_THUMBPRINT /fd sha256 /tr $timestamp /td sha256 /d "Nakos" /du "https://nakos.org" $Installer
} else {
    throw "Set NAKOS_CERT_THUMBPRINT or NAKOS_SIGN_COMMAND (see docs/SIGNING.md)."
}
if ($LASTEXITCODE -ne 0) { throw "signtool sign failed ($LASTEXITCODE)" }

Write-Host "== verify"
& $signtool.FullName verify /pa /v $Installer
if ($LASTEXITCODE -ne 0) { throw "signature does not verify" }
Get-AuthenticodeSignature $Installer | Format-List Status, SignerCertificate, TimeStamperCertificate
(Get-FileHash $Installer -Algorithm SHA256).Hash.ToLower()
