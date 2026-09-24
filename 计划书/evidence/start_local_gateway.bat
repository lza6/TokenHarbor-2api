@echo off
cd /d C:\Users\Administrator.DESKTOP-EGNE9ND\Desktop\tokenharbor-2api
set RUST_LOG=info
target\release\tokenharbor2api.exe --config config.json > "C:\Users\Administrator.DESKTOP-EGNE9ND\Desktop\tokenharbor-2api\计划书\evidence\local_gateway_stdout.log" 2>&1
