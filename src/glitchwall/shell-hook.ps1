# glitchwall's terminal hook: shows the commands you type in PowerShell, and what they print, on the wallpaper.
#
# Loaded from your PowerShell profile by `glitchwall terminal on` (and removed by `glitchwall terminal off`).
# On Enter it notes the command; when the command is done it reads what the command printed from the console
# screen. Both go to ~\.glitchwall\shell\<process id>.jsonl on this machine only, where glitchwall's feed picks
# them up, masks secrets and personal details (the same redaction as Claude's calls) and shows them. Nothing is
# sent anywhere. Each file is cut back when it grows past 256 KB, and glitchwall clears out files a day old.
if ($Host.Name -ne 'ConsoleHost' -or -not (Get-Module PSReadLine)) { return }

$global:__gw = @{
    Dir  = Join-Path $HOME '.glitchwall\shell'
    N    = 0       # commands so far in this shell
    Open = $null   # the command waiting for its output: id, text, the screen line it was typed on
    Utf8 = New-Object System.Text.UTF8Encoding($false)
}
$null = New-Item -ItemType Directory -Force $global:__gw.Dir -ErrorAction SilentlyContinue
$global:__gw.File = Join-Path $global:__gw.Dir "$PID.jsonl"

function global:__gwWrite([hashtable]$rec) {
    try {
        $f = $global:__gw.File
        if ((Test-Path $f) -and (Get-Item $f).Length -gt 256KB) { Remove-Item $f -Force }
        $rec.ts = [DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ss.fffZ')
        [IO.File]::AppendAllText($f, (ConvertTo-Json $rec -Compress) + "`n", $global:__gw.Utf8)
    } catch {}
}

# what the screen shows between two lines: the command's output
function global:__gwScreen([int]$from, [int]$to) {
    $ui = $Host.UI.RawUI
    if ($to -lt $from) { return '' }
    $w = $ui.BufferSize.Width
    $cells = $ui.GetBufferContents((New-Object System.Management.Automation.Host.Rectangle 0, $from, ($w - 1), $to))
    $lines = for ($y = 0; $y -le $to - $from; $y++) {
        $sb = New-Object System.Text.StringBuilder
        for ($x = 0; $x -lt $w; $x++) { $c = $cells[$y, $x]; if ($c.BufferCellType -ne 'Trailing') { [void]$sb.Append($c.Character) } }
        $sb.ToString().TrimEnd()
    }
    (@($lines) | Select-Object -Last 60) -join "`n"
}

# while a command runs: every second, the newest of what it has printed so far (ping -t never finishes), read off the
# screen by a second runspace (the shell's own thread is busy running the command)
$global:__gw.Watcher = {
    param($ui, $file, $id, $first, $enc)
    $last = ''
    while ($true) {
        Start-Sleep -Milliseconds 1000
        try {
            $w = $ui.BufferSize.Width; $end = $ui.CursorPosition.Y
            $cells = $ui.GetBufferContents((New-Object System.Management.Automation.Host.Rectangle 0, 0, ($w - 1), $end))
            $rows = for ($y = 0; $y -le $end; $y++) {
                $sb = New-Object System.Text.StringBuilder
                for ($x = 0; $x -lt $w; $x++) { $c = $cells[$y, $x]; if ($c.BufferCellType -ne 'Trailing') { [void]$sb.Append($c.Character) } }
                $sb.ToString().TrimEnd()
            }
            $rows = @($rows); $start = 0
            for ($y = $rows.Count - 1; $y -ge 0; $y--) { if ($rows[$y].Contains($first)) { $start = $y + 1; break } }
            $text = (@($rows[$start..($rows.Count - 1)]) | Select-Object -Last 60) -join "`n"
            $text = $text.Trim("`n")
            if ($start -lt $rows.Count -and $text -and $text -ne $last) {
                $last = $text
                $rec = @{ t = 'out'; id = $id; live = $true; text = $text; ts = [DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ss.fffZ') }
                [IO.File]::AppendAllText($file, (ConvertTo-Json $rec -Compress) + "`n", $enc)
            }
        } catch {}
    }
}
function global:__gwWatchStop {
    $w = $global:__gw.Watch
    if ($w) { $global:__gw.Watch = $null; try { $w.Stop(); $w.Dispose() } catch {} }
}

# Enter: note the command (only a whole one: an unfinished line just goes on to the next), then run it as usual
Set-PSReadLineKeyHandler -Key Enter -BriefDescription 'glitchwall: show on the wallpaper, then run' -ScriptBlock {
    $line = $null; $cur = $null
    [Microsoft.PowerShell.PSConsoleReadLine]::GetBufferState([ref]$line, [ref]$cur)
    try {
        # a prompt set after this hook loaded (a theme in a later profile) took over: wrap that one instead
        if (-not "$function:prompt".Contains('__gwPromptMark')) {
            $global:__gwPrompt = $function:prompt
            Set-Item function:global:prompt $global:__gw.Prompt
        }
        $errs = $null
        [void][System.Management.Automation.Language.Parser]::ParseInput($line, [ref]$null, [ref]$errs)
        $whole = -not ($errs | Where-Object { $_.IncompleteInput })
        if ($whole -and $line.Trim()) {
            $global:__gw.N++
            $global:__gw.Open = @{ id = $global:__gw.N; text = $line; y = $Host.UI.RawUI.CursorPosition.Y }
            __gwWrite @{ t = 'cmd'; id = $global:__gw.N; text = $line }
            __gwWatchStop
            $ps = [powershell]::Create()
            [void]$ps.AddScript($global:__gw.Watcher).AddArgument($Host.UI.RawUI).AddArgument($global:__gw.File).AddArgument(
                $global:__gw.N).AddArgument(($line -split "`n")[0].TrimEnd()).AddArgument($global:__gw.Utf8)
            [void]$ps.BeginInvoke()
            $global:__gw.Watch = $ps
        }
    } catch {}
    [Microsoft.PowerShell.PSConsoleReadLine]::AcceptLine()
}

# the prompt: the last command is done, so send what it printed; then the prompt you had before. Put in place now,
# and again on Enter if a prompt set later (a theme in another profile) replaced it
$global:__gwPrompt = $function:prompt
$global:__gw.Prompt = {
    $ok = $?   # __gwPromptMark
    __gwWatchStop   # the command is done: no more live output
    $open = $global:__gw.Open
    if ($open) {
        $global:__gw.Open = $null
        try {
            $ui = $Host.UI.RawUI
            $end = $ui.CursorPosition.Y - 1
            # the command's own line(s), found by its first line (the screen may have scrolled since)
            $first = ($open.text -split "`n")[0].TrimEnd()
            $start = -1
            for ($y = [Math]::Min($open.y, $end); $y -ge 0; $y--) {
                if ((__gwScreen $y $y).Contains($first)) { $start = $y + ($open.text -split "`n").Count; break }
            }
            if ($start -lt 0) { $start = [Math]::Max(0, $end - $ui.WindowSize.Height + 2) }   # scrolled off: the last screenful
            __gwWrite @{ t = 'out'; id = $open.id; ok = $ok; text = (__gwScreen $start $end).Trim("`n") }
        } catch {}
    }
    & $global:__gwPrompt
}
Set-Item function:global:prompt $global:__gw.Prompt
