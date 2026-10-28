@echo off
setlocal EnableExtensions EnableDelayedExpansion
set "archive="
set "members="
:next
if "%~1"=="" goto run
set "arg=%~1"
if /I "!arg:~0,5!"=="-out:" (
  set "archive=!arg:~5!"
) else if /I not "!arg!"=="-nologo" (
  set "members=!members! "!arg!""
)
shift
goto next
:run
if not defined archive exit /b 2
zig.exe ar crs "!archive!" !members!
exit /b %ERRORLEVEL%
