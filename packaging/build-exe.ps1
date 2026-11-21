# Build the standalone Windows exes into dist\glitchwall-<version>-windows\ and zip them.
#   powershell -File packaging\build-exe.ps1
# Needs uv. PyInstaller is fetched by uvx at build time only; nothing of it ships except its bootloader.
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot)
$ver  = (Select-String -Path pyproject.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$out  = "dist\glitchwall-$ver-windows"
$py   = 'pyinstaller==6.22.3'
$common = @('--onefile', '--clean', '--noconfirm', '--paths', 'src', '--workpath', 'build\pyi', '--specpath', 'build\pyi',
            '--distpath', $out, '--add-data', "$PWD\src\glitchwall\wallpaper;glitchwall\wallpaper")
uvx --python 3.14 --from $py pyinstaller @common --console  --name glitchwall      packaging\glitchwall_cli.py
uvx --python 3.14 --from $py pyinstaller @common --noconsole --name glitchwall-feed packaging\glitchwall_feed.py
Copy-Item LICENSE, README.md $out
Copy-Item src\glitchwall\wallpaper\OFL.txt "$out\OFL-DepartureMono.txt"
Compress-Archive -Force "$out\*" "dist\glitchwall-$ver-windows.zip"
Get-FileHash "dist\glitchwall-$ver-windows.zip", "$out\*.exe" -Algorithm SHA256 | Format-Table Hash, Path -AutoSize
