#!/usr/bin/env python3
"""Render the README gallery from `daypeek --demo` (fake data only) into docs/*.png.

Usage:  scripts/screenshot.py            # every shot
        scripts/screenshot.py compact    # just the named shot(s)
Needs:  a release build (cargo build --release), Google Chrome/Chromium, and jq
        for the JSON shot.
"""
import html, os, re, shutil, subprocess, sys, tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(ROOT, "target", "release", "daypeek")
AT = "2026-10-21T10:58:00-04:00"  # fixed instant => reproducible images

THEMES = {
    # Catppuccin Mocha-ish
    "dark": dict(bg="#1e1e2e", bar="#181825", fg="#cdd6f4", title="#6c7086", prompt="#a6e3a1",
                 pal={30: "#6c7086", 31: "#f38ba8", 32: "#a6e3a1", 33: "#f9e2af", 34: "#89b4fa",
                      35: "#f5c2e7", 36: "#94e2d5", 37: "#bac2de"}),
    # Catppuccin Latte-ish
    "light": dict(bg="#eff1f5", bar="#dce0e8", fg="#4c4f69", title="#8c8fa1", prompt="#40a02b",
                  pal={30: "#8c8fa1", 31: "#d20f39", 32: "#40a02b", 33: "#df8e1d", 34: "#1e66f5",
                       35: "#8839ef", 36: "#179299", 37: "#5c5f77"}),
}

# name -> (shell command shown in the prompt, argv or shell string, theme)
SHOTS = {
    "screenshot":           ("daypeek", ["--demo"], "dark"),
    "screenshot-compact":   ("daypeek --compact", ["--demo", "--compact"], "dark"),
    "screenshot-light":     ("daypeek", ["--demo"], "light"),
    "screenshot-offline":   ("daypeek   # wifi off", ["--demo", "--demo-offline"], "dark"),
    "screenshot-providers": ("daypeek providers", ["--demo", "providers"], "dark"),
    "screenshot-json":      ("daypeek --json | jq '{weather: .weather.data.condition, top: .tasks[:2]}'",
                             "{bin} --demo --json --at {at} | jq -C '{{weather: .weather.data.condition, top: .tasks[:2]}}'",
                             "dark"),
}


def ansi_to_html(text, pal):
    out, state, pos = [], {}, 0
    for m in re.finditer(r"\x1b\[([0-9;]*)m", text):
        out.append(span(text[pos:m.start()], state))
        pos = m.end()
        codes = [int(c) for c in m.group(1).split(";") if c] or [0]
        i = 0
        while i < len(codes):
            c = codes[i]
            if c == 0: state = {}
            elif c == 1: state["b"] = True
            elif c == 2: state["dim"] = True
            elif c == 3: state["i"] = True
            elif c == 9: state["s"] = True
            elif c == 22: state.pop("b", None); state.pop("dim", None)
            elif c in pal: state["fg"] = pal[c]
            elif 90 <= c <= 97: state["fg"] = pal[c - 60]
            elif c == 39: state.pop("fg", None)
            elif c == 38 and codes[i + 1:i + 2] == [2]:
                r, g, b = codes[i + 2:i + 5]; state["fg"] = f"rgb({r},{g},{b})"; i += 4
            i += 1
    out.append(span(text[pos:], state))
    return "".join(out)


def span(s, st):
    if not s:
        return ""
    css = []
    if "fg" in st: css.append(f"color:{st['fg']}")
    if st.get("b"): css.append("font-weight:700")
    if st.get("dim"): css.append("opacity:.55")
    if st.get("i"): css.append("font-style:italic")
    if st.get("s"): css.append("text-decoration:line-through")
    e = html.escape(s)
    return f'<span style="{";".join(css)}">{e}</span>' if css else e


def capture(cmd):
    env = dict(os.environ, COLORTERM="truecolor", COLUMNS="84", TZ="America/New_York")
    if isinstance(cmd, str):
        res = subprocess.run(cmd.format(bin=BIN, at=AT), shell=True, capture_output=True, text=True, env=env, check=True)
    else:
        res = subprocess.run([BIN, "--color", "always", "--at", AT] + cmd, capture_output=True, text=True, env=env, check=True)
    return res.stdout.rstrip("\n").lstrip("\n")


def render(name, shown, cmd, theme, chrome):
    t = THEMES[theme]
    text = capture(cmd)
    lines = text.split("\n")
    longest = max(len(re.sub(r"\x1b\[[0-9;]*m", "", l)) for l in lines + [shown])
    width_px = max(900, 36 + 36 + int(8.45 * longest))
    height_px = 30 + 8 + 21 * (len(lines) + 2) + 18 + 36
    prompt = f'<span style="color:{t["prompt"]}">❯</span> {html.escape(shown)}\n\n'
    page = f"""<!doctype html><meta charset="utf-8"><style>
body{{margin:0;background:transparent}}
.win{{margin:18px;background:{t['bg']};border-radius:12px;box-shadow:0 10px 30px rgba(0,0,0,.35);overflow:hidden}}
.bar{{height:30px;background:{t['bar']};display:flex;align-items:center;padding-left:14px;gap:8px}}
.dot{{width:12px;height:12px;border-radius:50%}}
.title{{color:{t['title']};font:12px -apple-system,Helvetica,sans-serif;margin-left:auto;margin-right:auto;transform:translateX(-30px)}}
pre{{margin:0;padding:8px 18px 18px;color:{t['fg']};font:14px/21px "JetBrains Mono","DejaVu Sans Mono",Menlo,monospace}}
</style><div class="win"><div class="bar"><span class="dot" style="background:#f38ba8"></span>
<span class="dot" style="background:#f9e2af"></span><span class="dot" style="background:#a6e3a1"></span>
<span class="title">~ — daypeek</span></div>
<pre>{prompt}{ansi_to_html(text, t['pal'])}</pre></div>"""
    docs = os.path.join(ROOT, "docs")
    html_path = os.path.join(docs, name + ".html")
    with open(html_path, "w") as f:
        f.write(page)
    with tempfile.TemporaryDirectory() as profile:
        subprocess.run([chrome, "--headless=new", "--disable-gpu", "--hide-scrollbars", "--no-sandbox",
                        f"--user-data-dir={profile}", "--force-device-scale-factor=2",
                        "--default-background-color=00000000", f"--window-size={width_px},{height_px}",
                        f"--screenshot={os.path.join(docs, name + '.png')}", "file://" + html_path],
                       check=True, capture_output=True)
    print(f"wrote docs/{name}.png")


def main():
    if not os.path.exists(BIN):
        sys.exit("build first: cargo build --release")
    chrome = next((c for c in ["google-chrome", "chromium", "chromium-browser"] if shutil.which(c)), None)
    if not chrome:
        sys.exit("install Chrome/Chromium to render the PNGs")
    wanted = [a if a.startswith("screenshot") else f"screenshot-{a}" for a in sys.argv[1:]] or list(SHOTS)
    for name in wanted:
        if name not in SHOTS:
            sys.exit(f"unknown shot {name}; choose from {', '.join(SHOTS)}")
        render(name, *SHOTS[name], chrome)


if __name__ == "__main__":
    main()
