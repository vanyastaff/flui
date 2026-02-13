@echo off
REM Android hot-reload wrapper for Windows
REM Runs android-dev.sh via Git Bash
REM Usage: scripts\android-dev.bat [OPTIONS]

setlocal

REM Try Git Bash locations
if exist "C:\Program Files\Git\bin\bash.exe" (
    "C:\Program Files\Git\bin\bash.exe" "%~dp0android-dev.sh" %*
    exit /b %ERRORLEVEL%
)

if exist "C:\Program Files (x86)\Git\bin\bash.exe" (
    "C:\Program Files (x86)\Git\bin\bash.exe" "%~dp0android-dev.sh" %*
    exit /b %ERRORLEVEL%
)

REM Try bash on PATH (WSL, MSYS2, etc.)
where bash >nul 2>&1
if %ERRORLEVEL% equ 0 (
    bash "%~dp0android-dev.sh" %*
    exit /b %ERRORLEVEL%
)

echo Error: bash not found. Install Git for Windows or WSL.
exit /b 1
