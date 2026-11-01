<#
.SYNOPSIS
Signs each Tauri executable or installer using the main-only Azure CLI OIDC session.
#>
param(
  [Parameter(Mandatory = $true)]
  [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
  [string]$Path
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

foreach ($name in @('WINDOWS_SIGNING_ENDPOINT', 'WINDOWS_SIGNING_ACCOUNT', 'WINDOWS_SIGNING_PROFILE')) {
  if ([string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($name))) {
    throw "Missing Windows signing setting: $name"
  }
}

Import-Module ArtifactSigning -RequiredVersion 0.1.20 -ErrorAction Stop
$parameters = @{
  Endpoint = $env:WINDOWS_SIGNING_ENDPOINT
  CodeSigningAccountName = $env:WINDOWS_SIGNING_ACCOUNT
  CertificateProfileName = $env:WINDOWS_SIGNING_PROFILE
  Files = (Resolve-Path -LiteralPath $Path).Path
  FileDigest = 'SHA256'
  TimestampRfc3161 = 'http://timestamp.acs.microsoft.com'
  TimestampDigest = 'SHA256'
  Description = 'Spellwake'
  DescriptionUrl = 'https://app.spellwake.com'
  ExcludeEnvironmentCredential = $true
  ExcludeWorkloadIdentityCredential = $true
  ExcludeManagedIdentityCredential = $true
  ExcludeSharedTokenCacheCredential = $true
  ExcludeVisualStudioCredential = $true
  ExcludeVisualStudioCodeCredential = $true
  ExcludeAzureCliCredential = $false
  ExcludeAzurePowerShellCredential = $true
  ExcludeAzureDeveloperCliCredential = $true
  ExcludeInteractiveBrowserCredential = $true
}
Invoke-ArtifactSigning @parameters

$signature = Get-AuthenticodeSignature -LiteralPath $Path
if ($signature.Status -ne 'Valid' -or $null -eq $signature.TimeStamperCertificate) {
  throw 'Windows signing did not produce a valid timestamped Authenticode signature.'
}
