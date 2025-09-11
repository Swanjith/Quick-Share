@echo off
echo 🚀 Building FastShare...
cargo build --release

echo 📦 Creating installation directory...
if not exist "%USERPROFILE%\.local\bin" mkdir "%USERPROFILE%\.local\bin"

echo 📋 Installing FastShare...
copy target\release\fastshare.exe "%USERPROFILE%\.local\bin\"

echo ✅ FastShare installed successfully!
echo 🎉 Add %USERPROFILE%\.local\bin to your PATH to use 'fastshare' from anywhere
echo.
echo Usage examples:
echo   fastshare send myfile.txt
echo   fastshare receive 192.168.1.100
pause
