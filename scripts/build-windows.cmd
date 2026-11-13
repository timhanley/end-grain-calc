@echo off
rem Builds End Grain Calc for Windows into "dist\End Grain Calc".
rem Run it from a Command Prompt or PowerShell window.
setlocal
cd /d "%~dp0.."

where cargo 1>nul 2>nul
if errorlevel 1 (
  echo Rust is not installed. Get it from https://rustup.rs then run this again.
  exit /b 1
)

cargo build --release
if errorlevel 1 exit /b 1

if not exist "dist\End Grain Calc" mkdir "dist\End Grain Calc"
copy /y "target\release\endgrain.exe" "dist\End Grain Calc\End Grain Calc.exe" >nul
if errorlevel 1 exit /b 1

echo.
echo Built "dist\End Grain Calc\End Grain Calc.exe"
echo Double-click it to run, or move that folder wherever you like.
