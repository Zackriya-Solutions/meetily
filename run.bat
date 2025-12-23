@echo off
REM Meetily Run Script
REM This script simplifies running the application in development mode

echo.
echo ========================================
echo   Run Meetily
echo ========================================
echo.

REM Check if Rust/Cargo is installed
where cargo >nul 2>&1
if errorlevel 1 (
    echo.
    echo ❌ Error: Rust/Cargo is not installed or not in your PATH.
    echo    Please install Rust from https://rustup.rs/
    echo    After installing, restart your terminal and try again.
    pause
    exit /b 1
)

REM Check if we are in the root directory and need to move to frontend
if exist "frontend\package.json" (
    echo    Found frontend directory, switching...
    cd frontend
) else if not exist "package.json" (
    echo    ❌ Error: Could not find package.json. Please run from the project root or frontend directory.
    pause
    exit /b 1
)

REM Check if pnpm is installed
where pnpm >nul 2>&1
if not errorlevel 1 goto :found_pnpm

REM Fallback to npm
where npm >nul 2>&1
if not errorlevel 1 goto :found_npm

goto :not_found

:found_pnpm
echo    Using pnpm...
call pnpm install
call pnpm run tauri:dev
goto :check_error

:found_npm
echo    Using npm...
call npm install
call npm run tauri:dev
goto :check_error

:not_found
echo    ❌ Error: Node.js (npm or pnpm) is not installed or not in PATH.
pause
exit /b 1

:check_error
if errorlevel 1 (
    echo.
    echo ❌ Application exited with error.
    pause
    exit /b 1
)

echo.
echo ✅ Application stopped.
exit /b 0
