@echo off
chcp 65001 >nul
title TokenHarbor2API 编译
cd /d "%~dp0"

echo ============================================
echo   TokenHarbor2API 编译脚本
echo ============================================
echo.

where cargo >nul 2>nul
if errorlevel 1 (
    echo [错误] 未找到 cargo，请先安装 Rust: https://rustup.rs
    pause
    exit /b 1
)

echo [1/3] 编译 release 版本...
cargo build --release
if errorlevel 1 (
    echo [错误] 编译失败
    pause
    exit /b 1
)

echo [2/3] 运行单元测试...
cargo test
if errorlevel 1 (
    echo [错误] 测试失败
    pause
    exit /b 1
)

echo.
echo [3/3] 编译成功!
echo   - 二进制: target\release\tokenharbor2api.exe
echo   - 运行:   tokenharbor2api.exe --config config.json
echo   - 面板:   http://127.0.0.1:47830/ui
echo.
pause
