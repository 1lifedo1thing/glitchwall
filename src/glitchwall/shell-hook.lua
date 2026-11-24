-- glitchwall's terminal hook for cmd (through Clink): shows the commands you type on the wallpaper.
--
-- Loaded by Clink from glitchwall's state folder after `glitchwall terminal on`, removed by `terminal off`. When you
-- press Enter it notes the command; when the next prompt comes it reads what the command printed off the console
-- screen and notes it with the exit code. Both go to %USERPROFILE%\.glitchwall\shell\cmd-<process id>.jsonl on this
-- machine only, where glitchwall's feed picks them up, masks secrets (the same redaction as Claude's calls) and shows
-- them. Nothing is sent anywhere. A command starting with a space is left out; nothing is recorded in a window
-- running as administrator.
if os.isuseradmin and os.isuseradmin() then return end

local dir = (os.getenv("GLITCHWALL_HOME") or ((os.getenv("USERPROFILE") or "") .. "\\.glitchwall")) .. "\\shell"
os.mkdir(dir)
local file = dir .. "\\cmd-" .. tostring(os.getpid()) .. ".jsonl"
local n, open = 0, nil

local function json(s)
    s = s:gsub("\\", "\\\\"):gsub('"', '\\"'):gsub("\n", "\\n"):gsub("\r", "\\r"):gsub("\t", "\\t")
    return '"' .. s:gsub("[%z\1-\31]", " ") .. '"'
end

local function write(rec)
    local f = io.open(file, "a+b")
    if not f then return end
    if f:seek("end") > 262144 then f:close(); os.remove(file); f = io.open(file, "a+b"); if not f then return end end
    f:write('{"sh": "cmd", "ts": "' .. os.date("!%Y-%m-%dT%H:%M:%S.000Z") .. '", ' .. rec .. "}\n")
    f:close()
end

-- Enter: note the command, and the exit code it starts from
clink.onendedit(function(line)
    open = nil
    if not line or line:match("^%s") or not line:match("%S") then return end
    n = n + 1
    -- cmd keeps the last exit code until a command sets another (echo doesn't), so remember it to see a change
    open = { id = n, text = line, code = os.geterrorlevel and os.geterrorlevel() or 0 }
    write('"t": "cmd", "id": ' .. n .. ', "text": ' .. json(line))
end)

-- the next prompt: the command is done; what it printed is on screen between its line and here
clink.onbeginedit(function()
    if not open then return end
    local o, lines = open, {}
    open = nil
    -- the command's own line, found from the bottom up (once the window is full it scrolls, so line numbers noted
    -- on Enter no longer point at the same text): by its end, or its start if a long line wrapped
    local last = console.getnumlines()
    local tail, head = o.text:sub(-30), o.text:sub(1, 30)
    local from
    for _, key in ipairs({ tail, head }) do
        for i = last, math.max(1, last - 300), -1 do
            local t = console.getlinetext(i)
            if t and t:find(key, 1, true) then from = i; break end
        end
        if from then break end
    end
    if from then
        for i = math.max(from + 1, last - 60), last do
            local t = console.getlinetext(i)
            if t then lines[#lines + 1] = (t:gsub("%s+$", "")) end
        end
    end
    while #lines > 0 and lines[#lines] == "" do lines[#lines] = nil end   -- the next prompt isn't drawn yet
    while #lines > 0 and lines[1] == "" do table.remove(lines, 1) end
    local code = os.geterrorlevel and os.geterrorlevel() or 0
    local failed = code ~= 0 and code ~= o.code   -- it set a new, non-zero exit code
    write('"t": "out", "id": ' .. o.id .. ', "ok": ' .. (failed and "false" or "true") ..
          ', "text": ' .. json(table.concat(lines, "\n")))
end)
