@echo off
chcp 65001 >nul
title TokenHarbor2API
cd /d "%~dp0"
if not exist config.json (
    copy config.example.json config.json >nul
)
start "" http://127.0.0.1:47830/ui
target\release\tokenharbor2api.exe --config config.json
