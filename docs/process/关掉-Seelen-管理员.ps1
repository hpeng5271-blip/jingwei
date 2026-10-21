# 以管理员身份运行（右键 PowerShell → 以管理员身份运行，然后粘这一整段）

# 1) 禁用 Seelen 的"服务"计划任务（RunLevel=Highest，普通权限动不了）
Disable-ScheduledTask -TaskPath '\Seelen\' -TaskName 'Seelen UI Service' -ErrorAction SilentlyContinue

# 2) 停掉进程（先停服务，再停界面，否则服务会把它拉起来）
Stop-Process -Name slu-service -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 1
Stop-Process -Name seelen-ui -Force -ErrorAction SilentlyContinue

# 3)（可选）彻底卸载——不想要桌面替换就执行这行
# & 'C:\Program Files\Seelen\Seelen UI\uninstall.exe'

# 4)（可选）把一个来路不明的自启动项也关掉（wnasst/wintray，HKLM，需管理员）
# Remove-ItemProperty 'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run' -Name wintray

# 5) 复查
Get-ScheduledTask -TaskPath '\Seelen\' | Select-Object TaskName, State
Get-Process seelen-ui, slu-service -ErrorAction SilentlyContinue
"free = {0} MB" -f [math]::Round((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory/1024)
