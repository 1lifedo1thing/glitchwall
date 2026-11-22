# Build the standalone Windows exes into dist\glitchwall-<version>-windows\ and zip them.
#   powershell -File packaging\build-exe.ps1
# Needs uv. PyInstaller is fetched by uvx at build time only; nothing of it ships except its bootloader.
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot)
$ver  = (Select-String -Path pyproject.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$out  = "dist\glitchwall-$ver-windows"
$py   = 'pyinstaller==6.22.3'
$wv   = 'pywebview==6.2.1'   # glitchwall's own desktop windows; keep in step with pyproject.toml
$icon = "$PWD\src\glitchwall\wallpaper\glitchwall.ico"   # made by tools\make_icon.py
$common = @('--onefile', '--clean', '--noconfirm', '--paths', 'src', '--workpath', 'build\pyi', '--specpath', 'build\pyi',
            '--distpath', $out, '--icon', $icon, '--add-data', "$PWD\src\glitchwall\wallpaper;glitchwall\wallpaper")
uvx --python 3.14 --with $wv --from $py pyinstaller @common --console  --name glitchwall      packaging\glitchwall_cli.py
uvx --python 3.14 --with $wv --from $py pyinstaller @common --noconsole --name glitchwall-feed packaging\glitchwall_feed.py
# the bundled libraries' license texts
New-Item -ItemType Directory -Force "$out\licenses" | Out-Null
uv run --no-project --python 3.14 --with $wv python packaging\copy_licenses.py "$out\licenses"
Copy-Item THIRD_PARTY_NOTICES.md $out
Copy-Item LICENSE, README.md $out
Copy-Item src\glitchwall\wallpaper\OFL.txt "$out\OFL-DepartureMono.txt"
New-Item -ItemType Directory -Force "$out\font-licenses" | Out-Null
Copy-Item src\glitchwall\wallpaper\fonts\licenses\*.txt "$out\font-licenses"
Compress-Archive -Force "$out\*" "dist\glitchwall-$ver-windows.zip"
Get-FileHash "dist\glitchwall-$ver-windows.zip", "$out\*.exe" -Algorithm SHA256 | Format-Table Hash, Path -AutoSize
