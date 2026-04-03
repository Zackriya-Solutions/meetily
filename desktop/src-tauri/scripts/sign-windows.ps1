param(
    [Parameter(Mandatory=$true)]
    [string]$FilePath
)

# Check if signing is enabled
if (-not $env:DIGICERT_KEYPAIR_ALIAS) {
    Write-Host "Skipping signing - DIGICERT_KEYPAIR_ALIAS not set"
    exit 0
}

Write-Host "Signing: $FilePath"
Write-Host "Using keypair alias: $env:DIGICERT_KEYPAIR_ALIAS"

# Sign the file with verbose output
$signOutput = smctl sign --keypair-alias $env:DIGICERT_KEYPAIR_ALIAS --input $FilePath --verbose 2>&1
$signExitCode = $LASTEXITCODE

Write-Host "Sign output: $signOutput"
Write-Host "Sign exit code: $signExitCode"

if ($signExitCode -ne 0) {
    Write-Error "Signing failed with exit code: $signExitCode"
    Write-Error "Output: $signOutput"
    exit $signExitCode
}

# Verify the signature was applied
$sig = Get-AuthenticodeSignature -FilePath $FilePath
if ($sig.Status -ne 'Valid') {
    Write-Error "Signature verification failed after signing"
    Write-Error "Status: $($sig.Status)"
    Write-Error "Message: $($sig.StatusMessage)"
    exit 1
}

Write-Host "Successfully signed: $FilePath"
Write-Host "Signature status: $($sig.Status)"
