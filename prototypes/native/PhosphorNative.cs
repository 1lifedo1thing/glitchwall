// glitchwall native renderer prototype: the phosphor theme drawn with Direct2D + DirectWrite, behind the icons.
//
// Question it answers: how much CPU and memory does a native renderer need for what the WebView2 page draws?
// Not a product: one theme, one screen, made-up events, system font (Consolas), no card blur.
//
// Build (no SDK needed, the C# compiler that ships with Windows' .NET Framework 4.8):
//   C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe /nologo /optimize /target:winexe /platform:x64
//       /out:phosphor-native.exe /r:System.Windows.Forms.dll /r:System.Drawing.dll PhosphorNative.cs
// Run: phosphor-native.exe          (stop glitchwall first: both sit at the same spot behind the icons)
// Quit: phosphor-native.exe --stop  (or end it in Task Manager)
// phosphor-native.exe --offscreen: an ordinary window out of sight instead, for screen grabs (PrintWindow)
//
// C# 5 (what that compiler speaks), Direct2D 1.0 called through its COM vtables: no NuGet, no downloads.
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Threading;
using System.Windows.Forms;

static class Native
{
    // ---------------------------------------------------------------- Win32
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindowW(string cls, string title);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr parent, IntPtr after, string cls, string title);
    [DllImport("user32.dll")] public static extern IntPtr SendMessageTimeoutW(IntPtr h, uint msg, IntPtr w, IntPtr l, uint flags, uint timeout, out IntPtr res);
    [DllImport("user32.dll")] public static extern IntPtr SetParent(IntPtr child, IntPtr parent);
    [DllImport("user32.dll")] public static extern IntPtr GetWindowLongPtrW(IntPtr h, int i);
    [DllImport("user32.dll")] public static extern IntPtr SetWindowLongPtrW(IntPtr h, int i, IntPtr v);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int hh, uint f);
    [DllImport("user32.dll")] public static extern bool MapWindowPoints(IntPtr from, IntPtr to, int[] pts, int n);
    [DllImport("user32.dll")] public static extern bool EnableWindow(IntPtr h, bool on);
    [DllImport("user32.dll")] public static extern IntPtr SetProcessDpiAwarenessContext(IntPtr v);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
    [DllImport("kernel32.dll")] public static extern IntPtr CreateWaitableTimerExW(IntPtr a, IntPtr name, uint flags, uint access);
    [DllImport("kernel32.dll")] public static extern bool SetWaitableTimer(IntPtr t, ref long due, int period, IntPtr f, IntPtr a, bool resume);
    [DllImport("kernel32.dll")] public static extern uint WaitForSingleObject(IntPtr h, uint ms);

    // ---------------------------------------------------------------- Direct2D / DirectWrite
    [DllImport("d2d1.dll")] public static extern int D2D1CreateFactory(int type, ref Guid iid, IntPtr opts, out IntPtr factory);
    [DllImport("dwrite.dll")] public static extern int DWriteCreateFactory(int type, ref Guid iid, out IntPtr factory);

    // a COM method, by its place in the object's vtable
    public static T Fn<T>(IntPtr obj, int slot) where T : class
    {
        IntPtr vt = Marshal.ReadIntPtr(obj);
        return Marshal.GetDelegateForFunctionPointer(Marshal.ReadIntPtr(vt, slot * IntPtr.Size), typeof(T)) as T;
    }
    public static void Check(int hr, string what) { if (hr < 0) throw new Exception(what + " failed: 0x" + hr.ToString("x8")); }
}

[StructLayout(LayoutKind.Sequential)] struct ColorF { public float r, g, b, a; public ColorF(float r, float g, float b, float a) { this.r = r; this.g = g; this.b = b; this.a = a; } }
[StructLayout(LayoutKind.Sequential)] struct RectF { public float l, t, r, b; public RectF(float l, float t, float r, float b) { this.l = l; this.t = t; this.r = r; this.b = b; } }
[StructLayout(LayoutKind.Sequential)] struct SizeU { public uint w, h; }
[StructLayout(LayoutKind.Sequential)] struct SizeF { public float w, h; }
[StructLayout(LayoutKind.Sequential)] struct PixelFormat { public int format, alpha; }
[StructLayout(LayoutKind.Sequential)] struct RTProps { public int type; public PixelFormat pf; public float dpiX, dpiY; public int usage, minLevel; }
[StructLayout(LayoutKind.Sequential)] struct HwndRTProps { public IntPtr hwnd; public SizeU size; public int present; }
[StructLayout(LayoutKind.Sequential)] struct BitmapProps { public PixelFormat pf; public float dpiX, dpiY; }

// the vtable slots used (d2d1.h / dwrite.h order)
delegate int CreateHwndRT(IntPtr self, ref RTProps p, ref HwndRTProps hp, out IntPtr rt);              // ID2D1Factory 14
delegate int CreateSolidBrush(IntPtr self, ref ColorF c, IntPtr props, out IntPtr brush);              // RenderTarget 8
delegate int CreateCompatibleRT(IntPtr self, ref SizeF size, ref SizeU px, IntPtr fmt, int opts, out IntPtr rt); // 12
delegate int CreateBitmap(IntPtr self, SizeU size, byte[] data, uint pitch, ref BitmapProps p, out IntPtr bmp);   // 4
delegate void FillRect(IntPtr self, ref RectF r, IntPtr brush);                                          // 17
delegate void DrawRect(IntPtr self, ref RectF r, IntPtr brush, float width, IntPtr style);               // 16
delegate void DrawBitmapFn(IntPtr self, IntPtr bmp, ref RectF dest, float opacity, int interp, IntPtr src); // 26
delegate void DrawTextFn(IntPtr self, [MarshalAs(UnmanagedType.LPWStr)] string s, uint len, IntPtr fmt, ref RectF r, IntPtr brush, int opts, int measuring); // 27
delegate void ClearFn(IntPtr self, ref ColorF c);                                                        // 47
delegate void BeginDrawFn(IntPtr self);                                                                  // 48
delegate int EndDrawFn(IntPtr self, IntPtr tag1, IntPtr tag2);                                           // 49
delegate int GetBitmapFn(IntPtr self, out IntPtr bmp);                                                   // BitmapRenderTarget 57
delegate void SetColorFn(IntPtr self, ref ColorF c);                                                     // SolidColorBrush 8
delegate void SetOpacityFn(IntPtr self, float o);                                                        // Brush 4
delegate void SetTextAA(IntPtr self, int mode);                                                          // RenderTarget 34
delegate int CreateTextFormatFn(IntPtr self, [MarshalAs(UnmanagedType.LPWStr)] string family, IntPtr coll, int weight, int style, int stretch, float size, [MarshalAs(UnmanagedType.LPWStr)] string locale, out IntPtr fmt); // IDWriteFactory 15
delegate int SetWordWrapFn(IntPtr self, int mode);                                                       // IDWriteTextFormat 5

class Target
{
    public IntPtr rt;
    public FillRect Fill; public DrawRect Stroke; public DrawBitmapFn Bitmap; public DrawTextFn Text;
    public ClearFn Clear; public BeginDrawFn Begin; public EndDrawFn End;
    public Target(IntPtr rt)
    {
        this.rt = rt;
        Fill = Native.Fn<FillRect>(rt, 17); Stroke = Native.Fn<DrawRect>(rt, 16); Bitmap = Native.Fn<DrawBitmapFn>(rt, 26);
        Text = Native.Fn<DrawTextFn>(rt, 27); Clear = Native.Fn<ClearFn>(rt, 47); Begin = Native.Fn<BeginDrawFn>(rt, 48); End = Native.Fn<EndDrawFn>(rt, 49);
    }
}

class Card
{
    public string proj, cmd, output; public float x, y, w, h; public int typed; public double born, done = -1, dying = -1;
}

class Wallpaper : Form
{
    public static bool Offscreen, Peek;
    // phosphor
    static readonly ColorF BG = Hex(0x04110a), SURFACE = Hex(0x0a1d12), INK = Hex(0x8cffb2), MUTED = Hex(0x4fae74),
                           DIM = Hex(0x1f6b40), ACC = Hex(0xffb02e), EDGE = Hex(0x357a51);
    const float FPS = 30, GLASS = .58f, CPS = 5 * 6;   // type speed 5 as the page reckons it (about 30 chars/s)
    const int SCALE = 3, MAXCARDS = 4;

    IntPtr d2d, dw, monoFmt, barFmt, brush, lowBmp, overlay;
    Target screen, low;
    SetColorFn setColor; SetOpacityFn setOpacity;
    int W, H, LW, LH;
    float[] colY, colV;
    readonly Random rnd = new Random();
    readonly List<Card> cards = new List<Card>();
    readonly Stopwatch clock = Stopwatch.StartNew();
    double nextEvent = 1;
    int evIndex;

    static readonly string[][] EVENTS = {
        new[] { "orbit", "git status -sb", "## main...origin/main\n M src/api/routes.py\n?? tests/test_totals.py" },
        new[] { "orbit", "pytest -q tests/test_totals.py", "....                                   [100%]\n4 passed in 0.41s" },
        new[] { "orbit", "read  src/api/routes.py", "def list_orders(req):\n    rows = db.orders.filter(user=req.user)\n    return {\"items\": rows, \"total\": len(rows)}" },
        new[] { "lumen", "npm run build", "vite v6.2.0 building for production...\n✓ 214 modules transformed.\ndist/index.html  0.46 kB" },
        new[] { "lumen", "edit  src/components/Chart.tsx", "-  const max = Math.max(...data)\n+  const max = Math.max(1, ...data)" },
        new[] { "orbit", "grep -rn TODO src/", "src/jobs/sync.py:12: # TODO retry on 429\nsrc/api/auth.py:41: # TODO rotate session keys" },
        new[] { "kiln", "cargo test --quiet", "running 18 tests\n..................\ntest result: ok. 18 passed; 0 failed" },
        new[] { "kiln", "write  docs/notes.md", "- [x] totals in list endpoint\n- [ ] retry on 429" },
    };

    static ColorF Hex(int v) { return new ColorF((v >> 16 & 255) / 255f, (v >> 8 & 255) / 255f, (v & 255) / 255f, 1); }
    static ColorF A(ColorF c, float a) { c.a = a; return c; }

    public Wallpaper()
    {
        FormBorderStyle = FormBorderStyle.None; ShowInTaskbar = false; StartPosition = FormStartPosition.Manual;
        W = Native.GetSystemMetrics(0); H = Native.GetSystemMetrics(1);
        Bounds = new System.Drawing.Rectangle(-32000, -32000, W, H); Text = "glitchwall native prototype";
    }

    protected override void OnShown(EventArgs e)
    {
        base.OnShown(e);
        if (Peek) { TopMost = true; Bounds = new System.Drawing.Rectangle(W - 980, H - 620, 960, 560); }   // --peek: a look, top right part of the drawing
        else if (!Offscreen) BehindIcons();   // --offscreen: a normal window out of sight, so it can be captured
        var t = new Thread(Run); t.IsBackground = true; t.Start();   // all drawing on one thread
    }

    // the same trick as winhost.py: a child of the desktop layer, just under the icons
    void BehindIcons()
    {
        IntPtr progman = Native.FindWindowW("Progman", null), res;
        Native.SendMessageTimeoutW(progman, 0x052C, (IntPtr)0xD, (IntPtr)1, 0, 1000, out res);
        IntPtr parent = progman, after = Native.FindWindowExW(progman, IntPtr.Zero, "SHELLDLL_DefView", null);
        if (after == IntPtr.Zero)
        {   // before Windows 11 24H2: the WorkerW after the one holding the icons
            IntPtr icons = IntPtr.Zero;
            Native.EnumWindows((h, l) => { if (Native.FindWindowExW(h, IntPtr.Zero, "SHELLDLL_DefView", null) != IntPtr.Zero) icons = h; return true; }, IntPtr.Zero);
            IntPtr worker = icons != IntPtr.Zero ? Native.FindWindowExW(IntPtr.Zero, icons, "WorkerW", null) : IntPtr.Zero;
            if (worker != IntPtr.Zero) parent = worker;
        }
        IntPtr h0 = Handle;
        long st = (long)Native.GetWindowLongPtrW(h0, -16);
        Native.SetWindowLongPtrW(h0, -16, (IntPtr)((st | 0x40000000L) & ~0x80000000L & ~0x00C00000L & ~0x00040000L));
        Native.SetParent(h0, parent);
        var pts = new[] { 0, 0, W, H };
        Native.MapWindowPoints(IntPtr.Zero, parent, pts, 2);
        Native.SetWindowPos(h0, after, pts[0], pts[1], W, H, 0x40 | 0x20 | 0x10);
        Native.EnableWindow(h0, false);
    }

    void Setup()
    {
        Guid iidD2D = new Guid("06152247-6f50-465a-9245-118bfd3b6007"), iidDW = new Guid("b859ee5a-d838-4b5b-a2e8-1adc7d93db48");
        Native.Check(Native.D2D1CreateFactory(0, ref iidD2D, IntPtr.Zero, out d2d), "D2D1CreateFactory");
        Native.Check(Native.DWriteCreateFactory(0, ref iidDW, out dw), "DWriteCreateFactory");
        var p = new RTProps { dpiX = 96, dpiY = 96 };
        var hp = new HwndRTProps { hwnd = Handle, size = new SizeU { w = (uint)W, h = (uint)H }, present = 2 };   // IMMEDIATELY: we pace ourselves
        IntPtr rt;
        Native.Check(Native.Fn<CreateHwndRT>(d2d, 14)(d2d, ref p, ref hp, out rt), "CreateHwndRenderTarget");
        screen = new Target(rt);
        Native.Fn<SetTextAA>(rt, 34)(rt, 2);   // grayscale text: it sits over a moving background
        var c = INK;
        Native.Check(Native.Fn<CreateSolidBrush>(rt, 8)(rt, ref c, IntPtr.Zero, out brush), "CreateSolidColorBrush");
        setColor = Native.Fn<SetColorFn>(brush, 8); setOpacity = Native.Fn<SetOpacityFn>(brush, 4);

        // the rain is drawn small and kept between frames (the fade), then blown up with hard pixel edges
        LW = W / SCALE; LH = H / SCALE;
        var sz = new SizeF { w = LW, h = LH }; var px = new SizeU { w = (uint)LW, h = (uint)LH };
        IntPtr lrt;
        Native.Check(Native.Fn<CreateCompatibleRT>(rt, 12)(rt, ref sz, ref px, IntPtr.Zero, 0, out lrt), "CreateCompatibleRenderTarget");
        low = new Target(lrt);
        Native.Check(Native.Fn<GetBitmapFn>(lrt, 57)(lrt, out lowBmp), "GetBitmap");
        low.Begin(lrt); var bg = BG; low.Clear(lrt, ref bg); low.End(lrt, IntPtr.Zero, IntPtr.Zero);
        int cols = LW / 6; colY = new float[cols]; colV = new float[cols];
        for (int i = 0; i < cols; i++) { colY[i] = (float)(rnd.NextDouble() * -LH); colV[i] = 4 + (float)rnd.NextDouble() * 7; }

        overlay = MakeOverlay(rt);
        IntPtr f;
        Native.Check(Native.Fn<CreateTextFormatFn>(dw, 15)(dw, "Consolas", IntPtr.Zero, 400, 0, 5, 16, "en-us", out f), "CreateTextFormat");
        monoFmt = f; Native.Fn<SetWordWrapFn>(f, 5)(f, 4);    // DWRITE_WORD_WRAPPING_CHARACTER: wrap at characters, like word-break:break-word
        Native.Check(Native.Fn<CreateTextFormatFn>(dw, 15)(dw, "Consolas", IntPtr.Zero, 700, 0, 5, 12, "en-us", out f), "CreateTextFormat");
        barFmt = f;
    }

    // the tube: scanlines every 3 px, a vignette and a little grain, all in one premultiplied bitmap made once
    IntPtr MakeOverlay(IntPtr rt)
    {
        var data = new byte[W * H * 4];
        var r = new Random(7);
        for (int y = 0; y < H; y++)
            for (int x = 0; x < W; x++)
            {
                float dx = (x - W / 2f) / (W / 2f), dy = (y - H / 2f) / (H / 2f);
                float a = Math.Min(1, Math.Max(0, (dx * dx + dy * dy - .45f) * .55f));   // vignette
                if (y % 3 == 0) a = Math.Max(a, .22f);                                    // scanline
                float g = (float)r.NextDouble() * .06f;                                   // grain: a little darker, here and there
                data[(y * W + x) * 4 + 3] = (byte)(Math.Min(1, a + g) * 255);              // black, premultiplied
            }
        var bp = new BitmapProps { pf = new PixelFormat { format = 87, alpha = 1 }, dpiX = 96, dpiY = 96 };   // BGRA, premultiplied
        IntPtr bmp;
        Native.Check(Native.Fn<CreateBitmap>(rt, 4)(rt, new SizeU { w = (uint)W, h = (uint)H }, data, (uint)(W * 4), ref bp, out bmp), "CreateBitmap");
        return bmp;
    }

    static readonly string LOG = System.IO.Path.Combine(System.IO.Path.GetTempPath(), "phosphor-native.log");
    static void Log(string s) { System.IO.File.AppendAllText(LOG, DateTime.Now.ToString("HH:mm:ss ") + s + Environment.NewLine); }
    int frames, badEnds; int lastHr;

    void Run()
    {
        try { Loop(); } catch (Exception e) { Log("render thread died: " + e); }
    }

    void Loop()
    {
        Setup();
        Log("set up: " + W + "x" + H + ", offscreen=" + Offscreen);
        IntPtr timer = Native.CreateWaitableTimerExW(IntPtr.Zero, IntPtr.Zero, 2, 0x1F0003);   // high resolution
        double last = clock.Elapsed.TotalSeconds;
        while (true)
        {
            double t0 = clock.Elapsed.TotalSeconds, dt = Math.Min(.1, t0 - last); last = t0;
            Frame(t0, (float)dt);
            if (++frames % 150 == 0) Log(frames + " frames, " + badEnds + " failed EndDraw (last 0x" + lastHr.ToString("x8") + ")");
            double rest = 1 / FPS - (clock.Elapsed.TotalSeconds - t0);
            if (rest > 0) { long due = -(long)(rest * 1e7); Native.SetWaitableTimer(timer, ref due, 0, IntPtr.Zero, IntPtr.Zero, false); Native.WaitForSingleObject(timer, 1000); }
        }
    }

    void Color(ColorF c) { setColor(brush, ref c); }

    void Frame(double now, float dt)
    {
        // 1. rain, into the small target (the page's "pixel" background, intensity 5)
        IntPtr l = low.rt;
        low.Begin(l);
        Color(A(BG, .18f)); var all = new RectF(0, 0, LW, LH); low.Fill(l, ref all, brush);
        for (int i = 0; i < colY.Length; i++)
        {
            colY[i] += colV[i] * dt * 9;
            if (colY[i] > LH + 40) { colY[i] = (float)(rnd.NextDouble() * -60); colV[i] = 4 + (float)rnd.NextDouble() * 7; }
            if (rnd.NextDouble() > .5 * 1.4) continue;
            int x = i * 6, y = (int)(colY[i] / 9) * 9;
            Color(rnd.NextDouble() < .08 ? INK : DIM);
            for (int r = 0; r < 7; r++)
            {
                int row = rnd.Next(32);
                for (int b = 0; b < 5; b++) if ((row >> b & 1) != 0) { var px = new RectF(x + b, y + r, x + b + 1, y + r + 1); low.Fill(l, ref px, brush); }
            }
        }
        low.End(l, IntPtr.Zero, IntPtr.Zero);

        // 2. the screen: rain blown up, cards, the tube on top
        IntPtr s = screen.rt;
        screen.Begin(s);
        var full = new RectF(0, 0, W, H);
        screen.Bitmap(s, lowBmp, ref full, 1, 0, IntPtr.Zero);   // nearest neighbour
        Cards(now, s);
        screen.Bitmap(s, overlay, ref full, 1, 1, IntPtr.Zero);
        int hr = screen.End(s, IntPtr.Zero, IntPtr.Zero);
        if (hr != 0) { badEnds++; lastHr = hr; }
    }

    void Cards(double now, IntPtr s)
    {
        if (now > nextEvent)
        {
            string[] ev = EVENTS[evIndex++ % EVENTS.Length];
            if (cards.Count >= MAXCARDS) cards[0].dying = cards[0].dying < 0 ? now : cards[0].dying;
            var c = new Card { proj = ev[0], cmd = ev[1], output = ev[2], born = now, w = 460, h = 170 };
            int slot = evIndex % MAXCARDS;
            c.x = 120 + (slot % 2) * 620 + rnd.Next(80); c.y = 140 + (slot / 2) * 330 + rnd.Next(60);
            cards.Add(c);
            nextEvent = now + 3.5 + rnd.NextDouble() * 3;
        }
        cards.RemoveAll(c => c.dying >= 0 && now - c.dying > .5);
        foreach (var c in cards)
        {
            string text = "> " + c.cmd + "\n" + c.output;
            if (c.typed < text.Length) c.typed = Math.Min(text.Length, (int)((now - c.born - .26) * CPS * (c.typed < c.cmd.Length + 2 ? 1 : 5)) + 1);
            else if (c.done < 0) c.done = now;
            if (c.done >= 0 && c.dying < 0 && now - c.done > 7) c.dying = now;   // linger 7 s
            float k = (float)Math.Min(1, (now - c.born) / .35);                  // arrive: a CRT line opening up
            if (c.dying >= 0) k = (float)Math.Max(0, 1 - (now - c.dying) / .5);
            float hh = c.h * k, top = c.y + (c.h - hh) / 2;
            var box = new RectF(c.x, top, c.x + c.w, top + hh);
            Color(A(SURFACE, GLASS)); screen.Fill(s, ref box, brush);
            Color(A(EDGE, .9f)); screen.Stroke(s, ref box, brush, 1, IntPtr.Zero);
            if (k < .9) continue;
            var bar = new RectF(c.x, top, c.x + c.w, top + 22);
            Color(A(DIM, .55f)); screen.Fill(s, ref bar, brush);
            var barText = new RectF(c.x + 10, top + 4, c.x + c.w - 10, top + 20);
            Color(INK); screen.Text(s, c.proj + " #a1f3", (uint)(c.proj.Length + 6), barFmt, ref barText, brush, 0, 0);
            string shown = c.typed > 0 ? text.Substring(0, c.typed) + (c.done < 0 && (int)(now * 2) % 2 == 0 ? "█" : "") : "";
            var area = new RectF(c.x + 12, top + 30, c.x + c.w - 12, top + hh - 10);
            // glow: the text drawn soft and offset a few times under the crisp copy (the page uses text-shadow)
            Color(A(INK, .16f));
            for (int g = 0; g < 4; g++)
            {
                var ga = area; float ox = g % 2 == 0 ? -1.5f : 1.5f, oy = g < 2 ? -1.5f : 1.5f;
                ga.l += ox; ga.r += ox; ga.t += oy; ga.b += oy;
                screen.Text(s, shown, (uint)shown.Length, monoFmt, ref ga, brush, 0, 0);
            }
            Color(INK); screen.Text(s, shown, (uint)shown.Length, monoFmt, ref area, brush, 0, 0);
        }
    }

    [STAThread]
    static void Main(string[] args)
    {
        if (args.Length > 0 && args[0] == "--stop")
        {
            foreach (var p in Process.GetProcessesByName(Process.GetCurrentProcess().ProcessName))
                if (p.Id != Process.GetCurrentProcess().Id) p.Kill();
            return;
        }
        Wallpaper.Offscreen = Array.IndexOf(args, "--offscreen") >= 0;
        Wallpaper.Peek = Array.IndexOf(args, "--peek") >= 0;
        Native.SetProcessDpiAwarenessContext((IntPtr)(-4));   // per-monitor v2: real pixels
        Application.Run(new Wallpaper());
    }
}
