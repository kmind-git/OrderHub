@echo off
echo Unexpected third-party compiler invocation: %~nx0 %* 1>&2
exit /b 99
