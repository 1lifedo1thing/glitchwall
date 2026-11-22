"""Mask secrets and personal paths before anything reaches the wallpaper.

Three layers, applied in order:
  1. known secret formats (cloud keys, vendor tokens, JWTs, private keys, ...)
  2. "name = value" style assignments, env exports, CLI flags and HTTP headers
     whose name says the value is secret (password, token, api_key, ...)
  3. a high-entropy catch for anything token-shaped that slipped through
Plus: the home directory becomes "~", the user name and email local parts are masked.

The goal is "never show a secret", not "never show anything odd", so this errs
toward masking. Masked text becomes MASK.
"""
from __future__ import annotations

import getpass
import math
import os
import re
from collections import Counter
from pathlib import Path

MASK = "••••"

# ---------------------------------------------------------------- 1. known formats
_KNOWN = [
    # private key blocks (PEM, OpenSSH, PGP, encrypted). Unterminated blocks mask to the end.
    r"-----BEGIN[A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----.*?(?:-----END[A-Z0-9 ]*PRIVATE KEY(?: BLOCK)?-----|\Z)",
    r"PuTTY-User-Key-File-\d+:.*?(?:Private-MAC:\s*\S+|\Z)",
    r"\bAGE-SECRET-KEY-1[0-9A-Z]{50,}",
    # AWS access key ids
    r"\b(?:AKIA|ASIA|AGPA|AIDA|AROA|AIPA|ANPA|ANVA|ABIA|ACCA)[0-9A-Z]{16}\b",
    # Google API key, OAuth client secret / refresh token
    r"\bAIza[0-9A-Za-z_-]{35}\b",
    r"\bGOCSPX-[0-9A-Za-z_-]{20,}",
    r"\b1//0[0-9A-Za-z_-]{30,}",
    r"\bya29\.[0-9A-Za-z_-]{20,}",
    # Azure storage / service bus keys and SAS signatures
    r"(?i)(?<=AccountKey=)[A-Za-z0-9+/=]{20,}",
    r"(?i)(?<=SharedAccessKey=)[A-Za-z0-9+/=]{20,}",
    r"(?i)(?<=[?&]sig=)[A-Za-z0-9%+/=]{16,}",
    # GitHub
    r"\bgh[pousr]_[A-Za-z0-9]{30,}\b",
    r"\bgithub_pat_[A-Za-z0-9_]{40,}\b",
    # GitLab
    r"\bgl(?:pat|dt|rt|ptt|cbt|oas|soat|ft|imt|agent|wt)-[A-Za-z0-9_-]{20,}",
    # Slack tokens and webhooks
    r"\bxox[abposre]-[A-Za-z0-9-]{10,}",
    r"\bxapp-\d-[A-Za-z0-9-]{10,}",
    r"hooks\.slack\.com/(?:services|workflows|triggers)/[A-Za-z0-9/_-]+",
    # Discord webhook and bot token
    r"discord(?:app)?\.com/api/webhooks/\d+/[A-Za-z0-9_-]+",
    r"\b[MN][A-Za-z\d_-]{23,25}\.[A-Za-z\d_-]{6}\.[A-Za-z\d_-]{27,}\b",
    # Stripe
    r"\b(?:sk|rk|pk)_(?:live|test)_[A-Za-z0-9]{10,}\b",
    r"\bwhsec_[A-Za-z0-9]{20,}\b",
    # AI providers (Anthropic, OpenAI, ...)
    r"\bsk-(?:ant-|proj-|svcacct-|admin-)?[A-Za-z0-9_-]{20,}",
    r"\bhf_[A-Za-z0-9]{30,}\b",
    r"\br8_[A-Za-z0-9]{30,}\b",
    r"\bgsk_[A-Za-z0-9]{40,}\b",
    # package registries
    r"\bnpm_[A-Za-z0-9]{36}\b",
    r"\bpypi-AgE[A-Za-z0-9_-]{50,}",
    r"\boy2[a-z0-9]{43}\b",  # NuGet
    r"\bdckr_pat_[A-Za-z0-9_-]{20,}",
    # infra and SaaS
    r"\bdop_v1_[a-f0-9]{64}\b",
    r"\bdo[ar]_v1_[a-f0-9]{64}\b",
    r"\bhvs\.[A-Za-z0-9_-]{24,}",
    r"\bhv[br]\.[A-Za-z0-9_-]{24,}",
    r"\btskey-[A-Za-z0-9-]{10,}",
    r"\bshp(?:at|ss|ca|pa)_[a-fA-F0-9]{32}\b",
    r"\bSG\.[A-Za-z0-9_-]{16,}\.[A-Za-z0-9_-]{16,}",
    r"\bkey-[0-9a-f]{32}\b",  # Mailgun
    r"\bSK[0-9a-fA-F]{32}\b",  # Twilio
    r"\bdp\.(?:pt|st|ct|sa|scim|audit)\.[A-Za-z0-9]{30,}",  # Doppler
    r"\bglc_[A-Za-z0-9+/=_-]{30,}",  # Grafana cloud
    r"\bglsa_[A-Za-z0-9_]{30,}",  # Grafana service account
    r"\bnfp_[A-Za-z0-9]{30,}",  # Netlify
    r"\bvercel_[A-Za-z0-9]{20,}",
    r"\bfigd_[A-Za-z0-9_-]{30,}",
    r"\blin_api_[A-Za-z0-9]{30,}",
    r"\bntn_[A-Za-z0-9]{30,}|\bsecret_[A-Za-z0-9]{40,}",  # Notion
    r"\bATATT3[A-Za-z0-9_=-]{30,}",  # Atlassian
    r"\b\d{8,10}:AA[A-Za-z0-9_-]{30,}\b",  # Telegram bot
    r"\bEAA[A-Za-z0-9]{60,}",  # Facebook
    # JSON Web Tokens (and other eyJ... base64 JSON blobs)
    r"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}(?:\.[A-Za-z0-9_-]*)?",
]
KNOWN = [re.compile(p, re.S) for p in _KNOWN]

# ---------------------------------------------------------------- 2. by name
# words that mark a value as secret when they appear in a variable / key / flag name
_SECRET_WORD = (
    r"(?:passw(?:or)?d|passwd|pass|pwd|passphrase|secret|token|api[_-]?key|apikey|access[_-]?key|"
    r"private[_-]?key|client[_-]?secret|credentials?|creds|auth(?!or)(?:[_-]?token)?|bearer|session[_-]?(?:id|key|token)|"
    r"cookie|signature|salt|seed[_-]?phrase|mnemonic|pin[_-]?code|otp|totp|webhook[_-]?url|dsn|conn(?:ection)?[_-]?str(?:ing)?|"
    r"license[_-]?key|encryption[_-]?key|master[_-]?key|signing[_-]?key)"
)
# a key name containing one of the words, e.g. DB_PASSWORD, githubToken, "api-key"
_NAME = rf"[A-Za-z0-9_.-]*{_SECRET_WORD}[A-Za-z0-9_.-]*"
_VALUE = r"(?:\"[^\"\n]*\"|'[^'\n]*'|`[^`\n]*`|[^\s,;&|)}\]\"']+)"

BY_NAME = [
    # name = value / name: value / "name": "value"  (also covers export X=, set X=, $env:X =, ENV X)
    # ":" counts only before a space or quote, so file:line references like auth.py:41: stay readable;
    # the gaps never cross a line break, so a diff's next "+" line isn't taken for the value
    (re.compile(rf"(?i)(\b{_NAME}[\"']?[ \t]*(?::=|=>|=|:(?=[\s\"'`]))[ \t]*)({_VALUE})"), r"\1" + MASK),
    # PowerShell / dotenv / Dockerfile:  $env:X = 'v'   ENV X v   setx X v
    (re.compile(rf"(?i)(\b(?:ENV|setx|set-item\s+env:)\s+{_NAME}\s+)({_VALUE})"), r"\1" + MASK),
    # CLI flags:  --password hunter2   --token=abc   -Password 'x'
    (re.compile(rf"(?i)((?<![\w-])--?{_NAME}(?:=|\s+))({_VALUE})"), r"\1" + MASK),
    # mysql -pSECRET
    (re.compile(r"(\bmysql\S*\s(?:.*\s)?-p)(\S+)"), r"\1" + MASK),
    # curl -u user:pass / --user user:pass
    (re.compile(r"((?:\s-u|--user)\s+[\"']?[^:\s\"']+:)([^\s\"']+)"), r"\1" + MASK),
    # HTTP headers
    (re.compile(r"(?i)\b((?:proxy-)?authorization\s*:\s*)(?:(?:bearer|basic|token|digest|negotiate)\s+)?[^\s\"',]+"), r"\1" + MASK),
    (re.compile(r"(?i)\b((?:set-)?cookie\s*:\s*)[^\n\"']+"), r"\1" + MASK),
    (re.compile(r"(?i)\b(x-[a-z-]*(?:key|token|secret|auth)[a-z-]*\s*:\s*)[^\s\"',]+"), r"\1" + MASK),
    # credentials inside URLs, connection strings:  scheme://user:pass@host
    (re.compile(r"(?i)\b([a-z][a-z0-9+.-]*://[^:/?#\s@]*:)[^@\s/]+@"), r"\1" + MASK + "@"),
    # ADO.NET / ODBC style:  Password=x;  Pwd=x;
    (re.compile(r"(?i)\b((?:password|pwd|user\s*password)\s*=\s*)([^;\"'\n]+)"), r"\1" + MASK),
    # .netrc:  machine x login y password z
    (re.compile(r"(?i)(\bpassword\s+)(\S+)"), r"\1" + MASK),
    # bare Bearer tokens anywhere
    (re.compile(r"(?i)(\bbearer\s+)[A-Za-z0-9._~+/=-]{8,}"), r"\1" + MASK),
    # passwords handed to a program as a plain argument
    (re.compile(rf"(\bsshpass\s+(?:-[^p\s]\S*\s+)*-p\s*)({_VALUE})"), r"\1" + MASK),
    (re.compile(rf"(\b(?:docker|podman|nerdctl|oras|helm\s+registry)\s+login\b[^\n|;&]*?\s-p\s+)({_VALUE})"), r"\1" + MASK),
    (re.compile(rf"(?m)(\bhtpasswd\b[^\n|;&]*?\s-\w*b\w*\s[^\n|;&]*\s)({_VALUE})(?=[ \t]*(?:$|[|;&]))"), r"\1" + MASK),
    (re.compile(rf"(?i)(\bnet\s+user\s+(?:\"[^\"\n]*\"|\S+)\s+)(?!/)({_VALUE})"), r"\1" + MASK),
    (re.compile(rf"(?i)(\bConvertTo-SecureString\s+(?:-String\s+)?)({_VALUE})"), r"\1" + MASK),
]

# ---------------------------------------------------------------- 3. entropy
_HEX = re.compile(r"(?<![A-Za-z0-9])[A-Fa-f0-9]{32,}(?![A-Za-z0-9])")
_TOKEN = re.compile(r"(?<![A-Za-z0-9+/=_-])[A-Za-z0-9+/=_-]{20,}(?![A-Za-z0-9+/=_-])")


def entropy(s: str) -> float:
    """Shannon entropy in bits per character."""
    n = len(s)
    return -sum(c / n * math.log2(c / n) for c in Counter(s).values()) if n else 0.0


def looks_random(tok: str) -> bool:
    """Token-shaped and random enough to be a key, not a word, path or identifier."""
    parts = [p for p in re.split(r"[/]", tok) if p]
    if len(parts) > 1:  # a path: only mask if one of its pieces itself looks random
        return any(len(p) >= 20 and looks_random(p) for p in parts)
    core = tok.strip("=")
    if len(core) < 20:
        return False
    if not (re.search(r"[a-z]", core) and re.search(r"[A-Z]", core) and re.search(r"[0-9]", core)):
        return False
    # identifiers like Some_Long_Name_2024 are made of words; secrets are not
    pieces = re.split(r"[_-]", core)
    if len(pieces) > 1 and max(len(p) for p in pieces) < 16:
        return False
    digits = sum(ch.isdigit() for ch in core)
    return entropy(core) >= 3.6 and digits >= 2


def _entropy_sub(text: str) -> str:
    text = _HEX.sub(MASK, text)
    return _TOKEN.sub(lambda m: MASK if looks_random(m.group(0)) else m.group(0), text)


# ---------------------------------------------------------------- personal info
_EMAIL = re.compile(r"\b[A-Za-z0-9._%+-]+@([A-Za-z0-9-]+\.)+[A-Za-z]{2,}\b")


def _home_variants(home: str) -> list[str]:
    out = {home, home.replace("\\", "/"), home.replace("\\", "\\\\"), home.replace("/", "\\")}
    if len(home) > 2 and home[1] == ":":  # C:\Users\x -> /c/Users/x (Git Bash), C:/Users/x
        out.add("/" + home[0].lower() + home[2:].replace("\\", "/"))
        out.add("/mnt/" + home[0].lower() + home[2:].replace("\\", "/"))
    return sorted((v for v in out if v), key=len, reverse=True)


def _short_home(home: str) -> str:
    """Windows' 8.3 form of the home folder (C:\\Users\\JOHNSM~1), which temp paths often use. '' elsewhere."""
    if os.name != "nt" or not home:
        return ""
    try:
        import ctypes
        buf = ctypes.create_unicode_buffer(520)
        n = ctypes.windll.kernel32.GetShortPathNameW(home, buf, 520)
        return buf.value if 0 < n < 520 and buf.value.lower() != home.lower() else ""
    except Exception:
        return ""


class Redactor:
    def __init__(self, home: str | None = None, user: str | None = None,
                 mask_user: bool = True, mask_email: bool = True, entropy_check: bool = True):
        home = home if home is not None else str(Path.home())
        if user is None:
            try:
                user = getpass.getuser()
            except Exception:
                user = os.path.basename(home)
        short = _short_home(home)
        variants = sorted({*_home_variants(home), *(_home_variants(short) if short else [])}, key=len, reverse=True)
        self.homes = [re.compile(re.escape(h) + r"(?![A-Za-z0-9_.-])", re.I) for h in variants] if home else []
        # the user name, and its 8.3 short form (JOHNSM~1); also right after a URL-encoded \ or /
        names = [re.escape(n) for n in (user, Path(short).name if "~" in Path(short).name else "")
                 if n and len(n) >= 4]
        self.user = re.compile(rf"(?i)(?:(?<![A-Za-z0-9])|(?<=%5C)|(?<=%2F))(?:{'|'.join(names)})(?![A-Za-z0-9])") \
            if mask_user and names else None
        self.mask_email = mask_email
        self.entropy_check = entropy_check

    def __call__(self, text: str) -> str:
        if not text:
            return text
        for rx in self.homes:
            text = rx.sub("~", text)
        for rx in KNOWN:
            text = rx.sub(MASK, text)
        for rx, rep in BY_NAME:
            text = rx.sub(rep, text)
        if self.entropy_check:
            text = _entropy_sub(text)
        if self.mask_email:
            text = _EMAIL.sub(lambda m: MASK + "@" + m.group(0).split("@", 1)[1], text)
        if self.user:
            text = self.user.sub("user", text)
        return text


_default: Redactor | None = None


def scrub(text: str) -> str:
    """Redact with the defaults for this machine."""
    global _default
    if _default is None:
        _default = Redactor()
    return _default(text)
