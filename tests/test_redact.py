"""Feed realistic fake secrets through the redactor; none may survive.

Fake values are built from pieces ("sk_" + "live_" ...) so secret scanners on code hosts
don't flag this file. None of them are real.
"""
import unittest

from glitchwall import redact
from glitchwall.redact import MASK, Redactor, entropy, looks_random

R = Redactor(home=r"C:\Users\octocat", user="octocat")
P = Redactor(home="/home/octocat", user="octocat")

B64 = "q8ZpT3vN7wXb2LmR9sKd4FhJ6yUcE1aG"      # 32 chars, random-looking
HEX = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"

SECRETS = {
    "aws access key": "AKIA" + "QX7Z5T3NBM4WPL2R",
    "aws secret env": "export AWS_SECRET_ACCESS_KEY=" + "wJalrXUtnFEMI/K7MDENG/bPxRfiCY" + "EXAMPLEKEY",
    "google api key": "AIza" + "SyD4-xq9Lm2Rk7Pz0Wv3Ny8Bt6Hc1Jf5Ge0",
    "github pat": "ghp_" + "a1B2c3D4e5F6g7H8i9J0k1L2m3N4o5P6q7R8",
    "github fine pat": "github_pat_" + "11ABCDEFG0" + "a" * 20 + "_" + "b" * 30 + "Z9",
    "gitlab pat": "glpat-" + "xY7k2Lm9Qw3Ez5Rt8Uv1",
    "slack bot": "xoxb-" + "123456789012-1234567890123-AbCdEfGhIjKlMnOpQrStUvWx",
    "slack webhook": "https://hooks.slack.com/services/" + "T0ABC1234/B0DEF5678/" + "xYz123AbC456dEf789GhI012",
    "discord webhook": "https://discord.com/api/webhooks/" + "123456789012345678/" + "AbCdEf_GhIjKl-MnOpQrStUvWxYz0123456789",
    "stripe live": "sk_" + "live_" + "51HxQ2bKz8Lm3Np4Qr5St6Uv7Wx8Yz9",
    "stripe restricted": "rk_" + "test_" + "51HxQ2bKz8Lm3Np4Qr5St6Uv7Wx8Yz9",
    "stripe webhook": "whsec_" + "Ab12Cd34Ef56Gh78Ij90Kl12Mn34Op56",
    "anthropic": "sk-ant-" + "api03-" + "Zx9Yw8Vu7Ts6Rq5Po4Nm3Lk2Ji1Hg0Fe9Dc8Ba7",
    "openai": "sk-proj-" + "Ab1Cd2Ef3Gh4Ij5Kl6Mn7Op8Qr9St0Uv1Wx2",
    "huggingface": "hf_" + "AbCdEfGhIjKlMnOpQrStUvWxYz12345678",
    "npm": "npm_" + "Ab1Cd2Ef3Gh4Ij5Kl6Mn7Op8Qr9St0Uv1Wx2Y",
    "pypi": "pypi-" + "AgEIcHlwaS5vcmc" + "CJGZmY2E3ZTk0LTIzOTgtNDk5ZS1iZDAxLWNkNmVhYjU0ZmM0YQ",
    "digitalocean": "dop_v1_" + HEX,
    "vault": "hvs." + "CAESIJx9Y8wV7uT6sR5qP4oN3mL2kJ1iH0gF",
    "tailscale": "tskey-" + "auth-kAbCdE1CNTRL-" + "aBcDeFgHiJkLmNoPqRsTuVwXyZ123456",
    "sendgrid": "SG." + "Ab1Cd2Ef3Gh4Ij5Kl6Mn7O" + "." + "Pq8Rs9Tu0Vw1Xy2Za3Bc4De5Fg6Hi7Jk8Lm9No0Pq1Rs",
    "telegram": "123456789:" + "AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw",
    "jwt": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9" + ".eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4ifQ" + ".SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c",
    "pem key": "-----BEGIN RSA " + "PRIVATE KEY-----\nMIIEowIBAAKCAQEA0Z3VS5JJcds3xfn/ygWyF8PbnGy\nabc\n-----END RSA PRIVATE KEY-----",
    "openssh key": "-----BEGIN OPENSSH " + "PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQ\n-----END OPENSSH PRIVATE KEY-----",
    "unterminated key": "-----BEGIN " + "PRIVATE KEY-----\nMIIEvQIBADANBgkqhkiG9w0BAQEFAASC\n…",
    "age key": "AGE-SECRET-KEY-1" + "QQPZ5LN8Y0A3VQKV5YSJ3MRSMFXXWDR7C3HU6K4TJ2CX8Z0YNFQ9S8LA4ME",
    "postgres url": "postgres://app:" + "S3cr3tPass!" + "@db.internal:5432/app",
    "mongodb srv": "mongodb+srv://admin:" + "hunter2xyz" + "@cluster0.abcde.mongodb.net/test",
    "redis url": "rediss://default:" + "pAsSw0rd99" + "@cache.example.test:6380",
    "https creds": "git clone https://octo:" + "tokenvalue123" + "@git.example.test/x.git",
    "ado.net": "Server=db;Database=app;User Id=sa;Password=" + "Pa55w0rd!x" + ";",
    "azure storage": "DefaultEndpointsProtocol=https;AccountName=acct;AccountKey=" + "Zm9vYmFyYmF6cXV4cXV1eGNvcmdlZ3JhdWx0" + "==;",
    "azure sas": "https://acct.blob.core.windows.net/c/f?sv=2022-11-02&sig=" + "aBcD1234%2BeFgH5678%3D",
    "password assign": "DB_PASSWORD=" + "correct-horse-battery",
    "password quoted": "password = '" + "my secret phrase" + "'",
    "json secret": '{"client_secret": "' + "s3cr3t-v4lue-xyz" + '"}',
    "yaml token": "api_token: " + "tok_9f8e7d6c5b4a",
    "export token": "export GITHUB_TOKEN=" + "plainvalue123",
    "powershell env": "$env:OPENAI_API_KEY = '" + "valuevalue" + "'",
    "setx": "setx SERVICE_TOKEN " + "valuevalue2",
    "cli flag": "mytool login --password " + "hunter2hunter2",
    "cli flag eq": "deploy --api-key=" + "abc123def456",
    "mysql -p": "mysql -u root -p" + "Sup3rS3cret db",
    "curl -u": "curl -u admin:" + "adminpass1" + " https://x.test",
    "auth header": "curl -H 'Authorization: Bearer " + "abc.def.ghi123456" + "' https://x.test",
    "basic header": "Authorization: Basic " + "YWRtaW46cGFzc3dvcmQ=",
    "x-api-key": "-H 'X-Api-Key: " + "k3yk3yk3y" + "'",
    "cookie": "Cookie: session=" + "abcd1234; theme=dark",
    "netrc": "machine example.test login octo password " + "netrcpass9",
    "high entropy bare": "the value is " + B64 + " ok",
    "hex blob": "digest " + HEX,
    "vault pass": "ansible_become_pass=" + "becomeme123",
    "sshpass": "sshpass -p '" + "Tr0ub4dor&3" + "' ssh admin@box",
    "sshpass attached": "sshpass -e -p" + "sshp4ssword" + " scp a b:",
    "docker login": "docker login -u bob -p " + "D0ckerPw" + " registry.example.test",
    "htpasswd -b": "htpasswd -cb .htpasswd bob " + "htpassw0rd",
    "htpasswd -nb": "htpasswd -nb bob " + "htnopass1" + " | tee x",
    "net user": "net user bob " + "N3tUserPw" + " /add",
    "securestring": "ConvertTo-SecureString '" + "Plain!Pass1" + "' -AsPlainText -Force",
    "aws secret bare": "wJalrXUtnFEMI/K7MDENG/bPxRfiCY" + "EXAMPLEKEY",
    "aws secret csv": "AKIA" + "QX7Z5T3NBM4WPL2R," + "wJalrXUtnFEMI/K7MDENG/bPxRfiCY" + "EXAMPLEKEY",
    "cmdkey": "cmdkey /add:srv /user:bob /pass:" + "Cmdk3yPass",
    "net use": r"net use Z: \\srv\share " + "N3tUsePass" + " /user:bob",
    "7z": "7z a -p" + "Sev3nZipPw" + " out.7z src",
    "zip -P": "zip -r -P " + "Z1pPassw0" + " out.zip src",
    "wifi key": "    Key Content            : " + "W1fiPassphrase",
}

# the part of each sample that must not appear in the output
LEAK = {
    "aws secret env": "wJalrXUtnFEMI", "postgres url": "S3cr3tPass", "mongodb srv": "hunter2xyz",
    "redis url": "pAsSw0rd99", "https creds": "tokenvalue123", "ado.net": "Pa55w0rd", "azure storage": "Zm9vYmFy",
    "azure sas": "aBcD1234", "password assign": "correct-horse", "password quoted": "secret phrase",
    "json secret": "s3cr3t", "yaml token": "tok_9f8e", "export token": "plainvalue123", "powershell env": "valuevalue",
    "setx": "valuevalue2", "cli flag": "hunter2hunter2", "cli flag eq": "abc123def456", "mysql -p": "Sup3rS3cret",
    "curl -u": "adminpass1", "auth header": "abc.def.ghi", "basic header": "YWRtaW46", "x-api-key": "k3yk3yk3y",
    "cookie": "abcd1234", "netrc": "netrcpass9", "high entropy bare": B64, "hex blob": HEX,
    "pem key": "MIIEowIBAAKCAQEA", "openssh key": "b3BlbnNzaC1rZXkt", "unterminated key": "MIIEvQIBADAN",
    "vault pass": "becomeme123", "slack webhook": "xYz123AbC456", "discord webhook": "AbCdEf_GhIjKl",
    "sshpass": "Tr0ub4dor", "sshpass attached": "sshp4ssword", "docker login": "D0ckerPw", "htpasswd -b": "htpassw0rd",
    "htpasswd -nb": "htnopass1", "net user": "N3tUserPw", "securestring": "Plain!Pass1",
    "aws secret bare": "wJalrXUtnFEMI", "aws secret csv": "wJalrXUtnFEMI", "cmdkey": "Cmdk3yPass",
    "net use": "N3tUsePass", "7z": "Sev3nZipPw", "zip -P": "Z1pPassw0", "wifi key": "W1fiPassphrase",
}


class TestSecrets(unittest.TestCase):
    def test_every_secret_is_masked(self):
        for name, sample in SECRETS.items():
            with self.subTest(name):
                out = R(sample)
                needle = LEAK.get(name, sample)
                self.assertNotIn(needle, out, f"{name} leaked: {out!r}")
                self.assertIn(MASK, out, name)

    def test_secrets_inside_larger_output(self):
        blob = "\n".join(f"line {i}: {s}" for i, s in enumerate(SECRETS.values()))
        out = R(blob)
        for name, sample in SECRETS.items():
            self.assertNotIn(LEAK.get(name, sample), out, name)

    def test_ordinary_text_is_left_alone(self):
        keep = [
            "git status -sb",
            "pytest -q tests/test_routes.py",
            "Get-ChildItem -Recurse src | Select-Object -First 5",
            "docker compose up -d --build",
            "src/glitchwall/redact.py",
            "ThreadingHTTPServer(('127.0.0.1', 47833), Handler)",
            "[main 9c1e4b7] list endpoint returns totals",
            "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}",
            "550e8400-e29b-41d4-a716-446655440000",
            "cpython-3.14-windows-x86_64-none",
            "30 passed in 1.92s",
            "Author: Octo Cat",
            "https://docs.python.org/3/library/http.server.html",
            "src/api/auth.py:41: # TODO rotate session keys",
            "tokenizer.py:12:def split(text):",
            "C:/Program Files/Some App/App.exe",
            "-     if not session_id:\n+     if session_id is None:",   # a diff: the "+" line is not a value
            "docker login registry.example.test",
            "htpasswd -c .htpasswd bob",
            "net user bob /delete",
            "sshpass -f pwfile ssh admin@box",
            "src/Components/Button2/Index3/Helpers999",           # 40 characters with slashes, but words
            r"net use Z: \\srv\share /user:bob",
            r"net use Z: \\srv\share * /user:bob",
            "cmdkey /list",
            "7z x archive.7z -ooutdir",
            "https://example.test/api/token/refresh",
        ]
        for s in keep:
            with self.subTest(s):
                self.assertEqual(R(s), s)


class TestPersonal(unittest.TestCase):
    def test_windows_home_becomes_tilde(self):
        self.assertEqual(R(r"type C:\Users\octocat\notes.txt"), r"type ~\notes.txt")
        self.assertEqual(R("cat C:/Users/octocat/notes.txt"), "cat ~/notes.txt")
        self.assertEqual(R("ls /c/Users/octocat/code"), "ls ~/code")
        self.assertEqual(R(r'{"p": "C:\\Users\\octocat\\x"}'), r'{"p": "~\\x"}')
        self.assertEqual(R(r"c:\users\OCTOCAT\x"), r"~\x")

    def test_posix_home_becomes_tilde(self):
        self.assertEqual(P("cd /home/octocat/code && ls"), "cd ~/code && ls")
        self.assertEqual(P("/home/octocatx/y"), "/home/octocatx/y")   # a different user's home

    def test_user_name_masked(self):
        self.assertEqual(R("whoami\ndesk\\octocat"), "whoami\ndesk\\user")

    def test_url_encoded_user_masked(self):
        self.assertNotIn("octocat", R("file=C%3A%5CUsers%5Coctocat%5Cx").lower())

    def test_windows_short_home(self):
        # temp paths often use the 8.3 form, C:\Users\OCTOCA~1, which still gives away the start of the name
        real = redact._short_home
        redact._short_home = lambda h: r"C:\Users\OCTOCA~1" if h == r"C:\Users\octocat" else ""
        try:
            r = Redactor(home=r"C:\Users\octocat", user="octocat")
        finally:
            redact._short_home = real
        self.assertEqual(r(r"C:\Users\OCTOCA~1\AppData\Local\Temp\x"), r"~\AppData\Local\Temp\x")
        self.assertEqual(r("C:/Users/OCTOCA~1/x"), "~/x")
        self.assertEqual(r(r"desk\OCTOCA~1"), r"desk\user")

    def test_email_masked(self):
        self.assertEqual(R("Author: Octo <octo.cat@example.com>"), f"Author: Octo <{MASK}@example.com>")

    def test_short_user_not_masked_everywhere(self):
        r = Redactor(home="/home/al", user="al")
        self.assertEqual(r("al is in alpha"), "al is in alpha")


class TestEntropy(unittest.TestCase):
    def test_entropy_values(self):
        self.assertLess(entropy("aaaaaaaa"), 0.1)
        self.assertGreater(entropy(B64), 4.0)

    def test_looks_random(self):
        self.assertTrue(looks_random(B64))
        self.assertFalse(looks_random("ThisIsAVeryLongClassName"))
        self.assertFalse(looks_random("some_long_function_name_here"))
        self.assertFalse(looks_random("src/glitchwall/redact"))
        self.assertTrue(looks_random("abc/" + B64))


class TestSpeed(unittest.TestCase):
    """Long text with no spaces once made one pattern search again from every '-' or '.' (20 KB took 37 s,
    and the feed stood still meanwhile). The feed redacts at most server.SCAN characters of an event."""

    def test_pathological_text_is_quick(self):
        import time
        from glitchwall.server import SCAN
        r = Redactor(home="C:\\Users\\someone", user="someone")
        for text in ("a-" * SCAN, "a." * SCAN, "a." * SCAN + "@", "mysql " * SCAN, "7z a " * SCAN, "a://b:" * SCAN):
            t = time.perf_counter()
            r(text[:SCAN])
            self.assertLess(time.perf_counter() - t, 0.5, text[:12])

    def test_long_names_still_masked(self):
        name = "MY_" + "LONG_" * 12 + "PASSWORD"   # 63 characters before the word
        self.assertNotIn("hunter2x", Redactor()(f"export {name}=hunter2x"))


if __name__ == "__main__":
    unittest.main()
