# Sets up the native build environment for Windows on ARM64 (aarch64-pc-windows-msvc).
# Local use (from frontend/):  . .\scripts\windows-arm64-env.ps1
# GitHub Actions:              ./frontend/scripts/windows-arm64-env.ps1 -GitHubEnv
#
# ggml rejects MSVC on ARM, so C/C++ dependencies build with clang-cl + Ninja.
# The CPU baseline (fp16 + dotprod, no i8mm) matches every Snapdragon Windows PC since 8cx Gen 1.
param([switch]$GitHubEnv)

$vsPath = & "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * `
    -requires Microsoft.VisualStudio.Component.VC.Tools.ARM64 -property installationPath
if (-not $vsPath) { throw 'Visual Studio with the ARM64 C++ build tools was not found.' }

$vars = @{}
# Import the Visual Studio ARM64 developer environment.
& cmd.exe /s /c "`"$vsPath\VC\Auxiliary\Build\vcvarsall.bat`" arm64 >nul 2>&1 && set" | ForEach-Object {
    if ($_ -match '^([^=]+)=(.*)$') { $vars[$Matches[1]] = $Matches[2] }
}

$llvmBin = "$vsPath\VC\Tools\Llvm\ARM64\bin"
if (-not (Test-Path "$llvmBin\clang-cl.exe")) { throw "clang-cl not found in $llvmBin." }
$vars['PATH'] = "$llvmBin;$($vars['PATH'])"
$vars['LIBCLANG_PATH'] = $llvmBin

# whisper-rs-sys pins bindgen 0.69, which generates empty structs with libclang 22+ (VS 2026).
# Keep compiling with Visual Studio's clang-cl, but give bindgen LLVM 21's libclang.
$clangMajor = [int]((& "$llvmBin\clang-cl.exe" --version)[0] -replace '^.*version (\d+).*$', '$1')
if ($clangMajor -ge 22) {
    if ($env:LIBCLANG_PATH) {
        $vars['LIBCLANG_PATH'] = $env:LIBCLANG_PATH
    } elseif ($GitHubEnv) {
        $installer = "$env:RUNNER_TEMP\LLVM-21.1.8-woa64.exe"
        curl.exe -sSfL -o $installer https://github.com/llvm/llvm-project/releases/download/llvmorg-21.1.8/LLVM-21.1.8-woa64.exe
        if ((Get-FileHash $installer -Algorithm SHA256).Hash -ne '1A770A49460B73667C1750B79F90E7027EAD468F966C5D30E180CAE19F61D91E') { throw 'LLVM 21 installer hash mismatch.' }
        Start-Process -Wait -FilePath $installer -ArgumentList '/S', "/D=$env:RUNNER_TEMP\llvm21"
        $vars['LIBCLANG_PATH'] = "$env:RUNNER_TEMP\llvm21\bin"
    } else {
        throw "Visual Studio's clang $clangMajor is too new for whisper-rs-sys's bindgen. Install LLVM 21 (ARM64) and set LIBCLANG_PATH to its bin folder."
    }
    if (-not (Test-Path "$($vars['LIBCLANG_PATH'])\libclang.dll")) { throw "libclang.dll not found in $($vars['LIBCLANG_PATH'])." }
}

# Use MSVC's linker by full path: in bash steps, Git's /usr/bin/link would otherwise win on PATH.
$vars['CARGO_TARGET_AARCH64_PC_WINDOWS_MSVC_LINKER'] = Join-Path $vars['VCToolsInstallDir'] 'bin\Hostarm64\arm64\link.exe'

$cpuFlags = '-march=armv8.2-a+fp16+dotprod -U__ARM_FEATURE_MATMUL_INT8'
$vars['CMAKE_GENERATOR'] = 'Ninja'
$vars['CC_aarch64_pc_windows_msvc'] = 'clang-cl'
$vars['CXX_aarch64_pc_windows_msvc'] = 'clang-cl'
# Clang 22+ (VS 2026) makes this an error by default; whisper.cpp's ggml.c mixes __fp16* and uint16_t*.
$vars['CFLAGS_aarch64_pc_windows_msvc'] = "$cpuFlags -Wno-incompatible-pointer-types"
$vars['CXXFLAGS_aarch64_pc_windows_msvc'] = "-EHsc $cpuFlags"
# llama-cpp-sys-2 links OpenMP only for GNU targets.
$vars['CMAKE_DISABLE_FIND_PACKAGE_OpenMP'] = 'TRUE'

foreach ($name in $vars.Keys) {
    if ([Environment]::GetEnvironmentVariable($name) -eq $vars[$name]) { continue }
    Set-Item "Env:$name" $vars[$name]
    if ($GitHubEnv) { "$name=$($vars[$name])" | Out-File $env:GITHUB_ENV -Encoding utf8 -Append }
}
