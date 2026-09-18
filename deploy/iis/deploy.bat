@echo off
REM
REM Build and stage a DBX Web deployment for IIS on Windows.
REM
REM Examples:
REM   deploy.bat                                  Build and stage to D:\dbx
REM   deploy.bat -DeployRoot E:\apps\dbx           Stage somewhere else
REM   deploy.bat -NoSqlCipher -SkipInstall         Skip the vendored OpenSSL build
REM   deploy.bat -SkipBuild -SkipIis               Restage an existing build only
REM   deploy.bat -Mode arr -Binding http/*:8080:   ARR reverse-proxy mode
REM   deploy.bat -?                                Full help
REM
REM Run an elevated prompt for the IIS app pool, unlock, and ACL steps.
REM
setlocal EnableDelayedExpansion
set "SCRIPT_DIR=%~dp0"
if not exist "%SCRIPT_DIR%deploy.ps1" (
  if exist "%CD%\deploy.ps1" set "SCRIPT_DIR=%CD%\"
)
cd /d "%SCRIPT_DIR%..\.."
if "%~1"=="--" shift
set "ARGS="
:collect_args
if "%~1"=="" goto run_script
set "ARGS=!ARGS! "%~1""
shift
goto collect_args
:run_script
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%SCRIPT_DIR%deploy.ps1" !ARGS!
exit /b %errorlevel%
