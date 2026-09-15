@echo off
setlocal
set JAVA_EXE=%JAVA_HOME%\bin\java.exe
if not exist "%JAVA_EXE%" (
  for /f "delims=" %%J in ('where java 2^>nul') do set JAVA_EXE=%%J
)
if "%JAVA_EXE%"=="" (
  echo java not found — install a JRE 17+ to run TLC
  exit /b 1
)
set HERE=%~dp0
"%JAVA_EXE%" -cp "%HERE%tools\tla2tools.jar" tlc2.TLC -config "%HERE%IncrementalIndex.cfg" "%HERE%IncrementalIndex"
exit /b %ERRORLEVEL%
