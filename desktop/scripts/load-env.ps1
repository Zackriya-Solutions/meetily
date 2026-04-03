# Load Environment Variables from .env file
# This script parses a .env file and loads variables into the current PowerShell session

function Load-EnvFile {
    param(
        [string]$EnvFilePath = ".env",
        [switch]$Verbose = $false
    )

    # Check if .env file exists
    if (-not (Test-Path $EnvFilePath)) {
        if ($Verbose) {
            Write-Host "ℹ️  No .env file found at: $EnvFilePath" -ForegroundColor Yellow
        }
        return $false
    }

    if ($Verbose) {
        Write-Host "📄 Loading environment variables from: $EnvFilePath" -ForegroundColor Cyan
    }

    $loadedCount = 0
    $lineNumber = 0

    try {
        Get-Content $EnvFilePath -ErrorAction Stop | ForEach-Object {
            $lineNumber++
            $line = $_.Trim()

            # Skip empty lines and comments
            if ([string]::IsNullOrWhiteSpace($line) -or $line.StartsWith('#')) {
                return
            }

            # Parse KEY=VALUE format
            if ($line -match '^([^=]+)=(.*)$') {
                $key = $matches[1].Trim()
                $value = $matches[2].Trim()

                # Remove surrounding quotes if present
                if ($value -match '^"(.*)"$' -or $value -match "^'(.*)'$") {
                    $value = $matches[1]
                }

                # Set environment variable
                Set-Item -Path "env:$key" -Value $value -Force

                if ($Verbose) {
                    $displayValue = if ($key -like "*KEY*" -or $key -like "*PASSWORD*" -or $key -like "*SECRET*") {
                        "***REDACTED***"
                    } else {
                        $value
                    }
                    Write-Host "   ✓ Loaded: $key = $displayValue" -ForegroundColor Green
                }

                $loadedCount++
            }
            else {
                Write-Warning "Skipping invalid line $lineNumber in .env: $line"
            }
        }

        if ($Verbose) {
            Write-Host "✅ Loaded $loadedCount environment variable(s) from .env" -ForegroundColor Green
            Write-Host ""
        }

        return $true
    }
    catch {
        Write-Error "Failed to load .env file: $_"
        return $false
    }
}

# Export function for module usage
Export-ModuleMember -Function Load-EnvFile
