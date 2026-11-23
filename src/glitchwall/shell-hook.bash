# glitchwall's terminal hook for bash (WSL): shows the commands you type on the wallpaper.
#
# Sourced from ~/.bashrc by `glitchwall terminal on` (and removed by `glitchwall terminal off`), with the Windows
# folder glitchwall reads as its argument. As a command starts (PS0) it notes the command; when it is done (the
# prompt) it notes whether it worked. Both go to <that folder>/wsl-<process id>.jsonl on this machine only, where
# glitchwall's feed picks them up, masks secrets and personal details (the same redaction as Claude's calls) and shows
# them. Nothing is sent anywhere. What a command prints is not read yet.
# Commands bash leaves out of its history (a leading space with HISTCONTROL=ignorespace, a repeat with ignoredups)
# are left out here too.
[[ $- == *i* && -n $1 && -z $__gw_dir ]] || return 0
__gw_dir=$1
__gw_file="$__gw_dir/wsl-$$.jsonl"
mkdir -p "$__gw_dir" 2>/dev/null || return 0

__gw_json() {   # a JSON string
    local s=$1 b='\'   # quoted patterns and replacements: bash 5.2 reads backslashes in bare ones
    s=${s//"$b"/"$b$b"}; s=${s//'"'/"$b\""}; s=${s//$'\n'/"${b}n"}; s=${s//$'\r'/"${b}r"}; s=${s//$'\t'/"${b}t"}
    s=${s//[$'\x01'-$'\x1f']/ }
    printf '"%s"' "$s"
}

__gw_write() {   # one record, with the time in UTC
    local f=$__gw_file
    if [[ -f $f ]] && (( $(stat -c %s "$f" 2>/dev/null || echo 0) > 262144 )); then rm -f "$f"; fi
    printf '{"sh": "bash", "ts": "%s", %s}\n' "$(date -u +%Y-%m-%dT%H:%M:%S.%3NZ)" "$1" >> "$f" 2>/dev/null
}

__gw_hist() { local h; h=$(HISTTIMEFORMAT='' builtin history 1); h=${h#"${h%%[! ]*}"}; printf '%s' "${h%%[ *]*}"; }

# a command is starting (PS0 runs in a subshell, just before it): note it if bash put it in its history
__gw_cmd() {
    local text n
    text=$(HISTTIMEFORMAT='' builtin history 1); text=${text#"${text%%[! ]*}"}; n=${text%%[ *]*}
    [[ -n $n && $n != "$__gw_seen" ]] || return 0
    text=${text#"$n"}; text=${text#\*}; text=${text#"${text%%[! ]*}"}
    [[ -n ${text//[[:space:]]/} ]] || return 0
    __gw_write "\"t\": \"cmd\", \"id\": $n, \"text\": $(__gw_json "$text")"
}

# the prompt: the last command is done, so note how it went
__gw_prompt() {
    local ok=$? n
    n=$(__gw_hist)
    if [[ -n $__gw_seen && -n $n && $n != "$__gw_seen" ]]; then
        # the prompt about to show, as it will look (colors taken out): glitchwall reads the command's output off the
        # window's screen and stops there
        local p=${PS1@P}
        p=$(printf '%s' "${p%%$'\n'*}" | sed 's/\x1b\[[0-9;?]*[A-Za-z]//g; s/\x1b\][^\x07]*\x07//g; s/[\x01\x02]//g')
        __gw_write "\"t\": \"out\", \"id\": $n, \"ok\": $( ((ok == 0)) && echo true || echo false), \"text\": \"exit $ok\", \"prompt\": $(__gw_json "$p")"
    fi
    __gw_seen=$n
    return $ok
}
__gw_seen=
__gw_prompt   # the history number we start from
PS0='$(__gw_cmd)'"$PS0"
if [[ $(declare -p PROMPT_COMMAND 2>/dev/null) == "declare -a"* ]]; then   # bash 5.1+: a list of commands
    PROMPT_COMMAND=(__gw_prompt "${PROMPT_COMMAND[@]}")
else
    PROMPT_COMMAND="__gw_prompt${PROMPT_COMMAND:+; $PROMPT_COMMAND}"
fi
