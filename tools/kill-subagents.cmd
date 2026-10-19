@echo off
chcp 65001 >nul
cd /d "%~dp0.."
echo.
echo  彻底清掉 DSH 里的子智能体会话
echo  ------------------------------------------------
echo  现在如果 DSH 还开着，请先完全退出（托盘也要退）。
echo  脚本会检查；检测到在跑就会拒绝执行。
echo.
node "%~dp0kill-subagents.mjs" %*
echo.
pause
